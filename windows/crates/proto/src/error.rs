//! Error type shared by every part of the protocol implementation.

use thiserror::Error;

/// Everything that can go wrong while speaking the TransClip protocol.
#[derive(Debug, Error)]
pub enum ProtoError {
    /// A size or count limit from PROTOCOL.md section 10 was exceeded.
    #[error("limit exceeded: {0}")]
    Limit(String),
    /// A frame header or message was malformed.
    #[error("bad header: {0}")]
    BadHeader(String),
    /// Authentication or decryption failed. Deliberately carries no detail.
    #[error("authentication failed")]
    Auth,
    /// The peer belongs to a different group.
    #[error("wrong group")]
    WrongGroup,
    /// The peer speaks a protocol version we do not support.
    #[error("unsupported protocol version {0}")]
    Version(u64),
    /// The other side closed the connection.
    #[error("connection closed")]
    Closed,
    /// A deadline passed (for example the 10 s handshake timeout).
    #[error("timeout")]
    Timeout,
    /// Underlying transport error.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Result alias used across the crate.
pub type Result<T> = std::result::Result<T, ProtoError>;
