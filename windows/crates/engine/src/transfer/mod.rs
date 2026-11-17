//! File and folder transfer (PROTOCOL.md section 9.3, PLAN section 4.5).
//!
//! The sender offers each entry of a batch, streams the bytes and waits for `file_result`; the
//! receiver validates, writes to a `.transclip-part` file, verifies the SHA-256 and renames.
//! Everything travels on the control link: file data uses its low-priority queue (D-013).

mod incoming;
mod outgoing;
pub mod scan;
pub mod store;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use proto::ids::DeviceId;
use proto::msg::{EntryKind, Message};
use tokio::sync::{mpsc, Semaphore};

use crate::peers::{ManagerEvent, PeerManager};
pub use store::{FsStore, IncomingStore, PartWriter, PART_SUFFIX};

/// Largest payload of one `file_chunk`.
pub const CHUNK: usize = 64 * 1024;
/// Parallel batches per peer.
pub const MAX_PARALLEL: usize = 3;

/// Tunables.
#[derive(Debug, Clone)]
pub struct TransferConfig {
    /// Accept incoming files at all.
    pub receive_files: bool,
    /// Largest accepted file; 0 means no limit.
    pub max_file_bytes: u64,
    /// How long to wait for `file_accept` / `file_result`.
    pub reply_timeout: Duration,
    /// How long to wait for a peer to come back before giving up on an entry.
    pub reconnect_wait: Duration,
    /// How many times one entry is retried after a lost connection.
    pub max_reconnects: u32,
    /// Pause after every chunk (tests use it to make a transfer slow enough to interrupt).
    pub chunk_delay: Duration,
}

impl Default for TransferConfig {
    fn default() -> Self {
        Self {
            receive_files: true,
            max_file_bytes: 0,
            reply_timeout: Duration::from_secs(30),
            reconnect_wait: Duration::from_secs(60),
            max_reconnects: 5,
            chunk_delay: Duration::ZERO,
        }
    }
}

/// One thing to send. `path` is the local file or folder; `rel` is the path the receiver sees.
#[derive(Debug, Clone)]
pub struct EntrySource {
    /// Relative path with `/` separators.
    pub rel: String,
    /// File or directory.
    pub kind: EntryKind,
    /// Local path to read from (unused for directories).
    pub path: PathBuf,
}

/// A group of entries sent together.
#[derive(Debug, Clone)]
pub struct Batch {
    /// Folder name for the receiver, or empty for loose files.
    pub name: String,
    /// The entries, directories before their contents.
    pub entries: Vec<EntrySource>,
}

/// Outcome of a batch for the sender.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchReport {
    /// Batch id.
    pub batch_id: String,
    /// Entries that arrived and verified.
    pub ok: u32,
    /// Entries that failed.
    pub failed: u32,
}

/// Progress and results for the UI. Never logged above `debug` (it names files).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferEvent {
    /// A transfer started or was resumed.
    Started {
        /// Entry id.
        xfer_id: String,
        /// Batch id.
        batch_id: String,
        /// The other device.
        peer: DeviceId,
        /// Relative path of the entry.
        name: String,
        /// Size in bytes.
        bytes: u64,
        /// Offset the transfer starts at (non-zero when resuming).
        offset: u64,
        /// True when we receive.
        incoming: bool,
    },
    /// Bytes transferred so far.
    Progress {
        /// Entry id.
        xfer_id: String,
        /// Bytes done.
        done: u64,
        /// Total bytes.
        total: u64,
    },
    /// One entry ended.
    Finished {
        /// Entry id.
        xfer_id: String,
        /// The other device.
        peer: DeviceId,
        /// Success.
        ok: bool,
        /// Short reason on failure.
        reason: Option<String>,
        /// True when we received it.
        incoming: bool,
    },
    /// A whole batch ended.
    BatchFinished {
        /// Batch id.
        batch_id: String,
        /// The other device.
        peer: DeviceId,
        /// Entries that succeeded.
        ok: u32,
        /// Entries that failed.
        failed: u32,
        /// True when we received it.
        incoming: bool,
    },
}

/// Errors starting a send.
#[derive(Debug, thiserror::Error)]
pub enum TransferError {
    /// The peer is not connected.
    #[error("peer is offline")]
    Offline,
    /// A source entry has an invalid relative path or cannot be read.
    #[error("invalid entry: {0}")]
    Invalid(String),
    /// Local I/O failed.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Message from the event pump to a waiting sender.
#[derive(Debug)]
pub(crate) enum OutMsg {
    Accept(u64),
    Reject(String),
    Result(bool, Option<String>),
    Cancel(String),
    Disconnected,
}

pub(crate) struct State {
    pub incoming: HashMap<String, incoming::Incoming>,
    pub batches: HashMap<String, incoming::BatchState>,
    pub waiters: HashMap<String, (DeviceId, mpsc::UnboundedSender<OutMsg>)>,
    pub limits: HashMap<DeviceId, Arc<Semaphore>>,
    pub cancelled: std::collections::HashSet<String>,
}

pub(crate) struct Inner {
    pub mgr: Arc<PeerManager>,
    pub store: Arc<dyn IncomingStore>,
    pub cfg: TransferConfig,
    pub events: mpsc::Sender<TransferEvent>,
    pub state: Mutex<State>,
}

impl Inner {
    pub fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub async fn emit(&self, ev: TransferEvent) {
        let _ = self.events.send(ev).await;
    }

    /// Progress is best effort: drop it when the consumer is slow.
    pub fn emit_progress(&self, xfer_id: &str, done: u64, total: u64) {
        let _ = self.events.try_send(TransferEvent::Progress {
            xfer_id: xfer_id.to_string(),
            done,
            total,
        });
    }
}

/// The transfer component. Cheap to clone.
#[derive(Clone)]
pub struct TransferManager {
    pub(crate) inner: Arc<Inner>,
}

impl TransferManager {
    /// Create the component; events arrive on the returned channel.
    pub fn new(
        mgr: Arc<PeerManager>,
        store: Arc<dyn IncomingStore>,
        cfg: TransferConfig,
    ) -> (Self, mpsc::Receiver<TransferEvent>) {
        let (events, rx) = mpsc::channel(1024);
        let inner = Inner {
            mgr,
            store,
            cfg,
            events,
            state: Mutex::new(State {
                incoming: HashMap::new(),
                batches: HashMap::new(),
                waiters: HashMap::new(),
                limits: HashMap::new(),
                cancelled: Default::default(),
            }),
        };
        (
            Self {
                inner: Arc::new(inner),
            },
            rx,
        )
    }

    /// Process a manager event. File frames are consumed (`None`); everything else is returned.
    pub async fn handle_event(&self, event: ManagerEvent) -> Option<ManagerEvent> {
        match event {
            ManagerEvent::Frame { from, frame } => match frame.message {
                Message::FileOffer { .. } => {
                    self.on_offer(from, frame.message).await;
                    None
                }
                Message::FileStream { xfer_id, offset } => {
                    self.on_stream(from, &xfer_id, offset).await;
                    None
                }
                Message::FileChunk { xfer_id, offset } => {
                    self.on_chunk(from, &xfer_id, offset, &frame.payload).await;
                    None
                }
                Message::FileEnd { xfer_id } => {
                    self.on_end(from, &xfer_id).await;
                    None
                }
                Message::FileCancel { xfer_id, reason } => {
                    // It may concern a transfer we send (receiver gave up) or one we receive.
                    self.notify(&xfer_id, OutMsg::Cancel(reason.clone()));
                    self.on_cancel(from, &xfer_id, reason).await;
                    None
                }
                Message::FileAccept { xfer_id, offset } => {
                    self.notify(&xfer_id, OutMsg::Accept(offset));
                    None
                }
                Message::FileReject { xfer_id, reason } => {
                    self.notify(&xfer_id, OutMsg::Reject(reason));
                    None
                }
                Message::FileResult {
                    xfer_id,
                    ok,
                    reason,
                } => {
                    self.notify(&xfer_id, OutMsg::Result(ok, reason));
                    None
                }
                Message::BatchDone { .. } => None, // the sender counts results itself
                message => Some(ManagerEvent::Frame {
                    from,
                    frame: proto::frame::Frame {
                        message,
                        payload: frame.payload,
                    },
                }),
            },
            ManagerEvent::Offline { id, reason } => {
                self.on_offline(id);
                Some(ManagerEvent::Offline { id, reason })
            }
            other => Some(other),
        }
    }

    fn notify(&self, xfer_id: &str, msg: OutMsg) {
        let tx = self
            .inner
            .state()
            .waiters
            .get(xfer_id)
            .map(|(_, tx)| tx.clone());
        if let Some(tx) = tx {
            let _ = tx.send(msg);
        }
    }

    fn on_offline(&self, peer: DeviceId) {
        let mut st = self.inner.state();
        // Parts stay on disk so the sender can resume; only the in-memory state goes away.
        st.incoming.retain(|_, i| i.peer != peer);
        for (owner, tx) in st.waiters.values() {
            if *owner == peer {
                let _ = tx.send(OutMsg::Disconnected);
            }
        }
    }

    /// Cancel a batch we are sending: the current entry is cancelled and no more are started.
    pub fn cancel_batch(&self, batch_id: &str) {
        self.inner.state().cancelled.insert(batch_id.to_string());
    }
}

#[cfg(test)]
mod tests;
