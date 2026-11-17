//! Receiving side of a transfer.

use proto::ids::DeviceId;
use proto::msg::{EntryKind, Message};
use proto::path::validate_relative_path;
use sha2::{Digest, Sha256};

use super::store::PartWriter;
use super::{TransferEvent, TransferManager};

/// An entry being received.
pub(crate) struct Incoming {
    pub peer: DeviceId,
    pub batch_id: String,
    pub name: String,
    pub bytes: u64,
    pub sha256: String,
    pub mtime: u64,
    pub writer: Box<dyn PartWriter>,
    pub hasher: Sha256,
    pub received: u64,
    pub streaming: bool,
    pub last_progress: u64,
}

/// Counters of one batch we receive.
pub(crate) struct BatchState {
    pub root: String,
    pub count: u32,
    pub ok: u32,
    pub failed: u32,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl TransferManager {
    async fn reply(&self, to: &DeviceId, message: Message) {
        let _ = self.inner.mgr.send(to, message, Vec::new()).await;
    }

    async fn reject(&self, from: &DeviceId, xfer_id: &str, reason: &str) {
        self.reply(
            from,
            Message::FileReject {
                xfer_id: xfer_id.to_string(),
                reason: reason.to_string(),
            },
        )
        .await;
    }

    /// Record the end of one entry and, when the batch is complete, tell the sender.
    async fn entry_done(
        &self,
        from: DeviceId,
        xfer_id: &str,
        batch_id: &str,
        ok: bool,
        reason: Option<&str>,
    ) {
        self.reply(
            &from,
            Message::FileResult {
                xfer_id: xfer_id.to_string(),
                ok,
                reason: reason.map(str::to_string),
            },
        )
        .await;
        self.inner
            .emit(TransferEvent::Finished {
                xfer_id: xfer_id.to_string(),
                peer: from,
                ok,
                reason: reason.map(str::to_string),
                incoming: true,
            })
            .await;
        let finished = {
            let mut st = self.inner.state();
            let Some(b) = st.batches.get_mut(batch_id) else {
                return;
            };
            if ok {
                b.ok += 1;
            } else {
                b.failed += 1;
            }
            (b.ok + b.failed >= b.count).then_some((b.ok, b.failed))
        };
        if let Some((ok_count, fail_count)) = finished {
            self.inner.state().batches.remove(batch_id);
            self.reply(
                &from,
                Message::BatchDone {
                    batch_id: batch_id.to_string(),
                    ok_count,
                    fail_count,
                },
            )
            .await;
            self.inner
                .emit(TransferEvent::BatchFinished {
                    batch_id: batch_id.to_string(),
                    peer: from,
                    ok: ok_count,
                    failed: fail_count,
                    incoming: true,
                })
                .await;
        }
    }

    pub(super) async fn on_offer(&self, from: DeviceId, message: Message) {
        let Message::FileOffer {
            xfer_id,
            batch_id,
            batch_name,
            batch_count,
            kind,
            relative_path,
            bytes,
            sha256,
            mtime,
            ..
        } = message
        else {
            return;
        };
        let cfg = &self.inner.cfg;
        if !cfg.receive_files {
            return self.reject(&from, &xfer_id, "disabled").await;
        }
        let Ok(rel) = validate_relative_path(&relative_path) else {
            return self.reject(&from, &xfer_id, "path").await;
        };
        // A batch name is one safe path segment (or empty for loose files).
        if !batch_name.is_empty()
            && !validate_relative_path(&batch_name).is_ok_and(|s| s.len() == 1)
        {
            return self.reject(&from, &xfer_id, "path").await;
        }
        if cfg.max_file_bytes > 0 && bytes > cfg.max_file_bytes {
            return self.reject(&from, &xfer_id, "quota").await;
        }
        if let Some(free) = self.inner.store.free_bytes() {
            if bytes > free {
                return self.reject(&from, &xfer_id, "disk_full").await;
            }
        }

        // The batch folder is decided once, by the first offer of the batch.
        let root = {
            let known = self
                .inner
                .state()
                .batches
                .get(&batch_id)
                .map(|b| b.root.clone());
            match known {
                Some(r) => r,
                None => match self.inner.store.begin_batch(&batch_name) {
                    Ok(r) => {
                        self.inner.state().batches.insert(
                            batch_id.clone(),
                            BatchState {
                                root: r.clone(),
                                count: batch_count,
                                ok: 0,
                                failed: 0,
                            },
                        );
                        r
                    }
                    Err(_) => return self.reject(&from, &xfer_id, "disk_full").await,
                },
            }
        };

        match kind {
            EntryKind::Dir => {
                let ok = self.inner.store.make_dir(&root, &rel).is_ok();
                self.inner
                    .emit(TransferEvent::Started {
                        xfer_id: xfer_id.clone(),
                        batch_id: batch_id.clone(),
                        peer: from,
                        name: relative_path,
                        bytes: 0,
                        offset: 0,
                        incoming: true,
                    })
                    .await;
                self.entry_done(from, &xfer_id, &batch_id, ok, (!ok).then_some("io"))
                    .await;
            }
            EntryKind::File => {
                let mut writer = match self.inner.store.open_part(&root, &rel) {
                    Ok(w) => w,
                    Err(_) => return self.reject(&from, &xfer_id, "disk_full").await,
                };
                // A part larger than the file cannot belong to it: start again.
                if writer.len() > bytes && writer.reset().is_err() {
                    return self.reject(&from, &xfer_id, "disk_full").await;
                }
                let mut hasher = Sha256::new();
                let offset = writer.len();
                if offset > 0 && writer.hash_existing(&mut hasher).is_err() {
                    return self.reject(&from, &xfer_id, "disk_full").await;
                }
                self.inner.state().incoming.insert(
                    xfer_id.clone(),
                    Incoming {
                        peer: from,
                        batch_id: batch_id.clone(),
                        name: relative_path.clone(),
                        bytes,
                        sha256,
                        mtime,
                        writer,
                        hasher,
                        received: offset,
                        streaming: false,
                        last_progress: offset,
                    },
                );
                self.inner
                    .emit(TransferEvent::Started {
                        xfer_id: xfer_id.clone(),
                        batch_id,
                        peer: from,
                        name: relative_path,
                        bytes,
                        offset,
                        incoming: true,
                    })
                    .await;
                self.reply(&from, Message::FileAccept { xfer_id, offset })
                    .await;
            }
        }
    }

    pub(super) async fn on_stream(&self, from: DeviceId, xfer_id: &str, offset: u64) {
        let ok = {
            let mut st = self.inner.state();
            match st.incoming.get_mut(xfer_id) {
                Some(i) if i.peer == from && i.received == offset => {
                    i.streaming = true;
                    true
                }
                _ => false,
            }
        };
        if !ok {
            self.abort_incoming(from, xfer_id, "protocol").await;
        }
    }

    pub(super) async fn on_chunk(&self, from: DeviceId, xfer_id: &str, offset: u64, data: &[u8]) {
        let problem = {
            let mut st = self.inner.state();
            match st.incoming.get_mut(xfer_id) {
                Some(i) if i.peer == from && i.streaming && i.received == offset => {
                    if i.received + data.len() as u64 > i.bytes {
                        Some("limit")
                    } else if i.writer.write(data).is_err() {
                        Some("io")
                    } else {
                        i.hasher.update(data);
                        i.received += data.len() as u64;
                        if i.received - i.last_progress >= 512 * 1024 {
                            i.last_progress = i.received;
                            self.inner.emit_progress(xfer_id, i.received, i.bytes);
                        }
                        None
                    }
                }
                _ => Some("protocol"),
            }
        };
        if let Some(reason) = problem {
            self.abort_incoming(from, xfer_id, reason).await;
        }
    }

    pub(super) async fn on_end(&self, from: DeviceId, xfer_id: &str) {
        let entry = {
            let mut st = self.inner.state();
            match st.incoming.get(xfer_id) {
                Some(i) if i.peer == from => st.incoming.remove(xfer_id),
                _ => None,
            }
        };
        let Some(i) = entry else {
            return self
                .reply(
                    &from,
                    Message::FileResult {
                        xfer_id: xfer_id.to_string(),
                        ok: false,
                        reason: Some("protocol".into()),
                    },
                )
                .await;
        };
        let Incoming {
            batch_id,
            name,
            bytes,
            sha256,
            mtime,
            writer,
            hasher,
            received,
            ..
        } = i;
        if received != bytes || hex(&hasher.finalize()) != sha256 {
            writer.abort();
            tracing::debug!(entry = %name, "received file failed verification");
            return self
                .entry_done(from, xfer_id, &batch_id, false, Some("hash"))
                .await;
        }
        self.inner.emit_progress(xfer_id, bytes, bytes);
        let done = tokio::task::spawn_blocking(move || writer.finish(mtime)).await;
        let ok = matches!(done, Ok(Ok(_)));
        self.entry_done(from, xfer_id, &batch_id, ok, (!ok).then_some("io"))
            .await;
    }

    pub(super) async fn on_cancel(&self, from: DeviceId, xfer_id: &str, reason: String) {
        let entry = {
            let mut st = self.inner.state();
            match st.incoming.get(xfer_id) {
                Some(i) if i.peer == from => st.incoming.remove(xfer_id),
                _ => None,
            }
        };
        if let Some(i) = entry {
            // The sender gave up for good: delete the part instead of leaving it for a resume.
            i.writer.abort();
            self.inner
                .emit(TransferEvent::Finished {
                    xfer_id: xfer_id.to_string(),
                    peer: from,
                    ok: false,
                    reason: Some(reason),
                    incoming: true,
                })
                .await;
            let finished = {
                let mut st = self.inner.state();
                st.batches.get_mut(&i.batch_id).map(|b| {
                    b.failed += 1;
                    (b.ok + b.failed >= b.count, b.ok, b.failed)
                })
            };
            if let Some((true, ok, failed)) = finished {
                self.inner.state().batches.remove(&i.batch_id);
                self.inner
                    .emit(TransferEvent::BatchFinished {
                        batch_id: i.batch_id,
                        peer: from,
                        ok,
                        failed,
                        incoming: true,
                    })
                    .await;
            }
        }
    }

    /// Stop an entry we receive because something went wrong; tells the sender.
    async fn abort_incoming(&self, from: DeviceId, xfer_id: &str, reason: &str) {
        let entry = self.inner.state().incoming.remove(xfer_id);
        if let Some(i) = entry {
            i.writer.abort();
            self.reply(
                &from,
                Message::FileCancel {
                    xfer_id: xfer_id.to_string(),
                    reason: reason.to_string(),
                },
            )
            .await;
            self.entry_done(from, xfer_id, &i.batch_id, false, Some(reason))
                .await;
        }
    }

    /// The user cancelled a transfer we are receiving.
    pub async fn cancel_incoming(&self, xfer_id: &str) {
        let peer = self.inner.state().incoming.get(xfer_id).map(|i| i.peer);
        if let Some(peer) = peer {
            self.abort_incoming(peer, xfer_id, "cancelled").await;
        }
    }
}
