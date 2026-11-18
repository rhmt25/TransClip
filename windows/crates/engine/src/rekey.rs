//! Group key rotation (PROTOCOL.md section 9.4).
//!
//! The initiator sends `rekey` to every online peer that is not being revoked, then both sides
//! restart their [`crate::peers::PeerManager`] with the new key. A revoked peer still holds the
//! old key, so its handshake with the new group fails.

use std::time::{SystemTime, UNIX_EPOCH};

use proto::group::GroupKey;
use proto::ids::{b64url_decode_exact, b64url_encode, DeviceId};
use proto::msg::Message;

use crate::peers::PeerManager;

/// A rotation request received from a peer.
#[derive(Debug, PartialEq, Eq)]
pub struct RekeyRequest {
    /// The key the whole group switches to.
    pub new_gk: GroupKey,
    /// Group name.
    pub name: String,
    /// Device that started the rotation.
    pub by: DeviceId,
    /// Timestamp to echo in the acknowledgement.
    pub ts: u64,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Read a `rekey` message; `None` for other messages or an invalid key.
pub fn parse(message: &Message) -> Option<RekeyRequest> {
    let Message::Rekey {
        new_gk,
        name,
        by,
        ts,
    } = message
    else {
        return None;
    };
    let key = b64url_decode_exact::<32>(new_gk).ok()?;
    let by = DeviceId::from_hex(by).ok()?;
    Some(RekeyRequest {
        new_gk: GroupKey(key),
        name: name.clone(),
        by,
        ts: *ts,
    })
}

/// The acknowledgement for `req`.
pub fn ack(req: &RekeyRequest) -> Message {
    Message::RekeyAck { ts: req.ts }
}

/// Send the new key to `targets` (skip revoked peers by leaving them out). Returns how many
/// peers accepted the message into their send queue.
pub async fn broadcast(
    mgr: &PeerManager,
    new_gk: &GroupKey,
    name: &str,
    by: DeviceId,
    targets: &[DeviceId],
) -> usize {
    let ts = now_secs();
    let mut sent = 0;
    for peer in targets {
        let msg = Message::Rekey {
            new_gk: b64url_encode(&new_gk.0),
            name: name.to_owned(),
            by: by.to_hex(),
            ts,
        };
        if mgr.send(peer, msg, Vec::new()).await.is_ok() {
            sent += 1;
        }
    }
    sent
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::{PeerSeen, Via};
    use crate::link::LocalInfo;
    use crate::peers::{ManagerConfig, ManagerEvent};
    use proto::beacon::Platform;
    use std::net::SocketAddr;
    use std::time::Duration;
    use tokio::sync::mpsc;

    struct Node {
        mgr: PeerManager,
        events: mpsc::Receiver<ManagerEvent>,
        seen: mpsc::Sender<PeerSeen>,
    }

    async fn node(id: u8, key: &GroupKey) -> Node {
        let local = LocalInfo {
            id: DeviceId([id; 16]),
            name: format!("dev{id}"),
            platform: "windows".into(),
            app: "test".into(),
            caps: vec![],
            role: "full".into(),
        };
        let mut c = ManagerConfig::new(local, key.clone(), SocketAddr::from(([127, 0, 0, 1], 0)));
        c.backoff_min = Duration::from_millis(50);
        c.backoff_max = Duration::from_millis(200);
        c.fallback_after = Duration::from_secs(3600);
        let (tx, rx) = mpsc::channel(16);
        let (mgr, events) = PeerManager::start(c, rx).await.unwrap();
        Node {
            mgr,
            events,
            seen: tx,
        }
    }

    fn seen(id: u8, addr: SocketAddr) -> PeerSeen {
        PeerSeen {
            device_id: DeviceId([id; 16]),
            addr,
            platform: Platform::Windows,
            name: format!("dev{id}"),
            via: Via::Beacon,
        }
    }

    async fn wait_online(n: &mut Node, id: u8) {
        let id = DeviceId([id; 16]);
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(5), n.events.recv())
                .await
                .unwrap()
                .unwrap();
            if matches!(&ev, ManagerEvent::Online(i) if i.id == id) {
                return;
            }
        }
    }

    async fn next_rekey(n: &mut Node) -> RekeyRequest {
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(5), n.events.recv())
                .await
                .unwrap()
                .unwrap();
            if let ManagerEvent::Frame { frame, .. } = ev {
                if let Some(r) = parse(&frame.message) {
                    return r;
                }
            }
        }
    }

    #[test]
    fn parse_rejects_bad_keys_and_other_messages() {
        let bad = Message::Rekey {
            new_gk: "short".into(),
            name: "g".into(),
            by: DeviceId([1; 16]).to_hex(),
            ts: 1,
        };
        assert!(parse(&bad).is_none());
        assert!(parse(&Message::RekeyAck { ts: 1 }).is_none());
    }

    #[tokio::test]
    async fn revoked_peer_cannot_join_the_new_group() {
        let old = GroupKey([7; 32]);
        let new = GroupKey([9; 32]);
        let mut a = node(1, &old).await;
        let mut b = node(2, &old).await;
        let c = node(3, &old).await;
        a.seen.send(seen(2, b.mgr.local_addr())).await.unwrap();
        a.seen.send(seen(3, c.mgr.local_addr())).await.unwrap();
        wait_online(&mut a, 2).await;
        wait_online(&mut a, 3).await;

        // A revokes C: only B receives the new key.
        let targets = [DeviceId([2; 16])];
        assert_eq!(
            broadcast(&a.mgr, &new, "home", DeviceId([1; 16]), &targets).await,
            1
        );
        let req = next_rekey(&mut b).await;
        assert_eq!(req.new_gk, new);
        assert_eq!(req.by, DeviceId([1; 16]));
        assert_eq!(ack(&req), Message::RekeyAck { ts: req.ts });

        // Both sides restart with the new key; C keeps the old one.
        a.mgr.shutdown().await;
        b.mgr.shutdown().await;
        let mut a2 = node(1, &req.new_gk).await;
        let b2 = node(2, &req.new_gk).await;
        a2.seen.send(seen(2, b2.mgr.local_addr())).await.unwrap();
        wait_online(&mut a2, 2).await;
        a2.seen.send(seen(3, c.mgr.local_addr())).await.unwrap();
        let r = tokio::time::timeout(Duration::from_millis(800), a2.events.recv()).await;
        assert!(r.is_err(), "revoked peer must not link: {r:?}");
        assert_eq!(a2.mgr.online_peers(), vec![DeviceId([2; 16])]);
    }
}
