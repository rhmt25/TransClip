//! Last-known address cache persisted as `peers.json` (PROTOCOL.md section 4.3).

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use proto::ids::DeviceId;
use serde::{Deserialize, Serialize};

use super::DiscoveryError;

#[derive(Serialize, Deserialize, Default)]
struct Stored {
    peers: BTreeMap<String, String>,
}

/// Map `device_id` to the last address that worked.
#[derive(Debug, Default, Clone)]
pub struct AddressCache {
    path: Option<PathBuf>,
    map: BTreeMap<[u8; 16], SocketAddr>,
}

impl AddressCache {
    /// In-memory cache without persistence.
    pub fn in_memory() -> Self {
        Self::default()
    }

    /// Load from `path`. A missing, corrupt or partially invalid file yields what could be read.
    pub fn load(path: &Path) -> Self {
        let mut map = BTreeMap::new();
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(stored) = serde_json::from_str::<Stored>(&text) {
                for (k, v) in stored.peers {
                    if let (Ok(id), Ok(addr)) = (DeviceId::from_hex(&k), v.parse::<SocketAddr>()) {
                        map.insert(id.0, addr);
                    }
                }
            }
        }
        Self {
            path: Some(path.to_path_buf()),
            map,
        }
    }

    /// Cached address for a device.
    pub fn get(&self, id: &DeviceId) -> Option<SocketAddr> {
        self.map.get(&id.0).copied()
    }

    /// Forget a device.
    pub fn remove(&mut self, id: &DeviceId) {
        self.map.remove(&id.0);
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// True when empty.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Record a successful address and persist atomically when it changed.
    pub fn record(&mut self, id: DeviceId, addr: SocketAddr) -> Result<(), DiscoveryError> {
        if self.map.insert(id.0, addr) == Some(addr) {
            return Ok(());
        }
        self.save()
    }

    /// Write the cache via temp file and rename.
    pub fn save(&self) -> Result<(), DiscoveryError> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let stored = Stored {
            peers: self
                .map
                .iter()
                .map(|(k, v)| (DeviceId(*k).to_hex(), v.to_string()))
                .collect(),
        };
        let json = serde_json::to_vec_pretty(&stored)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tc-cache-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("peers.json")
    }

    #[test]
    fn persists_and_reloads() {
        let p = temp_path("rt");
        let mut c = AddressCache::load(&p);
        assert!(c.is_empty());
        let addr: SocketAddr = "192.168.1.5:47820".parse().unwrap();
        c.record(DeviceId([3; 16]), addr).unwrap();
        let c2 = AddressCache::load(&p);
        assert_eq!(c2.get(&DeviceId([3; 16])), Some(addr));
        assert_eq!(c2.len(), 1);
    }

    #[test]
    fn corrupt_file_and_bad_entries_are_tolerated() {
        let p = temp_path("bad");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "{not json").unwrap();
        assert!(AddressCache::load(&p).is_empty());
        let bad_addr = DeviceId([1; 16]).to_hex();
        let ok = DeviceId([2; 16]).to_hex();
        std::fs::write(
            &p,
            format!(
                r#"{{"peers":{{"zz":"1.2.3.4:5","{bad_addr}":"nonsense","{ok}":"10.0.0.1:9"}}}}"#
            ),
        )
        .unwrap();
        let c = AddressCache::load(&p);
        assert_eq!(c.len(), 1);
        assert!(c.get(&DeviceId([2; 16])).is_some());
    }

    #[test]
    fn in_memory_never_touches_disk() {
        let mut c = AddressCache::in_memory();
        c.record(DeviceId([1; 16]), "1.1.1.1:1".parse().unwrap())
            .unwrap();
        assert!(c.get(&DeviceId([1; 16])).is_some());
        c.remove(&DeviceId([1; 16]));
        assert!(c.is_empty());
    }
}
