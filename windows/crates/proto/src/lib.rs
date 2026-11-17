//! TransClip wire protocol: framing, encryption, handshake and the pure rules
//! that both implementations must share. No platform code lives here.

pub mod beacon;
pub mod error;
pub mod frame;
pub mod group;
pub mod handshake;
pub mod ids;
pub mod msg;
pub mod pairing;
pub mod path;
pub mod record;
pub mod sync_rules;

pub use error::{ProtoError, Result};
