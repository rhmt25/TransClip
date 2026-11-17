//! Peer manager: one control link per peer, dial rule, backoff, duplicate resolution
//! (PROTOCOL.md section 5).
//!
//! A single actor task owns all peer state. Listener, dialer and link-pump tasks talk to it
//! through a channel, so there are no locks around peer state.

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use proto::frame::Frame;
use proto::group::GroupKey;
use proto::handshake::{initiate, respond, SecureConn};
use proto::ids::DeviceId;
use proto::msg::Message;
use proto::ProtoError;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, watch, Semaphore};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::discovery::PeerSeen;
use crate::link::{
    spawn_link, CloseReason, LinkConfig, LinkEvent, LinkHandle, LocalInfo, PeerInfo,
};

/// Maximum number of peers in a group (PLAN section 3).
pub const MAX_PEERS: usize = 10;
/// Maximum number of TCP connections that may be in the handshake at the same time.
const MAX_PENDING_HANDSHAKES: usize = 16;

/// Manager settings. [`ManagerConfig::new`] gives the production defaults.
#[derive(Clone)]
pub struct ManagerConfig {
    /// What we tell peers in `hello`.
    pub local: LocalInfo,
    /// Group key.
    pub gk: GroupKey,
    /// TCP listen address (port 47820 in production).
    pub listen: SocketAddr,
    /// Link keep-alive timings.
    pub link: LinkConfig,
    /// First reconnect delay.
    pub backoff_min: Duration,
    /// Longest reconnect delay.
    pub backoff_max: Duration,
    /// After this long without a link the peer with the larger id dials too.
    pub fallback_after: Duration,
    /// Maximum number of peers.
    pub max_peers: usize,
    /// Timeout for connect plus handshake when dialing.
    pub connect_timeout: Duration,
}

impl ManagerConfig {
    /// Defaults from PROTOCOL.md: backoff 2 s to 30 s, fallback dial after 30 s, 10 peers.
    pub fn new(local: LocalInfo, gk: GroupKey, listen: SocketAddr) -> Self {
        Self {
            local,
            gk,
            listen,
            link: LinkConfig::default(),
            backoff_min: Duration::from_secs(2),
            backoff_max: Duration::from_secs(30),
            fallback_after: Duration::from_secs(30),
            max_peers: MAX_PEERS,
            connect_timeout: Duration::from_secs(15),
        }
    }
}

/// Connection state of a peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerState {
    /// No link.
    Offline,
    /// A dial is in progress.
    Connecting,
    /// A link is up.
    Online,
}

/// Public view of one peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerStatus {
    /// Peer id.
    pub id: DeviceId,
    /// Last known display name.
    pub name: String,
    /// Connection state.
    pub state: PeerState,
    /// Last known address.
    pub addr: Option<SocketAddr>,
}

/// Events for the application.
#[derive(Debug)]
pub enum ManagerEvent {
    /// A peer became reachable.
    Online(PeerInfo),
    /// A peer's link ended.
    Offline {
        /// Peer id.
        id: DeviceId,
        /// Why.
        reason: CloseReason,
    },
    /// A message from an online peer.
    Frame {
        /// Sender.
        from: DeviceId,
        /// The frame.
        frame: Frame,
    },
    /// A peer was refused because the group is full.
    Rejected {
        /// Refused peer.
        id: DeviceId,
        /// Its display name when known.
        name: String,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dir {
    Inbound,
    Outbound,
}

enum Internal {
    Ready {
        link: u64,
        dir: Dir,
        info: PeerInfo,
        handle: LinkHandle,
        accept: oneshot::Sender<bool>,
    },
    Closed {
        link: u64,
        reason: CloseReason,
    },
    DialFailed(DeviceId),
    Shutdown,
}

type Handles = Arc<Mutex<HashMap<DeviceId, LinkHandle>>>;

struct Active {
    link: u64,
    dir: Dir,
    handle: LinkHandle,
}

struct Peer {
    name: String,
    addr: Option<SocketAddr>,
    active: Option<Active>,
    dialing: bool,
    backoff: Duration,
    next_try: Instant,
    offline_since: Instant,
}

impl Peer {
    fn state(&self) -> PeerState {
        if self.active.is_some() {
            PeerState::Online
        } else if self.dialing {
            PeerState::Connecting
        } else {
            PeerState::Offline
        }
    }
}

/// Handle to the running manager.
pub struct PeerManager {
    cmd: mpsc::Sender<Internal>,
    handles: Handles,
    status: watch::Receiver<Vec<PeerStatus>>,
    local_addr: SocketAddr,
    actor: Option<JoinHandle<()>>,
}

/// Next reconnect delay after a failed attempt.
pub(crate) fn next_backoff(current: Duration, max: Duration) -> Duration {
    current.saturating_mul(2).min(max)
}

/// The connection initiated by the smaller device id wins a duplicate.
fn initiator(dir: Dir, local: DeviceId, peer: DeviceId) -> DeviceId {
    match dir {
        Dir::Outbound => local,
        Dir::Inbound => peer,
    }
}

impl PeerManager {
    /// Bind the listener and start the actor. `seen` delivers discovery results.
    pub async fn start(
        cfg: ManagerConfig,
        seen: mpsc::Receiver<PeerSeen>,
    ) -> std::io::Result<(Self, mpsc::Receiver<ManagerEvent>)> {
        let listener = TcpListener::bind(cfg.listen).await?;
        let local_addr = listener.local_addr()?;
        let (cmd_tx, cmd_rx) = mpsc::channel(256);
        let (event_tx, event_rx) = mpsc::channel(256);
        let (status_tx, status_rx) = watch::channel(Vec::new());
        let handles: Handles = Arc::default();
        let link_ids = Arc::new(AtomicU64::new(1));

        let listener_task = tokio::spawn(accept_loop(
            listener,
            cfg.clone(),
            Arc::clone(&link_ids),
            cmd_tx.clone(),
            event_tx.clone(),
        ));
        let actor = Actor {
            cfg,
            peers: HashMap::new(),
            rejected: HashSet::new(),
            handles: Arc::clone(&handles),
            link_ids,
            cmd_tx: cmd_tx.clone(),
            events: event_tx,
            status: status_tx,
        };
        let actor = tokio::spawn(actor.run(seen, cmd_rx, listener_task));
        Ok((
            Self {
                cmd: cmd_tx,
                handles,
                status: status_rx,
                local_addr,
                actor: Some(actor),
            },
            event_rx,
        ))
    }

    /// Address the TCP listener is bound to.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Public peer status; updated on every change.
    pub fn status(&self) -> watch::Receiver<Vec<PeerStatus>> {
        self.status.clone()
    }

    /// Ids of peers that currently have a link.
    pub fn online_peers(&self) -> Vec<DeviceId> {
        self.handles
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .keys()
            .copied()
            .collect()
    }

    /// Send handle of an online peer.
    pub fn handle(&self, id: &DeviceId) -> Option<LinkHandle> {
        self.handles
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(id)
            .cloned()
    }

    /// Send a control-priority message to one peer.
    pub async fn send(
        &self,
        to: &DeviceId,
        message: Message,
        payload: Vec<u8>,
    ) -> Result<(), ProtoError> {
        match self.handle(to) {
            Some(h) => h.send_control(message, payload).await,
            None => Err(ProtoError::Closed),
        }
    }

    /// Stop everything: say `bye` to all peers and close the listener.
    pub async fn shutdown(mut self) {
        let _ = self.cmd.send(Internal::Shutdown).await;
        if let Some(actor) = self.actor.take() {
            let _ = actor.await;
        }
    }
}

impl Drop for PeerManager {
    fn drop(&mut self) {
        let _ = self.cmd.try_send(Internal::Shutdown);
    }
}

async fn accept_loop(
    listener: TcpListener,
    cfg: ManagerConfig,
    link_ids: Arc<AtomicU64>,
    cmd: mpsc::Sender<Internal>,
    events: mpsc::Sender<ManagerEvent>,
) {
    let slots = Arc::new(Semaphore::new(MAX_PENDING_HANDSHAKES));
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            tokio::time::sleep(Duration::from_millis(50)).await;
            continue;
        };
        let Ok(permit) = Arc::clone(&slots).try_acquire_owned() else {
            continue; // too many half-open connections: drop this one
        };
        let (cfg, cmd, events) = (cfg.clone(), cmd.clone(), events.clone());
        let link = link_ids.fetch_add(1, Ordering::Relaxed);
        tokio::spawn(async move {
            let _ = stream.set_nodelay(true);
            let conn = respond(stream, &cfg.gk).await;
            drop(permit);
            if let Ok(conn) = conn {
                let (handle, rx) = spawn_link(conn, cfg.local.clone(), cfg.link);
                pump(link, Dir::Inbound, None, handle, rx, cmd, events).await;
            }
        });
    }
}

/// Forward one link's events. Returns `false` when the link never became ready or was
/// not accepted.
async fn pump(
    link: u64,
    dir: Dir,
    expect: Option<DeviceId>,
    handle: LinkHandle,
    mut rx: mpsc::Receiver<LinkEvent>,
    cmd: mpsc::Sender<Internal>,
    events: mpsc::Sender<ManagerEvent>,
) -> bool {
    let info = loop {
        match rx.recv().await {
            Some(LinkEvent::Ready(info)) => break info,
            Some(LinkEvent::Frame(_)) => {}
            Some(LinkEvent::Closed(_)) | None => return false,
        }
    };
    if expect.is_some_and(|e| e != info.id) {
        handle.close("unexpected-peer").await;
        return false;
    }
    let from = info.id;
    let (accept_tx, accept_rx) = oneshot::channel();
    let ready = Internal::Ready {
        link,
        dir,
        info,
        handle: handle.clone(),
        accept: accept_tx,
    };
    if cmd.send(ready).await.is_err() || !accept_rx.await.unwrap_or(false) {
        let _ = handle
            .send_control(
                Message::Error {
                    code: "duplicate".into(),
                    msg: None,
                    reference: None,
                },
                Vec::new(),
            )
            .await;
        handle.close("duplicate").await;
        return false;
    }
    while let Some(ev) = rx.recv().await {
        match ev {
            LinkEvent::Frame(frame) => {
                if events
                    .send(ManagerEvent::Frame { from, frame })
                    .await
                    .is_err()
                {
                    handle.close("shutdown").await;
                }
            }
            LinkEvent::Closed(reason) => {
                let _ = cmd.send(Internal::Closed { link, reason }).await;
                return true;
            }
            LinkEvent::Ready(_) => {}
        }
    }
    true
}

struct Actor {
    cfg: ManagerConfig,
    peers: HashMap<DeviceId, Peer>,
    rejected: HashSet<DeviceId>,
    handles: Handles,
    link_ids: Arc<AtomicU64>,
    cmd_tx: mpsc::Sender<Internal>,
    events: mpsc::Sender<ManagerEvent>,
    status: watch::Sender<Vec<PeerStatus>>,
}

impl Actor {
    async fn run(
        mut self,
        mut seen: mpsc::Receiver<PeerSeen>,
        mut cmd: mpsc::Receiver<Internal>,
        listener: JoinHandle<()>,
    ) {
        let mut tick = tokio::time::interval(Duration::from_millis(100));
        loop {
            tokio::select! {
                Some(s) = seen.recv() => self.on_seen(s).await,
                Some(c) = cmd.recv() => match c {
                    Internal::Shutdown => break,
                    other => self.on_internal(other).await,
                },
                _ = tick.tick() => {
                    let ids: Vec<DeviceId> = self.peers.keys().copied().collect();
                    for id in ids {
                        self.maybe_dial(id);
                    }
                }
            }
        }
        listener.abort();
        let _ = listener.await;
        for peer in self.peers.values() {
            if let Some(a) = &peer.active {
                a.handle.close("shutdown").await;
            }
        }
        self.handles
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }

    fn publish(&self) {
        let mut list: Vec<PeerStatus> = self
            .peers
            .iter()
            .map(|(id, p)| PeerStatus {
                id: *id,
                name: p.name.clone(),
                state: p.state(),
                addr: p.addr,
            })
            .collect();
        list.sort_by_key(|s| s.id);
        self.status.send_replace(list);
    }

    fn new_peer(&self, name: String, addr: Option<SocketAddr>) -> Peer {
        let now = Instant::now();
        Peer {
            name,
            addr,
            active: None,
            dialing: false,
            backoff: self.cfg.backoff_min,
            next_try: now,
            offline_since: now,
        }
    }

    /// Make room for `id`; returns false (and reports once) when the group is full.
    async fn admit(&mut self, id: DeviceId, name: &str) -> bool {
        if self.peers.contains_key(&id) {
            return true;
        }
        if self.peers.len() >= self.cfg.max_peers {
            if self.rejected.insert(id) {
                tracing::warn!(peer = %short(id), "peer limit reached, refusing peer");
                let _ = self
                    .events
                    .send(ManagerEvent::Rejected {
                        id,
                        name: name.to_string(),
                    })
                    .await;
            }
            return false;
        }
        true
    }

    async fn on_seen(&mut self, s: PeerSeen) {
        if s.device_id == self.cfg.local.id || !self.admit(s.device_id, &s.name).await {
            return;
        }
        let fresh = self.new_peer(s.name.clone(), Some(s.addr));
        let peer = self.peers.entry(s.device_id).or_insert(fresh);
        peer.name = s.name;
        if peer.addr != Some(s.addr) {
            peer.addr = Some(s.addr);
            peer.next_try = Instant::now();
        }
        self.publish();
        self.maybe_dial(s.device_id);
    }

    fn maybe_dial(&mut self, id: DeviceId) {
        let now = Instant::now();
        let local = self.cfg.local.id;
        let fallback = self.cfg.fallback_after;
        let Some(peer) = self.peers.get_mut(&id) else {
            return;
        };
        let Some(addr) = peer.addr else { return };
        let allowed = local < id || now.duration_since(peer.offline_since) >= fallback;
        if peer.active.is_some() || peer.dialing || now < peer.next_try || !allowed {
            return;
        }
        peer.dialing = true;
        let link = self.link_ids.fetch_add(1, Ordering::Relaxed);
        let (cfg, cmd, events) = (self.cfg.clone(), self.cmd_tx.clone(), self.events.clone());
        tokio::spawn(async move {
            let attempt = async {
                let stream = TcpStream::connect(addr).await.ok()?;
                stream.set_nodelay(true).ok()?;
                initiate(stream, &cfg.gk).await.ok()
            };
            let conn = tokio::time::timeout(cfg.connect_timeout, attempt)
                .await
                .ok()
                .flatten();
            let ready = match conn {
                Some(conn) => dial_pump(conn, link, id, &cfg, cmd.clone(), events).await,
                None => false,
            };
            if !ready {
                let _ = cmd.send(Internal::DialFailed(id)).await;
            }
        });
        self.publish();
    }

    async fn on_internal(&mut self, c: Internal) {
        match c {
            Internal::Ready {
                link,
                dir,
                info,
                handle,
                accept,
            } => {
                let ok = self.on_ready(link, dir, info, handle).await;
                let _ = accept.send(ok);
            }
            Internal::Closed { link, reason } => self.on_closed(link, reason).await,
            Internal::DialFailed(id) => {
                let max = self.cfg.backoff_max;
                if let Some(p) = self.peers.get_mut(&id) {
                    p.dialing = false;
                    p.next_try = Instant::now() + p.backoff;
                    p.backoff = next_backoff(p.backoff, max);
                }
                self.publish();
            }
            Internal::Shutdown => {}
        }
    }

    async fn on_ready(&mut self, link: u64, dir: Dir, info: PeerInfo, handle: LinkHandle) -> bool {
        let id = info.id;
        let local = self.cfg.local.id;
        if id == local || !self.admit(id, &info.name).await {
            return false;
        }
        let fresh = self.new_peer(info.name.clone(), None);
        let min = self.cfg.backoff_min;
        let peer = self.peers.entry(id).or_insert(fresh);
        if dir == Dir::Outbound {
            peer.dialing = false;
        }
        let was_online = peer.active.is_some();
        if let Some(old) = &peer.active {
            let old_init = initiator(old.dir, local, id);
            let new_init = initiator(dir, local, id);
            if new_init > old_init {
                tracing::debug!(peer = %short(id), "duplicate link refused");
                return false;
            }
            let old_handle = old.handle.clone();
            tokio::spawn(async move {
                let dup = Message::Error {
                    code: "duplicate".into(),
                    msg: None,
                    reference: None,
                };
                let _ = old_handle.send_control(dup, Vec::new()).await;
                old_handle.close("duplicate").await;
            });
        }
        peer.name = info.name.clone();
        peer.backoff = min;
        peer.active = Some(Active {
            link,
            dir,
            handle: handle.clone(),
        });
        self.handles
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id, handle);
        self.publish();
        if !was_online {
            tracing::info!(peer = %short(id), "peer online");
            let _ = self.events.send(ManagerEvent::Online(info)).await;
        }
        true
    }

    async fn on_closed(&mut self, link: u64, reason: CloseReason) {
        let min = self.cfg.backoff_min;
        let found = self
            .peers
            .iter_mut()
            .find(|(_, p)| p.active.as_ref().is_some_and(|a| a.link == link));
        let Some((&id, peer)) = found else {
            return; // a replaced duplicate: not the current link
        };
        peer.active = None;
        let now = Instant::now();
        peer.offline_since = now;
        peer.next_try = now + min;
        self.handles
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&id);
        self.publish();
        tracing::info!(peer = %short(id), "peer offline");
        let _ = self.events.send(ManagerEvent::Offline { id, reason }).await;
    }
}

async fn dial_pump<S>(
    conn: SecureConn<S>,
    link: u64,
    expect: DeviceId,
    cfg: &ManagerConfig,
    cmd: mpsc::Sender<Internal>,
    events: mpsc::Sender<ManagerEvent>,
) -> bool
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (handle, rx) = spawn_link(conn, cfg.local.clone(), cfg.link);
    pump(link, Dir::Outbound, Some(expect), handle, rx, cmd, events).await
}

fn short(id: DeviceId) -> String {
    id.to_hex().chars().take(8).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proto::beacon::Platform;
    use proto::msg::Message;

    fn local(id: u8) -> LocalInfo {
        LocalInfo {
            id: DeviceId([id; 16]),
            name: format!("dev{id}"),
            platform: "windows".into(),
            app: "test".into(),
            caps: vec![],
            role: "full".into(),
        }
    }

    fn cfg(id: u8, key: u8) -> ManagerConfig {
        let mut c = ManagerConfig::new(
            local(id),
            GroupKey([key; 32]),
            SocketAddr::from(([127, 0, 0, 1], 0)),
        );
        c.backoff_min = Duration::from_millis(50);
        c.backoff_max = Duration::from_millis(200);
        c.fallback_after = Duration::from_secs(3600);
        c
    }

    fn probe() -> Message {
        Message::Pull {
            id: "00000000-0000-4000-8000-000000000000".into(),
        }
    }

    fn seen(id: u8, addr: SocketAddr) -> PeerSeen {
        PeerSeen {
            device_id: DeviceId([id; 16]),
            addr,
            platform: Platform::Windows,
            name: format!("dev{id}"),
            via: crate::discovery::Via::Beacon,
        }
    }

    struct Node {
        mgr: PeerManager,
        events: mpsc::Receiver<ManagerEvent>,
        seen: mpsc::Sender<PeerSeen>,
    }

    async fn node(c: ManagerConfig) -> Node {
        let (tx, rx) = mpsc::channel(16);
        let (mgr, events) = PeerManager::start(c, rx).await.unwrap();
        Node {
            mgr,
            events,
            seen: tx,
        }
    }

    async fn next_event(n: &mut Node) -> ManagerEvent {
        tokio::time::timeout(Duration::from_secs(5), n.events.recv())
            .await
            .unwrap()
            .unwrap()
    }

    async fn expect_online(n: &mut Node, id: u8) {
        loop {
            if let ManagerEvent::Online(info) = next_event(n).await {
                assert_eq!(info.id, DeviceId([id; 16]));
                return;
            }
        }
    }

    async fn expect_offline(n: &mut Node, id: u8) {
        loop {
            if let ManagerEvent::Offline { id: got, .. } = next_event(n).await {
                assert_eq!(got, DeviceId([id; 16]));
                return;
            }
        }
    }

    async fn expect_probe_frame(n: &mut Node) {
        loop {
            if let ManagerEvent::Frame { frame, .. } = next_event(n).await {
                if matches!(frame.message, Message::Pull { .. }) {
                    return;
                }
            }
        }
    }

    #[test]
    fn backoff_doubles_to_the_cap() {
        let max = Duration::from_secs(30);
        let mut d = Duration::from_secs(2);
        let mut seq = vec![];
        for _ in 0..6 {
            seq.push(d.as_secs());
            d = next_backoff(d, max);
        }
        assert_eq!(seq, [2, 4, 8, 16, 30, 30]);
    }

    #[tokio::test]
    async fn two_engines_link_exactly_once_and_exchange_messages() {
        let mut a = node(cfg(1, 7)).await;
        let mut b = node(cfg(2, 7)).await;
        a.seen.send(seen(2, b.mgr.local_addr())).await.unwrap();
        b.seen.send(seen(1, a.mgr.local_addr())).await.unwrap();
        expect_online(&mut a, 2).await;
        expect_online(&mut b, 1).await;
        // The larger id must not have dialed: B sees an inbound link only.
        a.mgr
            .send(&DeviceId([2; 16]), probe(), vec![])
            .await
            .unwrap();
        expect_probe_frame(&mut b).await;
        b.mgr
            .send(&DeviceId([1; 16]), probe(), vec![])
            .await
            .unwrap();
        expect_probe_frame(&mut a).await;
        assert_eq!(a.mgr.online_peers(), vec![DeviceId([2; 16])]);
        assert_eq!(b.mgr.online_peers(), vec![DeviceId([1; 16])]);
        let st = a.mgr.status().borrow().clone();
        assert_eq!(st.len(), 1);
        assert_eq!(st[0].state, PeerState::Online);
    }

    #[tokio::test]
    async fn peer_goes_offline_and_reconnects() {
        let mut a = node(cfg(1, 7)).await;
        let b = node(cfg(2, 7)).await;
        let b_addr = b.mgr.local_addr();
        a.seen.send(seen(2, b_addr)).await.unwrap();
        expect_online(&mut a, 2).await;
        b.mgr.shutdown().await;
        expect_offline(&mut a, 2).await;
        assert_eq!(a.mgr.status().borrow()[0].state, PeerState::Offline);

        // Bring B back on the same port; A keeps retrying with backoff.
        let mut c = cfg(2, 7);
        c.listen = b_addr;
        let mut b2 = None;
        for _ in 0..50 {
            let (tx, rx) = mpsc::channel(4);
            if let Ok((mgr, events)) = PeerManager::start(c.clone(), rx).await {
                b2 = Some(Node {
                    mgr,
                    events,
                    seen: tx,
                });
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let mut b2 = b2.unwrap();
        expect_online(&mut a, 2).await;
        expect_online(&mut b2, 1).await;
    }

    #[tokio::test]
    async fn simultaneous_dials_leave_a_single_link() {
        let mut ca = cfg(1, 7);
        let mut cb = cfg(2, 7);
        ca.fallback_after = Duration::ZERO; // both sides dial at once
        cb.fallback_after = Duration::ZERO;
        let mut a = node(ca).await;
        let mut b = node(cb).await;
        a.seen.send(seen(2, b.mgr.local_addr())).await.unwrap();
        b.seen.send(seen(1, a.mgr.local_addr())).await.unwrap();
        expect_online(&mut a, 2).await;
        expect_online(&mut b, 1).await;
        // Let any duplicate be resolved, then check the survivor works both ways.
        tokio::time::sleep(Duration::from_millis(500)).await;
        a.mgr
            .send(&DeviceId([2; 16]), probe(), vec![])
            .await
            .unwrap();
        expect_probe_frame(&mut b).await;
        b.mgr
            .send(&DeviceId([1; 16]), probe(), vec![])
            .await
            .unwrap();
        expect_probe_frame(&mut a).await;
        for n in [&a, &b] {
            let st = n.mgr.status().borrow().clone();
            assert_eq!(st.len(), 1);
            assert_eq!(st[0].state, PeerState::Online);
        }
        // No Offline flapping while the duplicate was settled.
        assert!(!matches!(
            a.events.try_recv(),
            Ok(ManagerEvent::Offline { .. })
        ));
        assert!(!matches!(
            b.events.try_recv(),
            Ok(ManagerEvent::Offline { .. })
        ));
    }

    #[tokio::test]
    async fn eleventh_peer_is_rejected() {
        let mut c = cfg(1, 7);
        c.max_peers = 2;
        let mut a = node(c).await;
        let dead = SocketAddr::from(([127, 0, 0, 1], 9));
        for id in 2..=4u8 {
            a.seen.send(seen(id, dead)).await.unwrap();
        }
        loop {
            if let ManagerEvent::Rejected { id, .. } = next_event(&mut a).await {
                assert_eq!(id, DeviceId([4; 16]));
                break;
            }
        }
        assert_eq!(a.mgr.status().borrow().len(), 2);
    }

    #[tokio::test]
    async fn wrong_group_key_never_links() {
        let mut a = node(cfg(1, 7)).await;
        let b = node(cfg(2, 8)).await;
        a.seen.send(seen(2, b.mgr.local_addr())).await.unwrap();
        let r = tokio::time::timeout(Duration::from_millis(700), a.events.recv()).await;
        assert!(r.is_err(), "no event expected, got {r:?}");
        assert!(a.mgr.online_peers().is_empty());
    }

    #[tokio::test]
    async fn larger_id_waits_for_fallback_before_dialing() {
        let mut b = node(cfg(2, 7)).await; // larger id, fallback far away
        let a = node(cfg(1, 7)).await;
        b.seen.send(seen(1, a.mgr.local_addr())).await.unwrap();
        let r = tokio::time::timeout(Duration::from_millis(500), b.events.recv()).await;
        assert!(r.is_err(), "larger id must not dial before the fallback");
    }
}
