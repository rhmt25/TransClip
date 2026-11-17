//! UDP discovery beacon (PROTOCOL.md section 4.2).
//!
//! ```text
//! 0   4  "TCLB"        4  1  version      5  8  group id     13 16 device id
//! 29  2  TCP port      31 1  platform     32 12 reserved      44 N  name (UTF-8, N <= 64)
//! ```

use crate::error::{ProtoError, Result};
use crate::ids::{DeviceId, GroupId};

/// Magic bytes at the start of every beacon.
pub const MAGIC: &[u8; 4] = b"TCLB";
/// Beacon format version.
pub const BEACON_VERSION: u8 = 1;
/// Fixed part of the packet.
pub const FIXED_LEN: usize = 44;
/// Longest device name in bytes.
pub const MAX_NAME: usize = 64;

/// Operating system of the announcing device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// Windows desktop.
    Windows,
    /// Android phone or tablet.
    Android,
}

impl Platform {
    fn code(self) -> u8 {
        match self {
            Platform::Windows => 1,
            Platform::Android => 2,
        }
    }

    fn from_code(c: u8) -> Option<Self> {
        match c {
            1 => Some(Platform::Windows),
            2 => Some(Platform::Android),
            _ => None,
        }
    }
}

/// A decoded beacon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Beacon {
    /// Group the sender belongs to.
    pub gid: GroupId,
    /// Sender device id.
    pub device_id: DeviceId,
    /// TCP port the sender listens on.
    pub port: u16,
    /// Sender platform.
    pub platform: Platform,
    /// Human readable device name.
    pub name: String,
}

impl Beacon {
    /// Serialize to the wire format.
    pub fn encode(&self) -> Result<Vec<u8>> {
        if self.name.len() > MAX_NAME {
            return Err(ProtoError::Limit("beacon name longer than 64 bytes".into()));
        }
        let mut out = Vec::with_capacity(FIXED_LEN + self.name.len());
        out.extend_from_slice(MAGIC);
        out.push(BEACON_VERSION);
        out.extend_from_slice(&self.gid.0);
        out.extend_from_slice(&self.device_id.0);
        out.extend_from_slice(&self.port.to_be_bytes());
        out.push(self.platform.code());
        out.extend_from_slice(&[0u8; 12]);
        out.extend_from_slice(self.name.as_bytes());
        Ok(out)
    }

    /// Parse a received datagram. Anything that is not a valid v1 beacon is an error.
    pub fn decode(data: &[u8]) -> Result<Self> {
        if data.len() < FIXED_LEN || &data[..4] != MAGIC {
            return Err(ProtoError::BadHeader("not a beacon".into()));
        }
        if data[4] != BEACON_VERSION {
            return Err(ProtoError::Version(u64::from(data[4])));
        }
        let name_bytes = &data[FIXED_LEN..];
        if name_bytes.len() > MAX_NAME {
            return Err(ProtoError::Limit("beacon name longer than 64 bytes".into()));
        }
        let name = std::str::from_utf8(name_bytes)
            .map_err(|_| ProtoError::BadHeader("beacon name is not UTF-8".into()))?
            .to_string();
        let platform = Platform::from_code(data[31])
            .ok_or_else(|| ProtoError::BadHeader("unknown platform".into()))?;
        let mut gid = [0u8; 8];
        gid.copy_from_slice(&data[5..13]);
        let mut dev = [0u8; 16];
        dev.copy_from_slice(&data[13..29]);
        Ok(Self {
            gid: GroupId(gid),
            device_id: DeviceId(dev),
            port: u16::from_be_bytes([data[29], data[30]]),
            platform,
            name,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(name: &str) -> Beacon {
        Beacon {
            gid: GroupId([1; 8]),
            device_id: DeviceId([2; 16]),
            port: 47820,
            platform: Platform::Android,
            name: name.to_string(),
        }
    }

    #[test]
    fn round_trip_including_multibyte_names() {
        for name in ["", "PC", "Ruang Tamu \u{1f4bb}", "\u{65e5}\u{672c}\u{8a9e}"] {
            let b = sample(name);
            assert_eq!(Beacon::decode(&b.encode().unwrap()).unwrap(), b);
        }
    }

    #[test]
    fn layout_matches_the_spec() {
        let bytes = sample("hi").encode().unwrap();
        assert_eq!(&bytes[..4], b"TCLB");
        assert_eq!(bytes[4], 1);
        assert_eq!(&bytes[5..13], &[1; 8]);
        assert_eq!(&bytes[13..29], &[2; 16]);
        assert_eq!(&bytes[29..31], &47820u16.to_be_bytes());
        assert_eq!(bytes[31], 2);
        assert_eq!(&bytes[32..44], &[0; 12]);
        assert_eq!(&bytes[44..], b"hi");
    }

    #[test]
    fn name_limit_is_64_bytes() {
        assert!(sample(&"a".repeat(64)).encode().is_ok());
        assert!(sample(&"a".repeat(65)).encode().is_err());
        let mut bytes = sample("").encode().unwrap();
        bytes.extend(vec![b'a'; 65]);
        assert!(Beacon::decode(&bytes).is_err());
    }

    #[test]
    fn rejects_garbage() {
        let good = sample("x").encode().unwrap();
        assert!(Beacon::decode(&good[..43]).is_err());
        let mut bad_magic = good.clone();
        bad_magic[0] = b'X';
        assert!(Beacon::decode(&bad_magic).is_err());
        let mut bad_version = good.clone();
        bad_version[4] = 2;
        assert!(matches!(
            Beacon::decode(&bad_version),
            Err(ProtoError::Version(2))
        ));
        let mut bad_platform = good.clone();
        bad_platform[31] = 9;
        assert!(Beacon::decode(&bad_platform).is_err());
        let mut bad_utf8 = good;
        bad_utf8.push(0xff);
        assert!(Beacon::decode(&bad_utf8).is_err());
    }

    #[test]
    fn reserved_bytes_are_ignored_on_decode() {
        let mut bytes = sample("x").encode().unwrap();
        bytes[40] = 0xaa;
        assert_eq!(Beacon::decode(&bytes).unwrap(), sample("x"));
    }
}
