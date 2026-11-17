//! Per-peer queue of clipboard items waiting for the peer to come online.

use std::collections::VecDeque;
use std::sync::Arc;

use crate::clip::{Clip, ItemMeta};

/// PROTOCOL.md section 10: at most 5 queued clipboard items per peer.
pub const MAX_QUEUED: usize = 5;

/// An item waiting for delivery.
#[derive(Debug, Clone)]
pub struct QueuedItem {
    /// Identity and ordering data, kept from the moment the item was created.
    pub meta: ItemMeta,
    /// The content, shared between peers' queues.
    pub clip: Arc<Clip>,
}

/// FIFO with a fixed capacity; the oldest item is dropped when it overflows.
#[derive(Debug, Default)]
pub struct OfflineQueue {
    items: VecDeque<QueuedItem>,
}

impl OfflineQueue {
    /// Empty queue.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an item (ignored when the id is already queued). Returns the evicted item, if any.
    pub fn push(&mut self, item: QueuedItem) -> Option<QueuedItem> {
        if self.items.iter().any(|i| i.meta.id == item.meta.id) {
            return None;
        }
        self.items.push_back(item);
        if self.items.len() > MAX_QUEUED {
            self.items.pop_front()
        } else {
            None
        }
    }

    /// Take all items in order.
    pub fn drain(&mut self) -> Vec<QueuedItem> {
        self.items.drain(..).collect()
    }

    /// Number of queued items.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// True when nothing is queued.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proto::ids::DeviceId;

    fn item(n: u8) -> QueuedItem {
        QueuedItem {
            meta: ItemMeta {
                id: format!("id{n}"),
                origin: DeviceId([1; 16]),
                ts: u64::from(n),
                hash: String::new(),
            },
            clip: Arc::new(Clip::Text(format!("t{n}"))),
        }
    }

    #[test]
    fn keeps_the_newest_five_in_order() {
        let mut q = OfflineQueue::new();
        let mut evicted = vec![];
        for n in 0..8 {
            if let Some(e) = q.push(item(n)) {
                evicted.push(e.meta.id);
            }
        }
        assert_eq!(evicted, ["id0", "id1", "id2"]);
        let ids: Vec<_> = q.drain().into_iter().map(|i| i.meta.id).collect();
        assert_eq!(ids, ["id3", "id4", "id5", "id6", "id7"]);
        assert!(q.is_empty());
    }

    #[test]
    fn duplicate_ids_are_not_queued_twice() {
        let mut q = OfflineQueue::new();
        q.push(item(1));
        q.push(item(1));
        assert_eq!(q.len(), 1);
    }
}
