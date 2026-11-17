//! Pure clipboard synchronisation rules: duplicate suppression and conflict resolution
//! (PROTOCOL.md section 9.2). Time is passed in so everything is testable.

use crate::ids::DeviceId;
use std::collections::{HashMap, VecDeque};

/// Default number of remembered item ids.
pub const SEEN_CAPACITY: usize = 1024;
/// Default time an item id is remembered, in milliseconds (10 minutes).
pub const SEEN_TTL_MS: u64 = 10 * 60 * 1000;

/// Remembers recently seen item ids so the same item is applied only once.
#[derive(Debug)]
pub struct SeenCache {
    capacity: usize,
    ttl_ms: u64,
    order: VecDeque<(String, u64)>,
    index: HashMap<String, u64>,
}

impl SeenCache {
    /// Create a cache holding at most `capacity` ids for `ttl_ms` milliseconds each.
    pub fn new(capacity: usize, ttl_ms: u64) -> Self {
        Self {
            capacity,
            ttl_ms,
            order: VecDeque::new(),
            index: HashMap::new(),
        }
    }

    /// Cache with the protocol defaults (1024 ids, 10 minutes).
    pub fn with_defaults() -> Self {
        Self::new(SEEN_CAPACITY, SEEN_TTL_MS)
    }

    /// Record `id`. Returns `true` when it is new, `false` when it was already seen.
    pub fn insert(&mut self, id: &str, now_ms: u64) -> bool {
        self.expire(now_ms);
        if self.index.contains_key(id) {
            return false;
        }
        self.index.insert(id.to_string(), now_ms);
        self.order.push_back((id.to_string(), now_ms));
        while self.order.len() > self.capacity {
            if let Some((old, _)) = self.order.pop_front() {
                self.index.remove(&old);
            }
        }
        true
    }

    /// True when `id` is currently remembered.
    pub fn contains(&mut self, id: &str, now_ms: u64) -> bool {
        self.expire(now_ms);
        self.index.contains_key(id)
    }

    /// Number of ids currently remembered.
    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// True when nothing is remembered.
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    fn expire(&mut self, now_ms: u64) {
        while let Some((id, at)) = self.order.front() {
            if now_ms.saturating_sub(*at) < self.ttl_ms {
                break;
            }
            self.index.remove(id);
            self.order.pop_front();
        }
    }
}

/// Decides whether an incoming item is newer than the one already applied.
///
/// Newer `ts` wins; on equal `ts` the smaller origin device id wins; an identical
/// `(ts, origin)` pair is a duplicate and is not applied again.
#[derive(Debug, Default)]
pub struct ConflictClock {
    last: Option<(u64, DeviceId)>,
}

impl ConflictClock {
    /// Create a clock that has applied nothing yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` and remembers the item when it should be applied.
    pub fn should_apply(&mut self, ts: u64, origin: DeviceId) -> bool {
        let apply = match self.last {
            None => true,
            Some((last_ts, last_origin)) => ts > last_ts || (ts == last_ts && origin < last_origin),
        };
        if apply {
            self.last = Some((ts, origin));
        }
        apply
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicates_are_detected() {
        let mut c = SeenCache::with_defaults();
        assert!(c.insert("a", 0));
        assert!(!c.insert("a", 1));
        assert!(c.insert("b", 1));
        assert_eq!(c.len(), 2);
    }

    #[test]
    fn entries_expire_after_the_ttl() {
        let mut c = SeenCache::new(10, 1000);
        assert!(c.insert("a", 0));
        assert!(c.contains("a", 999));
        assert!(!c.contains("a", 1000));
        assert!(c.insert("a", 1000));
    }

    #[test]
    fn oldest_entries_are_evicted_beyond_capacity() {
        let mut c = SeenCache::new(3, 1_000_000);
        for (i, id) in ["a", "b", "c", "d"].iter().enumerate() {
            assert!(c.insert(id, i as u64));
        }
        assert_eq!(c.len(), 3);
        assert!(!c.contains("a", 10));
        assert!(c.contains("d", 10));
        assert!(c.insert("a", 11));
    }

    #[test]
    fn default_limits_match_the_spec() {
        let mut c = SeenCache::with_defaults();
        for i in 0..2000 {
            c.insert(&format!("id{i}"), i);
        }
        assert_eq!(c.len(), SEEN_CAPACITY);
    }

    fn dev(n: u8) -> DeviceId {
        DeviceId([n; 16])
    }

    #[test]
    fn newer_timestamp_wins() {
        let mut k = ConflictClock::new();
        assert!(k.should_apply(10, dev(5)));
        assert!(k.should_apply(11, dev(9)));
        assert!(!k.should_apply(10, dev(1)));
    }

    #[test]
    fn equal_timestamp_prefers_smaller_origin() {
        let mut k = ConflictClock::new();
        assert!(k.should_apply(10, dev(5)));
        assert!(!k.should_apply(10, dev(7)));
        assert!(k.should_apply(10, dev(3)));
        assert!(!k.should_apply(10, dev(3)));
    }
}
