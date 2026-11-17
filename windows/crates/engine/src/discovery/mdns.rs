//! mDNS registration and browsing of `_transclip._tcp.local.` (PROTOCOL.md section 4.1).

use std::net::{IpAddr, SocketAddr};

use mdns_sd::{ResolvedService, ServiceDaemon, ServiceEvent, ServiceInfo};
use proto::beacon::Platform;
use proto::ids::{DeviceId, GroupId};
use tokio::sync::mpsc;

use super::beacon::clamp_name;
use super::{DiscoveryError, PeerSeen, Via};

/// Service type advertised and browsed.
pub const SERVICE_TYPE: &str = "_transclip._tcp.local.";

/// What we announce about ourselves.
#[derive(Debug, Clone)]
pub struct MdnsConfig {
    /// Our group (only peers of this group are reported).
    pub gid: GroupId,
    /// Our device id (own announcements are ignored).
    pub device_id: DeviceId,
    /// Our TCP port.
    pub tcp_port: u16,
    /// Our platform.
    pub platform: Platform,
    /// Our display name.
    pub name: String,
}

/// Running mDNS service. Dropping it unregisters and stops the daemon.
pub struct MdnsService {
    daemon: ServiceDaemon,
    fullname: String,
}

fn platform_str(p: Platform) -> &'static str {
    match p {
        Platform::Windows => "windows",
        Platform::Android => "android",
    }
}

fn platform_from(s: &str) -> Option<Platform> {
    match s {
        "windows" => Some(Platform::Windows),
        "android" => Some(Platform::Android),
        _ => None,
    }
}

/// Convert a resolved service into a peer of our group, or `None` when it is foreign,
/// our own, or malformed. Prefers a private IPv4 address.
pub(crate) fn parse_resolved(
    svc: &ResolvedService,
    gid: GroupId,
    me: DeviceId,
) -> Option<PeerSeen> {
    if svc.get_property_val_str("v") != Some("1") {
        return None;
    }
    let their_gid = GroupId::from_hex(svc.get_property_val_str("gid")?).ok()?;
    let device_id = DeviceId::from_hex(svc.get_property_val_str("id")?).ok()?;
    if their_gid != gid || device_id == me || svc.get_port() == 0 {
        return None;
    }
    let platform = platform_from(svc.get_property_val_str("pl")?)?;
    let name = clamp_name(svc.get_property_val_str("nm").unwrap_or(""), 64);
    let mut v4: Vec<_> = svc.get_addresses_v4().into_iter().collect();
    v4.sort_by_key(|a| std::cmp::Reverse(a.is_private() || a.is_loopback()));
    let ip = v4.first().copied()?;
    Some(PeerSeen {
        device_id,
        addr: SocketAddr::new(IpAddr::V4(ip), svc.get_port()),
        platform,
        name,
        via: Via::Mdns,
    })
}

impl MdnsService {
    /// Register ourselves and browse for peers; matches are sent on `out`.
    pub fn start(cfg: &MdnsConfig, out: mpsc::Sender<PeerSeen>) -> Result<Self, DiscoveryError> {
        let map_err = |e: mdns_sd::Error| DiscoveryError::Mdns(e.to_string());
        let daemon = ServiceDaemon::new().map_err(map_err)?;

        let instance = clamp_name(&cfg.name, 63);
        let host = format!("tc-{}.local.", &cfg.device_id.to_hex()[..12]);
        let gid_hex = cfg.gid.to_hex();
        let id_hex = cfg.device_id.to_hex();
        let name = clamp_name(&cfg.name, 64);
        let props = [
            ("v", "1"),
            ("id", id_hex.as_str()),
            ("gid", gid_hex.as_str()),
            ("pl", platform_str(cfg.platform)),
            ("nm", name.as_str()),
        ];
        let info = ServiceInfo::new(SERVICE_TYPE, &instance, &host, "", cfg.tcp_port, &props[..])
            .map_err(map_err)?
            .enable_addr_auto();
        let fullname = info.get_fullname().to_string();
        daemon.register(info).map_err(map_err)?;

        let rx = daemon.browse(SERVICE_TYPE).map_err(map_err)?;
        let (gid, me) = (cfg.gid, cfg.device_id);
        std::thread::Builder::new()
            .name("transclip-mdns".into())
            .spawn(move || {
                while let Ok(event) = rx.recv() {
                    if let ServiceEvent::ServiceResolved(svc) = event {
                        if let Some(seen) = parse_resolved(&svc, gid, me) {
                            if out.blocking_send(seen).is_err() {
                                break;
                            }
                        }
                    }
                }
            })?;
        Ok(Self { daemon, fullname })
    }
}

impl Drop for MdnsService {
    fn drop(&mut self) {
        let _ = self.daemon.unregister(&self.fullname);
        let _ = self.daemon.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn cfg(id: u8, gid: GroupId) -> MdnsConfig {
        MdnsConfig {
            gid,
            device_id: DeviceId([id; 16]),
            tcp_port: 41000 + u16::from(id),
            platform: Platform::Windows,
            name: format!("mdns-dev{id}"),
        }
    }

    /// Needs working multicast on a real interface, so it is not part of the default run.
    #[tokio::test]
    #[ignore = "requires multicast networking"]
    async fn two_instances_find_each_other_via_mdns() {
        let gid = GroupId([9; 8]);
        let (ta, mut ra) = mpsc::channel(8);
        let (tb, _rb) = mpsc::channel(8);
        let _a = MdnsService::start(&cfg(1, gid), ta).unwrap();
        let _b = MdnsService::start(&cfg(2, gid), tb).unwrap();
        let seen = tokio::time::timeout(Duration::from_secs(15), ra.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(seen.device_id, DeviceId([2; 16]));
        assert_eq!(seen.addr.port(), 41002);
        assert_eq!(seen.via, Via::Mdns);
    }

    #[test]
    fn platform_round_trip() {
        for p in [Platform::Windows, Platform::Android] {
            assert_eq!(platform_from(platform_str(p)), Some(p));
        }
        assert_eq!(platform_from("linux"), None);
    }
}
