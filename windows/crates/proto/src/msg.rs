//! All protocol messages (PROTOCOL.md sections 6 and 9).
//!
//! The frame-level `size` field is owned by the frame codec, not by these structs.

use crate::error::{ProtoError, Result};
use crate::ids::{is_sha256_hex, is_uuid_v4, DeviceId, GroupId};
use serde::{Deserialize, Serialize};

/// Protocol version spoken by this implementation.
pub const VERSION: u64 = 1;

/// Maximum text clipboard payload (8 MiB).
pub const MAX_TEXT: usize = 8 * 1024 * 1024;
/// Maximum image clipboard payload (50 MiB).
pub const MAX_IMAGE: usize = 50 * 1024 * 1024;
/// Maximum file chunk payload (64 KiB).
pub const MAX_CHUNK: usize = 64 * 1024;

/// Kind of a transferred entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    /// A regular file.
    File,
    /// A directory (no stream, created by the receiver).
    Dir,
}

/// One protocol message. The JSON `t` field selects the variant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Message {
    /// Handshake step 1 (initiator to responder).
    Hs1 {
        v: u64,
        gid: String,
        eph_i: String,
        n_i: String,
    },
    /// Handshake step 2 (responder to initiator).
    Hs2 {
        v: u64,
        eph_r: String,
        n_r: String,
        mac_r: String,
    },
    /// Handshake step 3 (initiator to responder).
    Hs3 { mac_i: String },
    /// First message after the handshake, both directions.
    Hello {
        v: u64,
        id: String,
        name: String,
        pl: String,
        app: String,
        caps: Vec<String>,
        role: String,
    },
    /// Keep-alive request.
    Ping,
    /// Keep-alive answer.
    Pong,
    /// Error report.
    Error {
        code: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        msg: Option<String>,
        #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
        reference: Option<String>,
    },
    /// Polite close.
    Bye { reason: String },
    /// Text clipboard item; payload is UTF-8 text.
    Text {
        id: String,
        origin: String,
        ts: u64,
        hash: String,
    },
    /// Image clipboard item; payload is the image bytes.
    Image {
        id: String,
        origin: String,
        ts: u64,
        hash: String,
        mime: String,
        w: u32,
        h: u32,
    },
    /// Announcement of an item too large to push right away.
    Announce {
        id: String,
        origin: String,
        ts: u64,
        hash: String,
        kind: String,
        mime: String,
        w: u32,
        h: u32,
        bytes: u64,
    },
    /// Request the payload of an announced item.
    Pull { id: String },
    /// Acknowledge a clipboard item.
    Ack {
        id: String,
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// Offer one file or directory of a batch.
    FileOffer {
        xfer_id: String,
        batch_id: String,
        batch_name: String,
        batch_count: u32,
        batch_bytes: u64,
        kind: EntryKind,
        relative_path: String,
        bytes: u64,
        sha256: String,
        mtime: u64,
    },
    /// Accept an offer, optionally resuming at `offset`.
    FileAccept { xfer_id: String, offset: u64 },
    /// Reject an offer.
    FileReject { xfer_id: String, reason: String },
    /// Open the data stream of an accepted transfer.
    FileStream { xfer_id: String, offset: u64 },
    /// One chunk of file data; payload is the bytes.
    FileChunk { xfer_id: String, offset: u64 },
    /// End of the data stream.
    FileEnd { xfer_id: String },
    /// Final result of one entry.
    FileResult {
        xfer_id: String,
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// Cancel a transfer.
    FileCancel { xfer_id: String, reason: String },
    /// Summary of a finished batch.
    BatchDone {
        batch_id: String,
        ok_count: u32,
        fail_count: u32,
    },
    /// Group key rotation.
    Rekey {
        new_gk: String,
        name: String,
        by: String,
        ts: u64,
    },
    /// Acknowledge a rotation.
    RekeyAck { ts: u64 },
    /// A message type this version does not know. Never serialized.
    #[serde(skip)]
    Unknown { t: String },
}

/// Every `t` value this version understands.
pub const KNOWN_TYPES: &[&str] = &[
    "hs1",
    "hs2",
    "hs3",
    "hello",
    "ping",
    "pong",
    "error",
    "bye",
    "text",
    "image",
    "announce",
    "pull",
    "ack",
    "file_offer",
    "file_accept",
    "file_reject",
    "file_stream",
    "file_chunk",
    "file_end",
    "file_result",
    "file_cancel",
    "batch_done",
    "rekey",
    "rekey_ack",
];

impl Message {
    /// The wire name of this message type.
    pub fn type_name(&self) -> &str {
        match self {
            Message::Hs1 { .. } => "hs1",
            Message::Hs2 { .. } => "hs2",
            Message::Hs3 { .. } => "hs3",
            Message::Hello { .. } => "hello",
            Message::Ping => "ping",
            Message::Pong => "pong",
            Message::Error { .. } => "error",
            Message::Bye { .. } => "bye",
            Message::Text { .. } => "text",
            Message::Image { .. } => "image",
            Message::Announce { .. } => "announce",
            Message::Pull { .. } => "pull",
            Message::Ack { .. } => "ack",
            Message::FileOffer { .. } => "file_offer",
            Message::FileAccept { .. } => "file_accept",
            Message::FileReject { .. } => "file_reject",
            Message::FileStream { .. } => "file_stream",
            Message::FileChunk { .. } => "file_chunk",
            Message::FileEnd { .. } => "file_end",
            Message::FileResult { .. } => "file_result",
            Message::FileCancel { .. } => "file_cancel",
            Message::BatchDone { .. } => "batch_done",
            Message::Rekey { .. } => "rekey",
            Message::RekeyAck { .. } => "rekey_ack",
            Message::Unknown { t } => t,
        }
    }

    /// Largest payload this message type may carry (PROTOCOL.md section 10).
    pub fn max_payload(&self) -> usize {
        match self {
            Message::Text { .. } => MAX_TEXT,
            Message::Image { .. } => MAX_IMAGE,
            Message::FileChunk { .. } => MAX_CHUNK,
            // Unknown frames are read and discarded, so allow the largest known payload.
            Message::Unknown { .. } => MAX_IMAGE,
            _ => 0,
        }
    }

    /// Check field formats (ids, hashes, versions). Does not check payload or paths.
    pub fn validate(&self) -> Result<()> {
        match self {
            Message::Hs1 { v, gid, .. } => {
                check_version(*v)?;
                GroupId::from_hex(gid)?;
            }
            Message::Hs2 { v, .. } => check_version(*v)?,
            Message::Hello { v, id, .. } => {
                check_version(*v)?;
                DeviceId::from_hex(id)?;
            }
            Message::Text {
                id, origin, hash, ..
            } => {
                uuid(id)?;
                device(origin)?;
                sha256(hash)?;
            }
            Message::Image {
                id, origin, hash, ..
            }
            | Message::Announce {
                id, origin, hash, ..
            } => {
                uuid(id)?;
                device(origin)?;
                sha256(hash)?;
            }
            Message::Pull { id } | Message::Ack { id, .. } => uuid(id)?,
            Message::FileOffer {
                xfer_id,
                batch_id,
                kind,
                bytes,
                sha256: digest,
                ..
            } => {
                uuid(xfer_id)?;
                uuid(batch_id)?;
                match kind {
                    EntryKind::File => sha256(digest)?,
                    EntryKind::Dir => {
                        if *bytes != 0 {
                            return Err(bad("directory entries must have bytes = 0"));
                        }
                    }
                }
            }
            Message::FileAccept { xfer_id, .. }
            | Message::FileReject { xfer_id, .. }
            | Message::FileStream { xfer_id, .. }
            | Message::FileChunk { xfer_id, .. }
            | Message::FileEnd { xfer_id }
            | Message::FileResult { xfer_id, .. }
            | Message::FileCancel { xfer_id, .. } => uuid(xfer_id)?,
            Message::BatchDone { batch_id, .. } => uuid(batch_id)?,
            Message::Rekey { by, .. } => device(by)?,
            _ => {}
        }
        Ok(())
    }
}

fn bad(msg: &str) -> ProtoError {
    ProtoError::BadHeader(msg.to_string())
}

fn check_version(v: u64) -> Result<()> {
    if v == VERSION {
        Ok(())
    } else {
        Err(ProtoError::Version(v))
    }
}

fn uuid(s: &str) -> Result<()> {
    if is_uuid_v4(s) {
        Ok(())
    } else {
        Err(bad("expected a lowercase UUIDv4"))
    }
}

fn device(s: &str) -> Result<()> {
    DeviceId::from_hex(s).map(|_| ())
}

fn sha256(s: &str) -> Result<()> {
    if is_sha256_hex(s) {
        Ok(())
    } else {
        Err(bad("expected a 64-character lowercase SHA-256 hex digest"))
    }
}

pub const SAMPLE_UUID_A: &str = "11111111-1111-4111-8111-111111111111";
pub const SAMPLE_UUID_B: &str = "22222222-2222-4222-9222-222222222222";

fn sample_device() -> String {
    "ab".repeat(16)
}
fn sample_digest() -> String {
    "cd".repeat(32)
}

/// One instance of every variant except `Unknown`. Used by tests and test vectors.
pub fn sample_messages() -> Vec<Message> {
    vec![
        Message::Hs1 {
            v: 1,
            gid: "00".repeat(8),
            eph_i: "A".repeat(43),
            n_i: "B".repeat(22),
        },
        Message::Hs2 {
            v: 1,
            eph_r: "A".repeat(43),
            n_r: "B".repeat(22),
            mac_r: "C".repeat(43),
        },
        Message::Hs3 {
            mac_i: "C".repeat(43),
        },
        Message::Hello {
            v: 1,
            id: sample_device(),
            name: "PC".into(),
            pl: "windows".into(),
            app: "0.1.0".into(),
            caps: vec!["text".into(), "image".into()],
            role: "control".into(),
        },
        Message::Ping,
        Message::Pong,
        Message::Error {
            code: "limit".into(),
            msg: Some("x".into()),
            reference: Some("r".into()),
        },
        Message::Bye {
            reason: "app_exit".into(),
        },
        Message::Text {
            id: SAMPLE_UUID_A.into(),
            origin: sample_device(),
            ts: 1,
            hash: sample_digest(),
        },
        Message::Image {
            id: SAMPLE_UUID_A.into(),
            origin: sample_device(),
            ts: 2,
            hash: sample_digest(),
            mime: "image/png".into(),
            w: 10,
            h: 20,
        },
        Message::Announce {
            id: SAMPLE_UUID_A.into(),
            origin: sample_device(),
            ts: 3,
            hash: sample_digest(),
            kind: "image".into(),
            mime: "image/png".into(),
            w: 1,
            h: 2,
            bytes: 3,
        },
        Message::Pull {
            id: SAMPLE_UUID_A.into(),
        },
        Message::Ack {
            id: SAMPLE_UUID_A.into(),
            ok: false,
            reason: Some("busy".into()),
        },
        Message::FileOffer {
            xfer_id: SAMPLE_UUID_A.into(),
            batch_id: SAMPLE_UUID_B.into(),
            batch_name: "photos".into(),
            batch_count: 2,
            batch_bytes: 10,
            kind: EntryKind::File,
            relative_path: "photos/a.jpg".into(),
            bytes: 5,
            sha256: sample_digest(),
            mtime: 99,
        },
        Message::FileAccept {
            xfer_id: SAMPLE_UUID_A.into(),
            offset: 7,
        },
        Message::FileReject {
            xfer_id: SAMPLE_UUID_A.into(),
            reason: "quota".into(),
        },
        Message::FileStream {
            xfer_id: SAMPLE_UUID_A.into(),
            offset: 0,
        },
        Message::FileChunk {
            xfer_id: SAMPLE_UUID_A.into(),
            offset: 64,
        },
        Message::FileEnd {
            xfer_id: SAMPLE_UUID_A.into(),
        },
        Message::FileResult {
            xfer_id: SAMPLE_UUID_A.into(),
            ok: true,
            reason: None,
        },
        Message::FileCancel {
            xfer_id: SAMPLE_UUID_A.into(),
            reason: "cancelled".into(),
        },
        Message::BatchDone {
            batch_id: SAMPLE_UUID_B.into(),
            ok_count: 1,
            fail_count: 0,
        },
        Message::Rekey {
            new_gk: "A".repeat(43),
            name: "home".into(),
            by: sample_device(),
            ts: 4,
        },
        Message::RekeyAck { ts: 5 },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variant_round_trips_through_json() {
        for m in sample_messages() {
            let json = serde_json::to_string(&m).unwrap();
            let back: Message = serde_json::from_str(&json).unwrap();
            assert_eq!(m, back, "{json}");
            assert!(
                json.contains(&format!("\"t\":\"{}\"", m.type_name())),
                "{json}"
            );
        }
    }

    #[test]
    fn known_types_match_the_variants() {
        let names: Vec<&str> = sample_messages()
            .iter()
            .map(|m| m.type_name().to_owned())
            .map(|s| KNOWN_TYPES.iter().copied().find(|k| *k == s).unwrap())
            .collect();
        assert_eq!(names.len(), KNOWN_TYPES.len());
    }

    #[test]
    fn extra_json_fields_are_ignored() {
        let m: Message =
            serde_json::from_str(r#"{"t":"pull","id":"x","size":0,"future":[1,2]}"#).unwrap();
        assert_eq!(m, Message::Pull { id: "x".into() });
    }

    #[test]
    fn samples_validate() {
        for m in sample_messages() {
            m.validate().unwrap_or_else(|e| panic!("{m:?}: {e}"));
        }
    }

    #[test]
    fn validation_rejects_bad_fields() {
        let bad_id = Message::Pull {
            id: "not-a-uuid".into(),
        };
        assert!(bad_id.validate().is_err());
        let bad_ver = Message::Hello {
            v: 2,
            id: sample_device(),
            name: String::new(),
            pl: String::new(),
            app: String::new(),
            caps: vec![],
            role: String::new(),
        };
        assert!(matches!(bad_ver.validate(), Err(ProtoError::Version(2))));
        let dir_with_bytes = Message::FileOffer {
            xfer_id: SAMPLE_UUID_A.into(),
            batch_id: SAMPLE_UUID_B.into(),
            batch_name: String::new(),
            batch_count: 1,
            batch_bytes: 0,
            kind: EntryKind::Dir,
            relative_path: "d".into(),
            bytes: 1,
            sha256: String::new(),
            mtime: 0,
        };
        assert!(dir_with_bytes.validate().is_err());
    }

    #[test]
    fn payload_limits_per_type() {
        assert_eq!(Message::Ping.max_payload(), 0);
        assert_eq!(
            Message::Text {
                id: String::new(),
                origin: String::new(),
                ts: 0,
                hash: String::new()
            }
            .max_payload(),
            MAX_TEXT
        );
        assert_eq!(
            Message::FileChunk {
                xfer_id: String::new(),
                offset: 0
            }
            .max_payload(),
            MAX_CHUNK
        );
    }
}
