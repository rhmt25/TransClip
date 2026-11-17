//! UDP beacon sender/receiver (PROTOCOL.md section 4.2).

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use proto::beacon::{Beacon, Platform, FIXED_LEN, MAX_NAME};
use proto::ids::{DeviceId, GroupId};
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, Notify};
use tokio::task::JoinHandle;

use super::{DiscoveryError, PeerSeen, Via};

/// Default UDP discovery port.
pub const DEFAULT_PORT: u16 = 47821;
/// Interval between beacons.
pub const BEACON_INTERVAL: Duration = Duration::from_secs(10);

/// Beacon service configuration.
#[derive(Debug, Clone)]
pub struct BeaconConfig {
    /// Our group.
    pub gid: GroupId,
    /// Our device id (own beacons are ignored).
    pub device_id: DeviceId,
    /// Our advertised TCP port.
    pub tcp_port: u16,
    /// Our platform.
    pub platform: Platform,
    /// Our display name (truncated to 64 bytes on a character boundary).
    pub name: String,
    /// Local UDP port to listen on.
    pub listen_port: u16,
    /// Destinations for outgoing beacons.
    pub targets: Vec<SocketAddr>,
    /// Interval between beacons.
    pub interval: Duration,
}

impl BeaconConfig {
    /// Production defaults: listen on 47821 and broadcast to 255.255.255.255:47821.
    pub fn lan(
        gid: GroupId,
        device_id: DeviceId,
        tcp_port: u16,
        platform: Platform,
        name: String,
    ) -> Self {
        Self {
            gid,
            device_id,
            tcp_port,
            platform,
            name,
            listen_port: DEFAULT_PORT,
            targets: vec![SocketAddr::from((Ipv4Addr::BROADCAST, DEFAULT_PORT))],
            interval: BEACON_INTERVAL,
        }
    }
}

/// Truncate to at most `MAX_NAME` bytes without splitting a character.
pub(crate) fn clamp_name(name: &str, max: usize) -> String {
    let mut end = name.len().min(max);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    name[..end].to_string()
}

/// Running beacon service. Dropping it stops the tasks.
pub struct BeaconService {
    announce: Arc<Notify>,
    tasks: Vec<JoinHandle<()>>,
    local_addr: SocketAddr,
}

impl BeaconService {
    /// Bind the socket and start sending/receiving. Seen peers are delivered on `out`.
    pub fn start(cfg: BeaconConfig, out: mpsc::Sender<PeerSeen>) -> Result<Self, DiscoveryError> {
        let sock = bind(cfg.listen_port)?;
        let local_addr = sock.local_addr()?;
        let sock = Arc::new(sock);
        let packet = Beacon {
            gid: cfg.gid,
            device_id: cfg.device_id,
            port: cfg.tcp_port,
            platform: cfg.platform,
            name: clamp_name(&cfg.name, MAX_NAME),
        }
        .encode()?;
        let announce = Arc::new(Notify::new());

        let send_task = {
            let sock = Arc::clone(&sock);
            let announce = Arc::clone(&announce);
            let targets = cfg.targets.clone();
            let interval = cfg.interval;
            tokio::spawn(async move {
                let mut tick = tokio::time::interval(interval);
                loop {
                    tokio::select! {
                        _ = tick.tick() => {}
                        () = announce.notified() => {}
                    }
                    for t in &targets {
                        if let Err(e) = sock.send_to(&packet, t).await {
                            tracing::debug!(target = %t, error = %e, "beacon send failed");
                        }
                    }
                }
            })
        };

        let recv_task = {
            let sock = Arc::clone(&sock);
            let (gid, me) = (cfg.gid, cfg.device_id);
            tokio::spawn(async move {
                let mut buf = [0u8; FIXED_LEN + MAX_NAME + 1];
                loop {
                    let Ok((n, from)) = sock.recv_from(&mut buf).await else {
                        // Transient errors (e.g. ICMP reset on Windows) must not stop discovery.
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        continue;
                    };
                    let Some(seen) = accept(&buf[..n], from, gid, me) else {
                        continue;
                    };
                    if out.send(seen).await.is_err() {
                        break;
                    }
                }
            })
        };

        Ok(Self {
            announce,
            tasks: vec![send_task, recv_task],
            local_addr,
        })
    }

    /// Send a beacon immediately (for example after a network change).
    pub fn announce_now(&self) {
        self.announce.notify_one();
    }

    /// Local UDP address the receiver is bound to.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }
}

impl Drop for BeaconService {
    fn drop(&mut self) {
        for t in &self.tasks {
            t.abort();
        }
    }
}

/// Validate a datagram; returns the peer if it is a foreign beacon of our group.
fn accept(data: &[u8], from: SocketAddr, gid: GroupId, me: DeviceId) -> Option<PeerSeen> {
    let b = Beacon::decode(data).ok()?;
    if b.gid != gid || b.device_id == me || b.port == 0 {
        return None;
    }
    Some(PeerSeen {
        device_id: b.device_id,
        addr: SocketAddr::new(from.ip(), b.port),
        platform: b.platform,
        name: b.name,
        via: Via::Beacon,
    })
}

fn bind(port: u16) -> Result<UdpSocket, DiscoveryError> {
    let sock = socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::DGRAM,
        Some(socket2::Protocol::UDP),
    )?;
    sock.set_reuse_address(true)?;
    sock.set_broadcast(true)?;
    sock.set_nonblocking(true)?;
    sock.bind(&SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)).into())?;
    Ok(UdpSocket::from_std(sock.into())?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(id: u8, port: u16, target: u16, gid: GroupId) -> BeaconConfig {
        BeaconConfig {
            gid,
            device_id: DeviceId([id; 16]),
            tcp_port: 40000 + u16::from(id),
            platform: Platform::Windows,
            name: format!("dev{id}"),
            listen_port: port,
            targets: vec![SocketAddr::from((Ipv4Addr::LOCALHOST, target))],
            interval: Duration::from_millis(50),
        }
    }

    fn free_port() -> u16 {
        let s = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        s.local_addr().unwrap().port()
    }

    #[tokio::test]
    async fn two_instances_find_each_other() {
        let (pa, pb) = (free_port(), free_port());
        let gid = GroupId([7; 8]);
        let (ta, mut ra) = mpsc::channel(8);
        let (tb, mut rb) = mpsc::channel(8);
        let _a = BeaconService::start(cfg(1, pa, pb, gid), ta).unwrap();
        let _b = BeaconService::start(cfg(2, pb, pa, gid), tb).unwrap();
        let t = Duration::from_secs(3);
        let seen_by_a = tokio::time::timeout(t, ra.recv()).await.unwrap().unwrap();
        let seen_by_b = tokio::time::timeout(t, rb.recv()).await.unwrap().unwrap();
        assert_eq!(seen_by_a.device_id, DeviceId([2; 16]));
        assert_eq!(
            seen_by_a.addr,
            SocketAddr::from((Ipv4Addr::LOCALHOST, 40002))
        );
        assert_eq!(seen_by_a.name, "dev2");
        assert_eq!(seen_by_a.via, Via::Beacon);
        assert_eq!(seen_by_b.device_id, DeviceId([1; 16]));
    }

    #[tokio::test]
    async fn foreign_group_is_ignored() {
        let (pa, pb) = (free_port(), free_port());
        let (ta, mut ra) = mpsc::channel(8);
        let (tb, _rb) = mpsc::channel(8);
        let _a = BeaconService::start(cfg(1, pa, pb, GroupId([7; 8])), ta).unwrap();
        let _b = BeaconService::start(cfg(2, pb, pa, GroupId([8; 8])), tb).unwrap();
        let r = tokio::time::timeout(Duration::from_millis(400), ra.recv()).await;
        assert!(r.is_err(), "beacon of another group must be dropped");
    }

    #[test]
    fn own_and_garbage_packets_are_rejected() {
        let gid = GroupId([7; 8]);
        let me = DeviceId([1; 16]);
        let from = SocketAddr::from((Ipv4Addr::LOCALHOST, 1));
        let own = Beacon {
            gid,
            device_id: me,
            port: 1,
            platform: Platform::Android,
            name: String::new(),
        }
        .encode()
        .unwrap();
        assert!(accept(&own, from, gid, me).is_none());
        assert!(accept(b"garbage", from, gid, me).is_none());
        assert!(accept(&[], from, gid, me).is_none());
    }

    #[test]
    fn long_names_are_clamped_on_char_boundary() {
        let name = "\u{65e5}".repeat(30); // 90 bytes
        let c = clamp_name(&name, MAX_NAME);
        assert!(c.len() <= MAX_NAME);
        assert!(c.chars().all(|ch| ch == '\u{65e5}'));
    }

    #[tokio::test]
    async fn announce_now_sends_before_the_interval() {
        let (pa, pb) = (free_port(), free_port());
        let gid = GroupId([7; 8]);
        let (ta, _ra) = mpsc::channel(8);
        let (tb, mut rb) = mpsc::channel(8);
        let mut ca = cfg(1, pa, pb, gid);
        ca.interval = Duration::from_secs(3600);
        let a = BeaconService::start(ca, ta).unwrap();
        let _b = BeaconService::start(cfg(2, pb, pa, gid), tb).unwrap();
        // Drain the immediate first tick, then ask for another one.
        tokio::time::sleep(Duration::from_millis(100)).await;
        while rb.try_recv().is_ok() {}
        a.announce_now();
        let r = tokio::time::timeout(Duration::from_secs(2), rb.recv()).await;
        assert!(r.unwrap().is_some());
    }
}
