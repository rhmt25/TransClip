//! Per-peer rate limit for incoming clipboard items (PLAN section 7): at most
//! [`MAX_ITEMS`] items per [`WINDOW`], so a misbehaving peer cannot flood the clipboard.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use proto::ids::DeviceId;

/// Items allowed per window.
pub const MAX_ITEMS: usize = 20;
/// Length of the sliding window.
pub const WINDOW: Duration = Duration::from_secs(10);

/// Sliding-window counter per peer.
#[derive(Default)]
pub struct PeerRate {
    hits: HashMap<DeviceId, VecDeque<Instant>>,
}

impl PeerRate {
    /// Record an item from `peer` at `now`; false when the peer is over its limit.
    pub fn allow(&mut self, peer: DeviceId, now: Instant) -> bool {
        let q = self.hits.entry(peer).or_default();
        while q
            .front()
            .is_some_and(|t| now.saturating_duration_since(*t) >= WINDOW)
        {
            q.pop_front();
        }
        if q.len() >= MAX_ITEMS {
            return false;
        }
        q.push_back(now);
        true
    }

    /// Forget a peer (it went offline for good).
    pub fn forget(&mut self, peer: &DeviceId) {
        self.hits.remove(peer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twenty_pass_then_blocked_until_the_window_slides() {
        let mut r = PeerRate::default();
        let p = DeviceId([1; 16]);
        let t0 = Instant::now();
        for _ in 0..MAX_ITEMS {
            assert!(r.allow(p, t0));
        }
        assert!(!r.allow(p, t0 + Duration::from_secs(5)));
        assert!(r.allow(p, t0 + WINDOW));
    }

    #[test]
    fn peers_are_limited_independently() {
        let mut r = PeerRate::default();
        let t0 = Instant::now();
        for _ in 0..MAX_ITEMS {
            assert!(r.allow(DeviceId([1; 16]), t0));
        }
        assert!(r.allow(DeviceId([2; 16]), t0));
    }
}
