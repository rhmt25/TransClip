//! Named-pipe server. The pipe is only accessible to the user who created it.

use std::ffi::c_void;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;
use windows::core::w;
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};

use super::protocol::{Event, Request, Response, MAX_LINE};

/// Answers one request.
pub type Handler = Arc<
    dyn Fn(String, Value) -> Pin<Box<dyn Future<Output = Result<Value, String>> + Send>>
        + Send
        + Sync,
>;

/// Running server; dropping it stops accepting connections.
pub struct IpcServer {
    accept: JoinHandle<()>,
    events: broadcast::Sender<Event>,
}

impl IpcServer {
    /// Create the pipe and start serving. Fails when another server already owns the name.
    pub fn start(name: &str, handler: Handler) -> std::io::Result<Self> {
        let first = create_instance(name, true)?;
        let (events, _) = broadcast::channel(64);
        let events2 = events.clone();
        let name = name.to_string();
        let accept = tokio::spawn(async move {
            let mut server = first;
            loop {
                if server.connect().await.is_err() {
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    continue;
                }
                let connected = server;
                server = match create_instance(&name, false) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::error!(error = %e, "cannot create the next pipe instance");
                        return;
                    }
                };
                tokio::spawn(serve(connected, Arc::clone(&handler), events2.subscribe()));
            }
        });
        Ok(Self { accept, events })
    }

    /// Push an event to every connected client.
    pub fn publish(&self, event: Event) {
        let _ = self.events.send(event);
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        self.accept.abort();
    }
}

fn create_instance(name: &str, first: bool) -> std::io::Result<NamedPipeServer> {
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: the SDDL string is a valid wide literal. "D:P(A;;GA;;;OW)" is a protected DACL
    // that grants full access only to the owner of the object, i.e. the current user.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            w!("D:P(A;;GA;;;OW)"),
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    }
    let mut attrs = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: false.into(),
    };
    let mut options = ServerOptions::new();
    options
        .first_pipe_instance(first)
        .reject_remote_clients(true);
    // SAFETY: `attrs` points to a valid SECURITY_ATTRIBUTES whose descriptor stays alive until
    // after the call; the system copies the descriptor when the pipe is created.
    let result = unsafe {
        options.create_with_security_attributes_raw(
            name,
            std::ptr::addr_of_mut!(attrs).cast::<c_void>(),
        )
    };
    // SAFETY: the descriptor was allocated by the conversion call above with LocalAlloc.
    unsafe {
        let _ = LocalFree(Some(HLOCAL(descriptor.0)));
    }
    result
}

async fn serve(pipe: NamedPipeServer, handler: Handler, mut events: broadcast::Receiver<Event>) {
    let (read, mut write) = tokio::io::split(pipe);
    let (out_tx, mut out_rx) = mpsc::channel::<String>(64);
    let writer = tokio::spawn(async move {
        while let Some(line) = out_rx.recv().await {
            if write.write_all(line.as_bytes()).await.is_err()
                || write.write_all(b"\n").await.is_err()
            {
                break;
            }
        }
    });
    let ev_tx = out_tx.clone();
    let forward = tokio::spawn(async move {
        while let Ok(ev) = events.recv().await {
            if let Ok(line) = serde_json::to_string(&ev) {
                if ev_tx.send(line).await.is_err() {
                    break;
                }
            }
        }
    });

    let mut reader = BufReader::new(read);
    let mut line = String::new();
    loop {
        line.clear();
        // Read at most MAX_LINE + 1 bytes so a peer cannot make us buffer without limit.
        let mut limited = (&mut reader).take(MAX_LINE as u64 + 1);
        match limited.read_line(&mut line).await {
            Ok(0) | Err(_) => break,
            Ok(n) if n > MAX_LINE => {
                tracing::warn!("IPC line too long, closing connection");
                break;
            }
            Ok(_) => {}
        }
        let Ok(req) = serde_json::from_str::<Request>(line.trim()) else {
            tracing::debug!("IPC: ignoring malformed line");
            continue;
        };
        let (handler, out) = (Arc::clone(&handler), out_tx.clone());
        tokio::spawn(async move {
            let outcome = handler(req.method, req.params).await;
            let resp = match outcome {
                Ok(v) => Response {
                    id: req.id,
                    result: Some(v),
                    error: None,
                },
                Err(e) => Response {
                    id: req.id,
                    result: None,
                    error: Some(e),
                },
            };
            if let Ok(text) = serde_json::to_string(&resp) {
                let _ = out.send(text).await;
            }
        });
    }
    forward.abort();
    drop(out_tx);
    let _ = writer.await;
}
