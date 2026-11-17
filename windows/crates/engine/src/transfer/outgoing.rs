//! Sending side of a transfer.

use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use proto::ids::{new_uuid_v4, DeviceId};
use proto::msg::{EntryKind, Message};
use proto::path::validate_relative_path;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::sync::{mpsc, Semaphore};

use super::{
    Batch, BatchReport, OutMsg, TransferError, TransferEvent, TransferManager, CHUNK, MAX_PARALLEL,
};

/// An entry with the metadata that goes into `file_offer`.
struct Prepared {
    rel: String,
    kind: EntryKind,
    path: std::path::PathBuf,
    bytes: u64,
    sha256: String,
    mtime: u64,
}

/// Why one attempt at an entry ended.
enum Fail {
    Disconnected,
    Hash,
    Final(String),
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn prepare(rel: String, kind: EntryKind, path: &Path) -> std::io::Result<Prepared> {
    if kind == EntryKind::Dir {
        return Ok(Prepared {
            rel,
            kind,
            path: path.to_path_buf(),
            bytes: 0,
            sha256: String::new(),
            mtime: 0,
        });
    }
    let meta = std::fs::metadata(path)?;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    Ok(Prepared {
        rel,
        kind,
        path: path.to_path_buf(),
        bytes: meta.len(),
        sha256: hex(&hasher.finalize()),
        mtime,
    })
}

struct BatchMeta {
    id: String,
    name: String,
    count: u32,
    bytes: u64,
}

impl TransferManager {
    fn peer_limit(&self, peer: DeviceId) -> Arc<Semaphore> {
        Arc::clone(
            self.inner
                .state()
                .limits
                .entry(peer)
                .or_insert_with(|| Arc::new(Semaphore::new(MAX_PARALLEL))),
        )
    }

    fn is_cancelled(&self, batch_id: &str) -> bool {
        self.inner.state().cancelled.contains(batch_id)
    }

    /// Send a batch to `peer`. Resolves when every entry has been sent, refused or failed.
    /// Lost connections are retried (resuming where the receiver left off).
    pub async fn send_batch(
        &self,
        peer: DeviceId,
        batch: Batch,
    ) -> Result<BatchReport, TransferError> {
        if !self.inner.mgr.online_peers().contains(&peer) {
            return Err(TransferError::Offline);
        }
        if !batch.name.is_empty()
            && !validate_relative_path(&batch.name).is_ok_and(|s| s.len() == 1)
        {
            return Err(TransferError::Invalid("batch name".into()));
        }
        let _permit = self
            .peer_limit(peer)
            .acquire_owned()
            .await
            .map_err(|_| TransferError::Offline)?;

        let mut prepared = Vec::with_capacity(batch.entries.len());
        for e in batch.entries {
            validate_relative_path(&e.rel)
                .map_err(|err| TransferError::Invalid(format!("{}: {err}", e.rel)))?;
            let p = tokio::task::spawn_blocking(move || prepare(e.rel, e.kind, &e.path))
                .await
                .map_err(|_| TransferError::Invalid("worker failed".into()))??;
            prepared.push(p);
        }
        let meta = BatchMeta {
            id: new_uuid_v4().map_err(|e| TransferError::Invalid(e.to_string()))?,
            name: batch.name,
            count: u32::try_from(prepared.len()).unwrap_or(u32::MAX),
            bytes: prepared.iter().map(|p| p.bytes).sum(),
        };

        let (mut ok, mut failed) = (0u32, 0u32);
        for entry in &prepared {
            if self.is_cancelled(&meta.id) {
                failed += 1;
                continue;
            }
            match self.send_entry(peer, &meta, entry).await {
                Ok(()) => ok += 1,
                Err(reason) => {
                    tracing::debug!(entry = %entry.rel, reason = %reason, "entry failed");
                    failed += 1;
                }
            }
        }
        self.inner.state().cancelled.remove(&meta.id);
        self.inner
            .emit(TransferEvent::BatchFinished {
                batch_id: meta.id.clone(),
                peer,
                ok,
                failed,
                incoming: false,
            })
            .await;
        Ok(BatchReport {
            batch_id: meta.id,
            ok,
            failed,
        })
    }

    async fn wait_online(&self, peer: DeviceId) -> bool {
        let deadline = tokio::time::Instant::now() + self.inner.cfg.reconnect_wait;
        while tokio::time::Instant::now() < deadline {
            if self.inner.mgr.online_peers().contains(&peer) {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        false
    }

    async fn send_entry(
        &self,
        peer: DeviceId,
        meta: &BatchMeta,
        entry: &Prepared,
    ) -> Result<(), String> {
        let xfer_id = new_uuid_v4().map_err(|e| e.to_string())?;
        let (tx, mut rx) = mpsc::unbounded_channel();
        self.inner
            .state()
            .waiters
            .insert(xfer_id.clone(), (peer, tx));
        let (mut reconnects, mut hash_retries) = (0u32, 0u32);
        let result = loop {
            match self.try_entry(peer, meta, entry, &xfer_id, &mut rx).await {
                Ok(()) => break Ok(()),
                Err(Fail::Disconnected) => {
                    reconnects += 1;
                    if reconnects > self.inner.cfg.max_reconnects || !self.wait_online(peer).await {
                        break Err("disconnected".to_string());
                    }
                }
                Err(Fail::Hash) => {
                    hash_retries += 1;
                    if hash_retries > 1 {
                        break Err("hash".to_string());
                    }
                }
                Err(Fail::Final(reason)) => break Err(reason),
            }
        };
        self.inner.state().waiters.remove(&xfer_id);
        self.inner
            .emit(TransferEvent::Finished {
                xfer_id,
                peer,
                ok: result.is_ok(),
                reason: result.as_ref().err().cloned(),
                incoming: false,
            })
            .await;
        result
    }

    async fn recv(&self, rx: &mut mpsc::UnboundedReceiver<OutMsg>) -> Result<OutMsg, Fail> {
        match tokio::time::timeout(self.inner.cfg.reply_timeout, rx.recv()).await {
            Ok(Some(OutMsg::Disconnected)) | Ok(None) => Err(Fail::Disconnected),
            Ok(Some(m)) => Ok(m),
            Err(_) => Err(Fail::Final("timeout".into())),
        }
    }

    async fn try_entry(
        &self,
        peer: DeviceId,
        meta: &BatchMeta,
        entry: &Prepared,
        xfer_id: &str,
        rx: &mut mpsc::UnboundedReceiver<OutMsg>,
    ) -> Result<(), Fail> {
        while rx.try_recv().is_ok() {} // stale messages from an earlier attempt
        let offer = Message::FileOffer {
            xfer_id: xfer_id.to_string(),
            batch_id: meta.id.clone(),
            batch_name: meta.name.clone(),
            batch_count: meta.count,
            batch_bytes: meta.bytes,
            kind: entry.kind,
            relative_path: entry.rel.clone(),
            bytes: entry.bytes,
            sha256: entry.sha256.clone(),
            mtime: entry.mtime,
        };
        self.inner
            .mgr
            .send(&peer, offer, Vec::new())
            .await
            .map_err(|_| Fail::Disconnected)?;

        let offset = match self.recv(rx).await? {
            OutMsg::Accept(o) => o,
            OutMsg::Reject(r) => return Err(Fail::Final(r)),
            OutMsg::Cancel(r) => return Err(Fail::Final(r)),
            OutMsg::Result(ok, reason) if entry.kind == EntryKind::Dir => {
                return if ok {
                    Ok(())
                } else {
                    Err(Fail::Final(reason.unwrap_or_else(|| "io".into())))
                };
            }
            _ => return Err(Fail::Final("protocol".into())),
        };
        if entry.kind == EntryKind::Dir {
            // Directories are answered with a result instead of an accept.
            return Err(Fail::Final("protocol".into()));
        }
        if offset > entry.bytes {
            return Err(Fail::Final("protocol".into()));
        }
        self.inner
            .emit(TransferEvent::Started {
                xfer_id: xfer_id.to_string(),
                batch_id: meta.id.clone(),
                peer,
                name: entry.rel.clone(),
                bytes: entry.bytes,
                offset,
                incoming: false,
            })
            .await;
        self.stream_file(peer, meta, entry, xfer_id, offset, rx)
            .await?;

        match self.recv(rx).await? {
            OutMsg::Result(true, _) => Ok(()),
            OutMsg::Result(false, Some(r)) if r == "hash" => Err(Fail::Hash),
            OutMsg::Result(false, r) => Err(Fail::Final(r.unwrap_or_else(|| "io".into()))),
            OutMsg::Reject(r) | OutMsg::Cancel(r) => Err(Fail::Final(r)),
            _ => Err(Fail::Final("protocol".into())),
        }
    }

    async fn stream_file(
        &self,
        peer: DeviceId,
        meta: &BatchMeta,
        entry: &Prepared,
        xfer_id: &str,
        offset: u64,
        rx: &mut mpsc::UnboundedReceiver<OutMsg>,
    ) -> Result<(), Fail> {
        let io_err = |_| Fail::Final("io".into());
        let mut file = tokio::fs::File::open(&entry.path).await.map_err(io_err)?;
        file.seek(std::io::SeekFrom::Start(offset))
            .await
            .map_err(io_err)?;
        let link = self.inner.mgr.handle(&peer).ok_or(Fail::Disconnected)?;
        let disc = |_| Fail::Disconnected;
        link.send_data(
            Message::FileStream {
                xfer_id: xfer_id.to_string(),
                offset,
            },
            Vec::new(),
        )
        .await
        .map_err(disc)?;
        let mut pos = offset;
        let mut buf = vec![0u8; CHUNK];
        let mut last_report = offset;
        while pos < entry.bytes {
            // Stop early when the receiver cancelled or the link dropped.
            while let Ok(m) = rx.try_recv() {
                match m {
                    OutMsg::Disconnected => return Err(Fail::Disconnected),
                    OutMsg::Cancel(r) | OutMsg::Reject(r) => return Err(Fail::Final(r)),
                    _ => {}
                }
            }
            if self.is_cancelled(&meta.id) {
                let cancel = Message::FileCancel {
                    xfer_id: xfer_id.to_string(),
                    reason: "cancelled".into(),
                };
                let _ = self.inner.mgr.send(&peer, cancel, Vec::new()).await;
                return Err(Fail::Final("cancelled".into()));
            }
            let want = usize::try_from((entry.bytes - pos).min(CHUNK as u64)).unwrap_or(CHUNK);
            let n = file.read(&mut buf[..want]).await.map_err(io_err)?;
            if n == 0 {
                return Err(Fail::Final("io".into())); // the file shrank while sending
            }
            link.send_data(
                Message::FileChunk {
                    xfer_id: xfer_id.to_string(),
                    offset: pos,
                },
                buf[..n].to_vec(),
            )
            .await
            .map_err(disc)?;
            pos += n as u64;
            if pos - last_report >= 512 * 1024 {
                last_report = pos;
                self.inner.emit_progress(xfer_id, pos, entry.bytes);
            }
            if !self.inner.cfg.chunk_delay.is_zero() {
                tokio::time::sleep(self.inner.cfg.chunk_delay).await;
            }
        }
        link.send_data(
            Message::FileEnd {
                xfer_id: xfer_id.to_string(),
            },
            Vec::new(),
        )
        .await
        .map_err(disc)?;
        Ok(())
    }
}
