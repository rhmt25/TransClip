//! Clipboard synchronisation (PLAN sections 4.3 and 4.4, PROTOCOL.md section 9.2).
//!
//! The platform layer calls [`ClipboardSync::submit_local`] when the OS clipboard changes and
//! implements [`ClipboardSink`] to write received items. The application's event loop passes
//! every [`ManagerEvent`] to [`ClipboardSync::handle_event`].

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use proto::ids::{new_uuid_v4, DeviceId};
use proto::msg::Message;
use proto::sync_rules::{ConflictClock, SeenCache};
use sha2::{Digest, Sha256};
use tokio::sync::{mpsc, oneshot};

use crate::peers::{ManagerEvent, PeerManager, PeerState};
use crate::queue::{OfflineQueue, QueuedItem};

/// Largest text item (PROTOCOL.md section 10).
pub const MAX_TEXT: usize = 8 * 1024 * 1024;
/// Largest image item.
pub const MAX_IMAGE: usize = 50 * 1024 * 1024;
/// Image types that may be sent.
pub const IMAGE_MIMES: [&str; 5] = [
    "image/png",
    "image/jpeg",
    "image/webp",
    "image/gif",
    "image/bmp",
];

/// Clipboard content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Clip {
    /// Plain text.
    Text(String),
    /// Image bytes exactly as they should be pasted.
    Image {
        /// One of [`IMAGE_MIMES`].
        mime: String,
        /// Width in pixels.
        w: u32,
        /// Height in pixels.
        h: u32,
        /// Encoded image.
        bytes: Vec<u8>,
    },
}

impl Clip {
    fn payload(&self) -> &[u8] {
        match self {
            Clip::Text(t) => t.as_bytes(),
            Clip::Image { bytes, .. } => bytes,
        }
    }

    /// Lower-case hex SHA-256 of the payload.
    pub fn hash(&self) -> String {
        sha256_hex(self.payload())
    }
}

fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Identity of an item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemMeta {
    /// Item id (UUID v4).
    pub id: String,
    /// Device that created it.
    pub origin: DeviceId,
    /// Creation time in milliseconds since the epoch (strictly increasing per device).
    pub ts: u64,
    /// SHA-256 of the payload.
    pub hash: String,
}

/// An item received from a peer, ready to be written to the OS clipboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteClip {
    /// Identity data.
    pub meta: ItemMeta,
    /// Content.
    pub clip: Clip,
}

/// Implemented by the platform to write received items to the OS clipboard. The
/// implementation must mark the write as originating from TransClip so it is not captured
/// again. Called on a blocking thread.
pub trait ClipboardSink: Send + Sync + 'static {
    /// Write `clip` to the clipboard.
    fn apply(&self, clip: RemoteClip) -> Result<(), String>;
}

/// Tunables.
#[derive(Debug, Clone, Copy)]
pub struct ClipConfig {
    /// How long to wait for an `ack` before the single retry.
    pub ack_timeout: Duration,
}

impl Default for ClipConfig {
    fn default() -> Self {
        Self {
            ack_timeout: Duration::from_secs(15),
        }
    }
}

/// Reasons `submit_local` refuses an item.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ClipError {
    /// The payload is over the limit.
    #[error("clipboard item too large")]
    TooLarge,
    /// The image type is not supported.
    #[error("unsupported image type")]
    BadMime,
    /// The system could not provide an id.
    #[error("cannot create item id")]
    Id,
}

/// Result of `submit_local`.
#[derive(Debug, PartialEq, Eq)]
pub enum Submitted {
    /// Same content as the current clipboard item: nothing was sent.
    Unchanged,
    /// The item was created and distributed.
    Sent(ItemMeta),
}

/// What happened to an item. Never carries content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipEvent {
    /// The peer acknowledged the item.
    Delivered {
        /// Peer.
        peer: DeviceId,
        /// Item id.
        id: String,
    },
    /// Delivery failed (refused by the peer or no ack after the retry).
    Failed {
        /// Peer.
        peer: DeviceId,
        /// Item id.
        id: String,
        /// Short reason.
        reason: String,
    },
    /// The item is waiting for the peer to come online.
    Queued {
        /// Peer.
        peer: DeviceId,
        /// Item id.
        id: String,
    },
    /// A queued item was dropped because the queue was full.
    Dropped {
        /// Peer.
        peer: DeviceId,
        /// Item id.
        id: String,
    },
    /// An item from a peer was written to the clipboard.
    Applied {
        /// Sender.
        from: DeviceId,
        /// Item id.
        id: String,
    },
}

type AckResult = (bool, Option<String>);

struct State {
    seen: SeenCache,
    clock: ConflictClock,
    last_hash: Option<String>,
    last_ts: u64,
    queues: HashMap<DeviceId, OfflineQueue>,
    pending: HashMap<(DeviceId, String), oneshot::Sender<AckResult>>,
    rate: crate::ratelimit::PeerRate,
}

struct Inner {
    mgr: Arc<PeerManager>,
    sink: Arc<dyn ClipboardSink>,
    local: DeviceId,
    cfg: ClipConfig,
    state: Mutex<State>,
    events: mpsc::Sender<ClipEvent>,
}

impl Inner {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

fn wire(meta: &ItemMeta, clip: &Clip) -> (Message, Vec<u8>) {
    let (id, origin, ts, hash) = (
        meta.id.clone(),
        meta.origin.to_hex(),
        meta.ts,
        meta.hash.clone(),
    );
    match clip {
        Clip::Text(t) => (
            Message::Text {
                id,
                origin,
                ts,
                hash,
            },
            t.as_bytes().to_vec(),
        ),
        Clip::Image { mime, w, h, bytes } => (
            Message::Image {
                id,
                origin,
                ts,
                hash,
                mime: mime.clone(),
                w: *w,
                h: *h,
            },
            bytes.clone(),
        ),
    }
}

/// The clipboard sync component. Cheap to clone.
#[derive(Clone)]
pub struct ClipboardSync {
    inner: Arc<Inner>,
}

impl ClipboardSync {
    /// Create the component. Delivery and apply notifications arrive on the returned channel.
    pub fn new(
        mgr: Arc<PeerManager>,
        sink: Arc<dyn ClipboardSink>,
        local: DeviceId,
        cfg: ClipConfig,
    ) -> (Self, mpsc::Receiver<ClipEvent>) {
        let (events, rx) = mpsc::channel(256);
        let inner = Inner {
            mgr,
            sink,
            local,
            cfg,
            state: Mutex::new(State {
                seen: SeenCache::with_defaults(),
                clock: ConflictClock::new(),
                last_hash: None,
                last_ts: 0,
                queues: HashMap::new(),
                pending: HashMap::new(),
                rate: crate::ratelimit::PeerRate::default(),
            }),
            events,
        };
        (
            Self {
                inner: Arc::new(inner),
            },
            rx,
        )
    }

    /// The OS clipboard changed locally. Sends to online peers and queues for offline ones.
    pub async fn submit_local(&self, clip: Clip) -> Result<Submitted, ClipError> {
        match &clip {
            Clip::Text(t) if t.len() > MAX_TEXT => return Err(ClipError::TooLarge),
            Clip::Image { mime, bytes, .. } => {
                if bytes.len() > MAX_IMAGE {
                    return Err(ClipError::TooLarge);
                }
                if !IMAGE_MIMES.contains(&mime.as_str()) {
                    return Err(ClipError::BadMime);
                }
            }
            Clip::Text(_) => {}
        }
        let hash = clip.hash();
        let id = new_uuid_v4().map_err(|_| ClipError::Id)?;
        let meta = {
            let mut st = self.inner.state();
            if st.last_hash.as_deref() == Some(hash.as_str()) {
                return Ok(Submitted::Unchanged);
            }
            let ts = now_ms().max(st.last_ts + 1);
            st.last_ts = ts;
            st.last_hash = Some(hash.clone());
            // Our own item counts as applied, and its id must never be applied if it echoes back.
            st.seen.insert(&id, ts);
            st.clock.should_apply(ts, self.inner.local);
            ItemMeta {
                id,
                origin: self.inner.local,
                ts,
                hash,
            }
        };
        let item = QueuedItem {
            meta: meta.clone(),
            clip: Arc::new(clip),
        };
        let peers: Vec<_> = self
            .inner
            .mgr
            .status()
            .borrow()
            .iter()
            .map(|p| (p.id, p.state))
            .collect();
        for (peer, state) in peers {
            if state == PeerState::Online {
                spawn_send(&self.inner, peer, item.clone());
            } else {
                enqueue(&self.inner, peer, item.clone()).await;
            }
        }
        Ok(Submitted::Sent(meta))
    }

    /// Process a manager event. Clipboard frames are consumed (`None`); everything else is
    /// returned for the application. A peer coming online flushes its offline queue.
    pub async fn handle_event(&self, event: ManagerEvent) -> Option<ManagerEvent> {
        match event {
            ManagerEvent::Frame { from, frame } => match frame.message {
                Message::Ack { id, ok, reason } => {
                    if let Some(tx) = self.inner.state().pending.remove(&(from, id)) {
                        let _ = tx.send((ok, reason));
                    }
                    None
                }
                m @ (Message::Text { .. } | Message::Image { .. }) => {
                    self.on_item(from, m, frame.payload).await;
                    None
                }
                message => Some(ManagerEvent::Frame {
                    from,
                    frame: proto::frame::Frame {
                        message,
                        payload: frame.payload,
                    },
                }),
            },
            ManagerEvent::Online(info) => {
                let queued = self
                    .inner
                    .state()
                    .queues
                    .get_mut(&info.id)
                    .map(OfflineQueue::drain)
                    .unwrap_or_default();
                for item in queued {
                    spawn_send(&self.inner, info.id, item);
                }
                Some(ManagerEvent::Online(info))
            }
            other => Some(other),
        }
    }

    async fn on_item(&self, from: DeviceId, message: Message, payload: Vec<u8>) {
        let inner = &self.inner;
        let (id, parsed) = parse_item(message, payload);
        let reply = |ok: bool, reason: Option<&str>| {
            let m = Message::Ack {
                id: id.clone(),
                ok,
                reason: reason.map(str::to_string),
            };
            let mgr = Arc::clone(&inner.mgr);
            async move {
                let _ = mgr.send(&from, m, Vec::new()).await;
            }
        };
        let remote = match parsed {
            Ok(r) => r,
            Err(reason) => return reply(false, Some(reason)).await,
        };
        if !inner.state().rate.allow(from, std::time::Instant::now()) {
            return reply(false, Some("busy")).await;
        }
        let apply = {
            let mut st = inner.state();
            let fresh = st.seen.insert(&remote.meta.id, now_ms());
            remote.meta.origin != inner.local
                && fresh
                && st.last_hash.as_deref() != Some(remote.meta.hash.as_str())
                && st.clock.should_apply(remote.meta.ts, remote.meta.origin)
        };
        if !apply {
            return reply(true, None).await;
        }
        let (hash, item_id) = (remote.meta.hash.clone(), remote.meta.id.clone());
        let sink = Arc::clone(&inner.sink);
        let result = tokio::task::spawn_blocking(move || sink.apply(remote)).await;
        match result {
            Ok(Ok(())) => {
                inner.state().last_hash = Some(hash);
                let _ = inner
                    .events
                    .send(ClipEvent::Applied { from, id: item_id })
                    .await;
                reply(true, None).await;
            }
            _ => reply(false, Some("apply")).await,
        }
    }
}

/// Validate an incoming item. Returns its id (for the ack) and the parsed clip.
fn parse_item(message: Message, payload: Vec<u8>) -> (String, Result<RemoteClip, &'static str>) {
    let (id, origin, ts, hash, image) = match message {
        Message::Text {
            id,
            origin,
            ts,
            hash,
        } => (id, origin, ts, hash, None),
        Message::Image {
            id,
            origin,
            ts,
            hash,
            mime,
            w,
            h,
        } => (id, origin, ts, hash, Some((mime, w, h))),
        _ => return (String::new(), Err("type")),
    };
    let parsed = (|| {
        let origin = DeviceId::from_hex(&origin).map_err(|_| "origin")?;
        let limit = if image.is_some() { MAX_IMAGE } else { MAX_TEXT };
        if payload.len() > limit {
            return Err("limit");
        }
        if sha256_hex(&payload) != hash {
            return Err("hash");
        }
        let clip = match image {
            None => Clip::Text(String::from_utf8(payload).map_err(|_| "utf8")?),
            Some((mime, w, h)) => {
                if !IMAGE_MIMES.contains(&mime.as_str()) {
                    return Err("mime");
                }
                Clip::Image {
                    mime,
                    w,
                    h,
                    bytes: payload,
                }
            }
        };
        Ok(RemoteClip {
            meta: ItemMeta {
                id: id.clone(),
                origin,
                ts,
                hash,
            },
            clip,
        })
    })();
    (id, parsed)
}

fn spawn_send(inner: &Arc<Inner>, peer: DeviceId, item: QueuedItem) {
    tokio::spawn(send_with_ack(Arc::clone(inner), peer, item));
}

async fn enqueue(inner: &Arc<Inner>, peer: DeviceId, item: QueuedItem) {
    let id = item.meta.id.clone();
    let evicted = inner.state().queues.entry(peer).or_default().push(item);
    let _ = inner.events.send(ClipEvent::Queued { peer, id }).await;
    if let Some(old) = evicted {
        let _ = inner
            .events
            .send(ClipEvent::Dropped {
                peer,
                id: old.meta.id,
            })
            .await;
    }
}

/// Send once, wait for the ack, retry once, then report failure (PROTOCOL.md section 10).
async fn send_with_ack(inner: Arc<Inner>, peer: DeviceId, item: QueuedItem) {
    let id = item.meta.id.clone();
    for _ in 0..2 {
        let (tx, rx) = oneshot::channel();
        inner.state().pending.insert((peer, id.clone()), tx);
        let (message, payload) = wire(&item.meta, &item.clip);
        if inner.mgr.send(&peer, message, payload).await.is_err() {
            inner.state().pending.remove(&(peer, id.clone()));
            enqueue(&inner, peer, item).await;
            return;
        }
        match tokio::time::timeout(inner.cfg.ack_timeout, rx).await {
            Ok(Ok((true, _))) => {
                let _ = inner.events.send(ClipEvent::Delivered { peer, id }).await;
                return;
            }
            Ok(Ok((false, reason))) => {
                let reason = reason.unwrap_or_else(|| "refused".into());
                let _ = inner
                    .events
                    .send(ClipEvent::Failed { peer, id, reason })
                    .await;
                return;
            }
            _ => {
                inner.state().pending.remove(&(peer, id.clone()));
            }
        }
    }
    let reason = "timeout".to_string();
    let _ = inner
        .events
        .send(ClipEvent::Failed { peer, id, reason })
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::{PeerSeen, Via};
    use crate::link::LocalInfo;
    use crate::peers::ManagerConfig;
    use proto::beacon::Platform;
    use proto::group::GroupKey;
    use std::net::SocketAddr;

    #[derive(Default)]
    struct MemSink(Mutex<Vec<RemoteClip>>);

    impl ClipboardSink for MemSink {
        fn apply(&self, clip: RemoteClip) -> Result<(), String> {
            self.0.lock().unwrap().push(clip);
            Ok(())
        }
    }

    impl MemSink {
        fn texts(&self) -> Vec<String> {
            self.0
                .lock()
                .unwrap()
                .iter()
                .filter_map(|c| match &c.clip {
                    Clip::Text(t) => Some(t.clone()),
                    Clip::Image { .. } => None,
                })
                .collect()
        }
    }

    struct Node {
        id: DeviceId,
        mgr: Arc<PeerManager>,
        sync: ClipboardSync,
        sink: Arc<MemSink>,
        events: mpsc::Receiver<ClipEvent>,
        seen: mpsc::Sender<PeerSeen>,
        pump: tokio::task::JoinHandle<()>,
    }

    fn mgr_cfg(id: u8, listen: SocketAddr) -> ManagerConfig {
        let local = LocalInfo {
            id: DeviceId([id; 16]),
            name: format!("dev{id}"),
            platform: "windows".into(),
            app: "test".into(),
            caps: vec![],
            role: "full".into(),
        };
        let mut c = ManagerConfig::new(local, GroupKey([7; 32]), listen);
        c.backoff_min = Duration::from_millis(50);
        c.backoff_max = Duration::from_millis(200);
        c.fallback_after = Duration::from_secs(3600);
        c
    }

    async fn node_at(id: u8, listen: SocketAddr, run_sync: bool, ack: Duration) -> Node {
        let (seen_tx, seen_rx) = mpsc::channel(16);
        let (mgr, mut mgr_events) = PeerManager::start(mgr_cfg(id, listen), seen_rx)
            .await
            .unwrap();
        let mgr = Arc::new(mgr);
        let sink = Arc::new(MemSink::default());
        let did = DeviceId([id; 16]);
        let (sync, events) = ClipboardSync::new(
            Arc::clone(&mgr),
            Arc::clone(&sink) as Arc<dyn ClipboardSink>,
            did,
            ClipConfig { ack_timeout: ack },
        );
        let s2 = sync.clone();
        let pump = tokio::spawn(async move {
            while let Some(ev) = mgr_events.recv().await {
                if run_sync {
                    let _ = s2.handle_event(ev).await;
                }
            }
        });
        Node {
            id: did,
            mgr,
            sync,
            sink,
            events,
            seen: seen_tx,
            pump,
        }
    }

    async fn node(id: u8) -> Node {
        node_at(
            id,
            SocketAddr::from(([127, 0, 0, 1], 0)),
            true,
            Duration::from_secs(5),
        )
        .await
    }

    async fn introduce(a: &Node, b: &Node) {
        a.seen
            .send(PeerSeen {
                device_id: b.id,
                addr: b.mgr.local_addr(),
                platform: Platform::Windows,
                name: "b".into(),
                via: Via::Beacon,
            })
            .await
            .unwrap();
    }

    async fn wait_online(n: &Node, count: usize) {
        for _ in 0..100 {
            if n.mgr.online_peers().len() >= count {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("peers did not come online");
    }

    async fn next_clip_event(n: &mut Node) -> ClipEvent {
        tokio::time::timeout(Duration::from_secs(5), n.events.recv())
            .await
            .unwrap()
            .unwrap()
    }

    #[tokio::test]
    async fn text_reaches_both_peers_exactly_once_without_echo_or_relay() {
        let mut a = node(1).await;
        let b = node(2).await;
        let c = node(3).await;
        // Full mesh: the smaller id dials.
        introduce(&a, &b).await;
        introduce(&a, &c).await;
        introduce(&b, &c).await;
        wait_online(&a, 2).await;
        wait_online(&b, 2).await;
        wait_online(&c, 2).await;

        let sent = a.sync.submit_local(Clip::Text("hello".into())).await;
        assert!(matches!(sent, Ok(Submitted::Sent(_))));
        let mut delivered = 0;
        while delivered < 2 {
            if let ClipEvent::Delivered { .. } = next_clip_event(&mut a).await {
                delivered += 1;
            }
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(b.sink.texts(), ["hello"]);
        assert_eq!(c.sink.texts(), ["hello"]);
        assert!(a.sink.texts().is_empty(), "no echo to the sender");

        // Same content again is a no-op.
        assert_eq!(
            a.sync.submit_local(Clip::Text("hello".into())).await,
            Ok(Submitted::Unchanged)
        );
        // A reply from B reaches A and C, and A does not bounce it back.
        b.sync
            .submit_local(Clip::Text("reply".into()))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(a.sink.texts(), ["reply"]);
        assert_eq!(c.sink.texts(), ["hello", "reply"]);
        assert_eq!(b.sink.texts(), ["hello"]);
    }

    #[tokio::test]
    async fn offline_peer_receives_the_item_after_it_comes_online() {
        let mut a = node(1).await;
        let b = node(2).await;
        let b_addr = b.mgr.local_addr();
        introduce(&a, &b).await;
        wait_online(&a, 1).await;
        b.pump.abort();
        b.mgr.stop().await;
        drop(b);
        for _ in 0..100 {
            if a.mgr.online_peers().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        a.sync
            .submit_local(Clip::Text("while you were away".into()))
            .await
            .unwrap();
        assert!(matches!(
            next_clip_event(&mut a).await,
            ClipEvent::Queued { .. }
        ));

        let mut b2 = None;
        for _ in 0..50 {
            // The port may need a moment to be released.
            let try_bind = tokio::net::TcpListener::bind(b_addr).await.is_ok();
            if try_bind {
                b2 = Some(node_at(2, b_addr, true, Duration::from_secs(5)).await);
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let b2 = b2.unwrap();
        loop {
            if let ClipEvent::Delivered { .. } = next_clip_event(&mut a).await {
                break;
            }
        }
        assert_eq!(b2.sink.texts(), ["while you were away"]);
    }

    #[tokio::test]
    async fn oversized_and_invalid_items_are_refused() {
        let a = node(1).await;
        let big = "x".repeat(MAX_TEXT + 1);
        assert_eq!(
            a.sync.submit_local(Clip::Text(big)).await,
            Err(ClipError::TooLarge)
        );
        let bad = Clip::Image {
            mime: "image/tiff".into(),
            w: 1,
            h: 1,
            bytes: vec![1],
        };
        assert_eq!(a.sync.submit_local(bad).await, Err(ClipError::BadMime));
        let max = "x".repeat(MAX_TEXT);
        assert!(a.sync.submit_local(Clip::Text(max)).await.is_ok());
    }

    #[tokio::test]
    async fn missing_ack_is_retried_once_then_reported() {
        let mut a = node_at(
            1,
            SocketAddr::from(([127, 0, 0, 1], 0)),
            true,
            Duration::from_millis(150),
        )
        .await;
        // B never runs ClipboardSync, so it never acknowledges.
        let b = node_at(
            2,
            SocketAddr::from(([127, 0, 0, 1], 0)),
            false,
            Duration::from_secs(5),
        )
        .await;
        introduce(&a, &b).await;
        wait_online(&a, 1).await;
        a.sync.submit_local(Clip::Text("hi".into())).await.unwrap();
        match next_clip_event(&mut a).await {
            ClipEvent::Failed { reason, .. } => assert_eq!(reason, "timeout"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn image_round_trip_keeps_bytes_exact() {
        let mut a = node(1).await;
        let b = node(2).await;
        introduce(&a, &b).await;
        wait_online(&a, 1).await;
        wait_online(&b, 1).await;
        let bytes: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        a.sync
            .submit_local(Clip::Image {
                mime: "image/png".into(),
                w: 10,
                h: 10,
                bytes: bytes.clone(),
            })
            .await
            .unwrap();
        loop {
            if let ClipEvent::Delivered { .. } = next_clip_event(&mut a).await {
                break;
            }
        }
        let got = b.sink.0.lock().unwrap().clone();
        assert_eq!(got.len(), 1);
        assert_eq!(
            got[0].clip,
            Clip::Image {
                mime: "image/png".into(),
                w: 10,
                h: 10,
                bytes
            }
        );
    }

    #[test]
    fn corrupted_payload_is_refused() {
        let meta = Message::Text {
            id: "00000000-0000-4000-8000-000000000000".into(),
            origin: DeviceId([1; 16]).to_hex(),
            ts: 1,
            hash: sha256_hex(b"right"),
        };
        let (_, r) = parse_item(meta, b"wrong".to_vec());
        assert_eq!(r.unwrap_err(), "hash");
    }
}
