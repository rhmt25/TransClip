//! Group key and group id (PROTOCOL.md section 3).

use crate::error::Result;
use crate::ids::{fill_random, GroupId};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, ZeroizeOnDrop};

const GID_DOMAIN: &[u8] = b"TransClip-v1 group-id";

/// 32-byte secret shared by every device of a group. Wiped from memory on drop.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct GroupKey(pub [u8; 32]);

impl GroupKey {
    /// Generate a fresh random key.
    pub fn generate() -> Result<Self> {
        let mut k = [0u8; 32];
        fill_random(&mut k)?;
        Ok(Self(k))
    }

    /// The group id other devices can see without learning the key.
    pub fn group_id(&self) -> GroupId {
        let mut h = Sha256::new();
        h.update(GID_DOMAIN);
        h.update(self.0);
        let d = h.finalize();
        let mut g = [0u8; 8];
        g.copy_from_slice(&d[..8]);
        GroupId(g)
    }
}

impl std::fmt::Debug for GroupKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GroupKey(..)")
    }
}

impl PartialEq for GroupKey {
    fn eq(&self, other: &Self) -> bool {
        use subtle::ConstantTimeEq;
        self.0.ct_eq(&other.0).into()
    }
}

impl Eq for GroupKey {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_id_is_stable_and_key_dependent() {
        let a = GroupKey([1u8; 32]);
        let b = GroupKey([2u8; 32]);
        assert_eq!(a.group_id(), GroupKey([1u8; 32]).group_id());
        assert_ne!(a.group_id(), b.group_id());
    }

    #[test]
    fn debug_never_prints_the_key() {
        assert_eq!(format!("{:?}", GroupKey([9u8; 32])), "GroupKey(..)");
    }

    #[test]
    fn generated_keys_differ() {
        assert_ne!(GroupKey::generate().unwrap(), GroupKey::generate().unwrap());
    }
}
