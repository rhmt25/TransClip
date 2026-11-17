//! Named-pipe client, used by the settings process and by a second launch.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::windows::named_pipe::ClientOptions;
use tokio::sync::{mpsc, oneshot};

use super::protocol::{Event, Incoming, Request, MAX_LINE};

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>;

/// A connection to the daemon.
pub struct IpcClient {
    out: mpsc::Sender<String>,
    pending: Pending,
    next_id: AtomicU64,
}

impl IpcClient {
    /// Connect. Events pushed by the daemon arrive on the returned receiver.
    pub async fn connect(name: &str) -> std::io::Result<(Self, mpsc::Receiver<Event>)> {
        let pipe = connect_with_retry(name).await?;
        let (read, mut write) = tokio::io::split(pipe);
        let (out_tx, mut out_rx) = mpsc::channel::<String>(32);
        tokio::spawn(async move {
            while let Some(line) = out_rx.recv().await {
                if write.write_all(line.as_bytes()).await.is_err()
                    || write.write_all(b"\n").await.is_err()
                {
                    break;
                }
            }
        });
        let pending: Pending = Arc::default();
        let (ev_tx, ev_rx) = mpsc::channel(64);
        let pending2 = Arc::clone(&pending);
        tokio::spawn(async move {
            let mut reader = BufReader::new(read);
            let mut line = String::new();
            loop {
                line.clear();
                let mut limited = (&mut reader).take(MAX_LINE as u64 + 1);
                match limited.read_line(&mut line).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) if n > MAX_LINE => break,
                    Ok(_) => {}
                }
                match serde_json::from_str::<Incoming>(line.trim()) {
                    Ok(Incoming::Response(r)) => {
                        let tx = pending2
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .remove(&r.id);
                        if let Some(tx) = tx {
                            let _ = tx.send(match r.error {
                                Some(e) => Err(e),
                                None => Ok(r.result.unwrap_or(Value::Null)),
                            });
                        }
                    }
                    Ok(Incoming::Event(e)) => {
                        let _ = ev_tx.send(e).await;
                    }
                    Err(_) => {}
                }
            }
            // Connection ended: fail every request still waiting.
            let waiting: Vec<_> = pending2
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .drain()
                .collect();
            for (_, tx) in waiting {
                let _ = tx.send(Err("connection closed".into()));
            }
        });
        Ok((
            Self {
                out: out_tx,
                pending,
                next_id: AtomicU64::new(1),
            },
            ev_rx,
        ))
    }

    /// Call a method and wait for the answer (10 s limit).
    pub async fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id, tx);
        let line = serde_json::to_string(&Request {
            id,
            method: method.into(),
            params,
        })
        .map_err(|e| e.to_string())?;
        if self.out.send(line).await.is_err() {
            return Err("connection closed".into());
        }
        match tokio::time::timeout(Duration::from_secs(10), rx).await {
            Ok(Ok(r)) => r,
            Ok(Err(_)) => Err("connection closed".into()),
            Err(_) => {
                self.pending
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .remove(&id);
                Err("timeout".into())
            }
        }
    }
}

/// The pipe may be momentarily busy between instances; try for a short while.
async fn connect_with_retry(
    name: &str,
) -> std::io::Result<tokio::net::windows::named_pipe::NamedPipeClient> {
    let mut last = None;
    for _ in 0..20 {
        match ClientOptions::new().open(name) {
            Ok(c) => return Ok(c),
            Err(e) => {
                last = Some(e);
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("cannot connect")))
}

/// Connect, make one call and disconnect.
pub async fn one_shot(name: &str, method: &str, params: Value) -> Result<Value, String> {
    let (client, _events) = IpcClient::connect(name).await.map_err(|e| e.to_string())?;
    client.call(method, params).await
}
