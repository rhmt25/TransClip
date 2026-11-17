//! One authenticated connection to one peer (PLAN section 4.1, PROTOCOL.md sections 9.1 and 10).
//!
//! A [`Link`] sends `hello` first, keeps the connection alive with `ping`/`pong`, drops a
//! peer that stays silent, and gives control frames priority over bulk data frames.

use proto::frame::Frame;
use proto::handshake::SecureConn;
use proto::ids::DeviceId;
use proto::msg::{Message, VERSION};
use proto::ProtoError;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;
use tokio::time::{sleep_until, timeout, Instant};

/// Timings of a link. The defaults are the values in PROTOCOL.md section 10.
#[derive(Debug, Clone, Copy)]
pub struct LinkConfig {
    /// Send a `ping` when nothing was sent for this long.
    pub ping_interval: Duration,
    /// Treat the peer as dead when nothing was received for this long.
    pub dead_after: Duration,
}

impl Default for LinkConfig {
    fn default() -> Self {
        Self {
            ping_interval: Duration::from_secs(30),
            dead_after: Duration::from_secs(90),
        }
    }
}

/// What this device tells the peer about itself in `hello`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalInfo {
    /// Our device id.
    pub id: DeviceId,
    /// Human readable name.
    pub name: String,
    /// `windows` or `android`.
    pub platform: String,
    /// Application version.
    pub app: String,
    /// Capabilities, for example `text`, `image`, `file`.
    pub caps: Vec<String>,
    /// `control` or `file`.
    pub role: String,
}

/// What the peer told us about itself in `hello`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerInfo {
    /// Peer device id.
    pub id: DeviceId,
    /// Peer name.
    pub name: String,
    /// Peer platform.
    pub platform: String,
    /// Peer application version.
    pub app: String,
    /// Peer capabilities.
    pub caps: Vec<String>,
    /// Role of this connection.
    pub role: String,
}

/// Why a link ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseReason {
    /// The peer sent `bye`.
    PeerBye(String),
    /// The peer closed the connection.
    PeerClosed,
    /// Nothing was received for `dead_after`.
    Dead,
    /// We closed it.
    Local,
    /// Protocol or transport error.
    Error(String),
}

/// Something that happened on a link.
#[derive(Debug)]
pub enum LinkEvent {
    /// The peer's `hello` arrived; the link is usable.
    Ready(PeerInfo),
    /// Any other message from the peer.
    Frame(Frame),
    /// The link is finished. Always the last event.
    Closed(CloseReason),
}

enum Outgoing {
    Frame(Message, Vec<u8>),
    Close(String),
}

/// Handle used to send on a link. Cheap to clone.
#[derive(Clone)]
pub struct LinkHandle {
    control: mpsc::Sender<Outgoing>,
    data: mpsc::Sender<Outgoing>,
}

impl LinkHandle {
    /// Send a high-priority message (clipboard items, acks, offers, control).
    pub async fn send_control(&self, message: Message, payload: Vec<u8>) -> Result<(), ProtoError> {
        self.control
            .send(Outgoing::Frame(message, payload))
            .await
            .map_err(|_| ProtoError::Closed)
    }

    /// Send a low-priority message (file chunks). Control messages overtake queued data.
    pub async fn send_data(&self, message: Message, payload: Vec<u8>) -> Result<(), ProtoError> {
        self.data
            .send(Outgoing::Frame(message, payload))
            .await
            .map_err(|_| ProtoError::Closed)
    }

    /// Say `bye` and close. Queued control messages are sent first.
    pub async fn close(&self, reason: &str) {
        let _ = self.control.send(Outgoing::Close(reason.to_string())).await;
    }

    /// True once the link has ended.
    pub fn is_closed(&self) -> bool {
        self.control.is_closed()
    }
}

/// Translate a transport error into the reason reported to the user of the link.
fn reason_for(e: &ProtoError) -> CloseReason {
    use std::io::ErrorKind::{BrokenPipe, ConnectionAborted, ConnectionReset, UnexpectedEof};
    match e {
        ProtoError::Closed => CloseReason::PeerClosed,
        ProtoError::Io(io)
            if matches!(
                io.kind(),
                BrokenPipe | ConnectionReset | ConnectionAborted | UnexpectedEof
            ) =>
        {
            CloseReason::PeerClosed
        }
        other => CloseReason::Error(other.to_string()),
    }
}

/// Send `Closed` exactly once per link, whichever task notices the end first.
async fn finish(events: &mpsc::Sender<LinkEvent>, sent: &AtomicBool, reason: CloseReason) {
    if !sent.swap(true, Ordering::SeqCst) {
        let _ = events.send(LinkEvent::Closed(reason)).await;
    }
}

/// Start a link on an established encrypted connection.
///
/// Returns the send handle and the stream of events. The first event is `Ready` (or
/// `Closed` when the peer never sends a valid `hello`).
pub fn spawn_link<S>(
    conn: SecureConn<S>,
    local: LocalInfo,
    cfg: LinkConfig,
) -> (LinkHandle, mpsc::Receiver<LinkEvent>)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (control_tx, control_rx) = mpsc::channel(64);
    let (data_tx, data_rx) = mpsc::channel(64);
    let (event_tx, event_rx) = mpsc::channel(256);
    let SecureConn {
        mut reader,
        mut writer,
    } = conn;
    let closed = Arc::new(AtomicBool::new(false));

    // Writer task: hello, then control before data, plus keep-alive pings.
    let writer_events = event_tx.clone();
    let writer_closed = closed.clone();
    tokio::spawn(async move {
        let mut control_rx: mpsc::Receiver<Outgoing> = control_rx;
        let mut data_rx: mpsc::Receiver<Outgoing> = data_rx;
        let hello = Message::Hello {
            v: VERSION,
            id: local.id.to_hex(),
            name: local.name,
            pl: local.platform,
            app: local.app,
            caps: local.caps,
            role: local.role,
        };
        if let Err(e) = writer.write_frame(&hello, &[]).await {
            finish(&writer_events, &writer_closed, reason_for(&e)).await;
            return;
        }
        let mut next_ping = Instant::now() + cfg.ping_interval;
        loop {
            let item = tokio::select! {
                biased;
                Some(o) = control_rx.recv() => o,
                Some(o) = data_rx.recv() => o,
                () = sleep_until(next_ping) => Outgoing::Frame(Message::Ping, Vec::new()),
                else => break,
            };
            let result = match item {
                Outgoing::Frame(m, p) => writer.write_frame(&m, &p).await,
                Outgoing::Close(reason) => {
                    let _ = writer.write_frame(&Message::Bye { reason }, &[]).await;
                    finish(&writer_events, &writer_closed, CloseReason::Local).await;
                    return;
                }
            };
            if let Err(e) = result {
                finish(&writer_events, &writer_closed, reason_for(&e)).await;
                return;
            }
            next_ping = Instant::now() + cfg.ping_interval;
        }
    });

    // Reader task: dead-peer detection, pong replies, hello, everything else is an event.
    let pong_tx = control_tx.clone();
    tokio::spawn(async move {
        let reason = loop {
            let frame = match timeout(cfg.dead_after, reader.read_frame()).await {
                Err(_) => break CloseReason::Dead,
                Ok(Err(e)) => break reason_for(&e),
                Ok(Ok(f)) => f,
            };
            match frame.message {
                Message::Ping => {
                    let _ = pong_tx
                        .send(Outgoing::Frame(Message::Pong, Vec::new()))
                        .await;
                }
                Message::Pong | Message::Unknown { .. } => {}
                Message::Bye { reason } => break CloseReason::PeerBye(reason),
                Message::Hello {
                    ref id,
                    ref name,
                    ref pl,
                    ref app,
                    ref caps,
                    ref role,
                    ..
                } => {
                    if let Err(e) = frame.message.validate() {
                        break CloseReason::Error(e.to_string());
                    }
                    let Ok(id) = DeviceId::from_hex(id) else {
                        break CloseReason::Error("bad device id".into());
                    };
                    let info = PeerInfo {
                        id,
                        name: name.clone(),
                        platform: pl.clone(),
                        app: app.clone(),
                        caps: caps.clone(),
                        role: role.clone(),
                    };
                    if event_tx.send(LinkEvent::Ready(info)).await.is_err() {
                        return;
                    }
                }
                _ => {
                    if event_tx.send(LinkEvent::Frame(frame)).await.is_err() {
                        return;
                    }
                }
            }
        };
        finish(&event_tx, &closed, reason).await;
    });

    (
        LinkHandle {
            control: control_tx,
            data: data_tx,
        },
        event_rx,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use proto::group::GroupKey;
    use proto::handshake::{initiate, respond};
    use tokio::io::{duplex, DuplexStream};

    const UUID: &str = "11111111-1111-4111-8111-111111111111";

    fn info(n: u8, name: &str) -> LocalInfo {
        LocalInfo {
            id: DeviceId([n; 16]),
            name: name.into(),
            platform: "windows".into(),
            app: "0.1.0".into(),
            caps: vec!["text".into()],
            role: "control".into(),
        }
    }

    async fn pair() -> (SecureConn<DuplexStream>, SecureConn<DuplexStream>) {
        let (a, b) = duplex(1 << 20);
        let key = GroupKey([9; 32]);
        let (i, r) = tokio::join!(initiate(a, &key), respond(b, &key));
        (i.unwrap(), r.unwrap())
    }

    async fn next(rx: &mut mpsc::Receiver<LinkEvent>) -> LinkEvent {
        rx.recv().await.expect("event stream ended")
    }

    #[tokio::test(start_paused = true)]
    async fn hello_is_exchanged() {
        let (a, b) = pair().await;
        let (_ha, mut ea) = spawn_link(a, info(1, "PC"), LinkConfig::default());
        let (_hb, mut eb) = spawn_link(b, info(2, "Phone"), LinkConfig::default());
        let LinkEvent::Ready(peer_of_a) = next(&mut ea).await else {
            panic!("expected Ready")
        };
        let LinkEvent::Ready(peer_of_b) = next(&mut eb).await else {
            panic!("expected Ready")
        };
        assert_eq!(peer_of_a.name, "Phone");
        assert_eq!(peer_of_a.id, DeviceId([2; 16]));
        assert_eq!(peer_of_b.name, "PC");
    }

    #[tokio::test(start_paused = true)]
    async fn messages_flow_both_ways() {
        let (a, b) = pair().await;
        let (ha, mut ea) = spawn_link(a, info(1, "PC"), LinkConfig::default());
        let (hb, mut eb) = spawn_link(b, info(2, "Phone"), LinkConfig::default());
        let _ = next(&mut ea).await;
        let _ = next(&mut eb).await;
        ha.send_control(Message::Pull { id: UUID.into() }, vec![])
            .await
            .unwrap();
        let LinkEvent::Frame(f) = next(&mut eb).await else {
            panic!("expected frame")
        };
        assert_eq!(f.message, Message::Pull { id: UUID.into() });
        hb.send_control(
            Message::Ack {
                id: UUID.into(),
                ok: true,
                reason: None,
            },
            vec![],
        )
        .await
        .unwrap();
        let LinkEvent::Frame(f) = next(&mut ea).await else {
            panic!("expected frame")
        };
        assert!(matches!(f.message, Message::Ack { ok: true, .. }));
    }

    #[tokio::test(start_paused = true)]
    async fn keepalive_pings_keep_an_idle_link_alive() {
        let (a, b) = pair().await;
        let (ha, mut ea) = spawn_link(a, info(1, "PC"), LinkConfig::default());
        let (_hb, mut eb) = spawn_link(b, info(2, "Phone"), LinkConfig::default());
        let _ = next(&mut ea).await;
        let _ = next(&mut eb).await;
        // Ten minutes of silence: pings every 30 s must prevent the 90 s dead timeout.
        tokio::time::sleep(Duration::from_secs(600)).await;
        assert!(!ha.is_closed());
        assert!(
            ea.try_recv().is_err(),
            "no event expected on an idle healthy link"
        );
        assert!(eb.try_recv().is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn silent_peer_is_declared_dead_after_90_seconds() {
        let (a, b) = pair().await;
        let (_ha, mut ea) = spawn_link(a, info(1, "PC"), LinkConfig::default());
        // The peer completed the handshake but never speaks and never answers.
        let _silent = b;
        let started = Instant::now();
        let LinkEvent::Closed(reason) = next(&mut ea).await else {
            panic!("expected Closed")
        };
        assert_eq!(reason, CloseReason::Dead);
        let waited = started.elapsed();
        assert!(
            waited >= Duration::from_secs(90) && waited < Duration::from_secs(95),
            "{waited:?}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn control_frames_overtake_queued_data() {
        let (a, b) = pair().await;
        let (ha, mut ea) = spawn_link(a, info(1, "PC"), LinkConfig::default());
        let (_hb, mut eb) = spawn_link(b, info(2, "Phone"), LinkConfig::default());
        let _ = next(&mut ea).await;
        let _ = next(&mut eb).await;
        for i in 0..40u64 {
            ha.send_data(
                Message::FileChunk {
                    xfer_id: UUID.into(),
                    offset: i,
                },
                vec![0u8; 1000],
            )
            .await
            .unwrap();
        }
        ha.send_control(Message::Pull { id: UUID.into() }, vec![])
            .await
            .unwrap();
        let mut position = None;
        for index in 0..41 {
            let LinkEvent::Frame(f) = next(&mut eb).await else {
                panic!("expected frame")
            };
            if matches!(f.message, Message::Pull { .. }) {
                position = Some(index);
            }
        }
        let position = position.expect("control frame never arrived");
        assert!(position < 5, "control frame arrived at position {position}");
    }

    #[tokio::test(start_paused = true)]
    async fn bye_closes_the_peer_with_a_reason() {
        let (a, b) = pair().await;
        let (ha, mut ea) = spawn_link(a, info(1, "PC"), LinkConfig::default());
        let (_hb, mut eb) = spawn_link(b, info(2, "Phone"), LinkConfig::default());
        let _ = next(&mut ea).await;
        let _ = next(&mut eb).await;
        ha.close("app_exit").await;
        let LinkEvent::Closed(r) = next(&mut ea).await else {
            panic!("expected Closed")
        };
        assert_eq!(r, CloseReason::Local);
        let LinkEvent::Closed(r) = next(&mut eb).await else {
            panic!("expected Closed")
        };
        assert_eq!(r, CloseReason::PeerBye("app_exit".into()));
    }

    #[tokio::test(start_paused = true)]
    async fn dropping_the_transport_reports_peer_closed() {
        let (a, b) = pair().await;
        let (_ha, mut ea) = spawn_link(a, info(1, "PC"), LinkConfig::default());
        drop(b);
        let LinkEvent::Closed(r) = next(&mut ea).await else {
            panic!("expected Closed")
        };
        assert_eq!(r, CloseReason::PeerClosed);
    }
}
