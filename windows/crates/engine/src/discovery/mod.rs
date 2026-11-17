//! Peer discovery: UDP beacon, mDNS and an address cache (PROTOCOL.md section 4).

pub mod beacon;
pub mod cache;
pub mod mdns;

use std::net::SocketAddr;

use proto::beacon::Platform;
use proto::ids::DeviceId;

/// How a peer was discovered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    /// UDP broadcast beacon.
    Beacon,
    /// mDNS service browse.
    Mdns,
}

/// A peer of our group was seen on the network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerSeen {
    /// Peer device id.
    pub device_id: DeviceId,
    /// Address to dial (source IP plus advertised TCP port).
    pub addr: SocketAddr,
    /// Peer platform.
    pub platform: Platform,
    /// Peer display name.
    pub name: String,
    /// Discovery mechanism.
    pub via: Via,
}

/// Errors from the discovery layer.
#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    /// Socket or file I/O failed.
    #[error("discovery io: {0}")]
    Io(#[from] std::io::Error),
    /// mDNS daemon failure.
    #[error("mdns: {0}")]
    Mdns(String),
    /// Invalid input.
    #[error("discovery: {0}")]
    Proto(#[from] proto::ProtoError),
}
