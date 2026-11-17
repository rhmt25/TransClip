//! Frame codec (PROTOCOL.md section 8).
//!
//! ```text
//! [u32 BE header length N][N bytes JSON header][size bytes payload]
//! ```

use crate::error::{ProtoError, Result};
use crate::msg::{Message, KNOWN_TYPES};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Largest allowed JSON header.
pub const MAX_HEADER: usize = 65_536;

/// One decoded frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    /// The message carried by the header.
    pub message: Message,
    /// The payload bytes (empty when the message has none).
    pub payload: Vec<u8>,
}

/// Serialize a message and payload into one frame.
pub fn encode_frame(message: &Message, payload: &[u8]) -> Result<Vec<u8>> {
    if matches!(message, Message::Unknown { .. }) {
        return Err(ProtoError::BadHeader(
            "cannot encode an unknown message".into(),
        ));
    }
    if payload.len() > message.max_payload() {
        return Err(ProtoError::Limit(format!(
            "payload of {} bytes exceeds the limit for {}",
            payload.len(),
            message.type_name()
        )));
    }
    let mut value = serde_json::to_value(message)
        .map_err(|e| ProtoError::BadHeader(format!("cannot serialize message: {e}")))?;
    let Value::Object(map) = &mut value else {
        return Err(ProtoError::BadHeader("message is not a JSON object".into()));
    };
    map.insert("size".into(), Value::from(payload.len() as u64));
    let json = serde_json::to_vec(&value)
        .map_err(|e| ProtoError::BadHeader(format!("cannot serialize header: {e}")))?;
    if json.is_empty() || json.len() > MAX_HEADER {
        return Err(ProtoError::Limit("header too large".into()));
    }
    let mut out = Vec::with_capacity(4 + json.len() + payload.len());
    out.extend_from_slice(&(json.len() as u32).to_be_bytes());
    out.extend_from_slice(&json);
    out.extend_from_slice(payload);
    Ok(out)
}

/// Parse a header (without length prefix) into a message and its declared payload size.
pub fn decode_header(header: &[u8]) -> Result<(Message, usize)> {
    let value: Value = serde_json::from_slice(header)
        .map_err(|e| ProtoError::BadHeader(format!("header is not JSON: {e}")))?;
    let Value::Object(map) = &value else {
        return Err(ProtoError::BadHeader("header is not a JSON object".into()));
    };
    let t = map
        .get("t")
        .and_then(Value::as_str)
        .ok_or_else(|| ProtoError::BadHeader("missing \"t\"".into()))?;
    let size = map
        .get("size")
        .and_then(Value::as_u64)
        .ok_or_else(|| ProtoError::BadHeader("missing or invalid \"size\"".into()))?;
    let message = if KNOWN_TYPES.contains(&t) {
        serde_json::from_value::<Message>(value.clone())
            .map_err(|e| ProtoError::BadHeader(format!("invalid {t} message: {e}")))?
    } else {
        Message::Unknown { t: t.to_string() }
    };
    let size = usize::try_from(size).map_err(|_| ProtoError::Limit("payload size".into()))?;
    if size > message.max_payload() {
        return Err(ProtoError::Limit(format!(
            "{} payload of {size} bytes exceeds the limit",
            message.type_name()
        )));
    }
    Ok((message, size))
}

/// Check a header length prefix.
pub fn check_header_len(n: usize) -> Result<()> {
    if n == 0 || n > MAX_HEADER {
        Err(ProtoError::Limit(format!("header length {n}")))
    } else {
        Ok(())
    }
}

/// Incremental decoder: feed it bytes in any chunking, get whole frames out.
#[derive(Debug, Default)]
pub struct FrameDecoder {
    buf: Vec<u8>,
}

impl FrameDecoder {
    /// Create an empty decoder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append `data` and return every frame that is now complete.
    pub fn push(&mut self, data: &[u8]) -> Result<Vec<Frame>> {
        self.buf.extend_from_slice(data);
        let mut frames = Vec::new();
        let mut offset = 0;
        while let Some((frame, used)) = Self::parse_at(&self.buf[offset..])? {
            frames.push(frame);
            offset += used;
        }
        self.buf.drain(..offset);
        Ok(frames)
    }

    fn parse_at(buf: &[u8]) -> Result<Option<(Frame, usize)>> {
        let Some(prefix) = buf.get(..4) else {
            return Ok(None);
        };
        let n = u32::from_be_bytes([prefix[0], prefix[1], prefix[2], prefix[3]]) as usize;
        check_header_len(n)?;
        let Some(header) = buf.get(4..4 + n) else {
            return Ok(None);
        };
        let (message, size) = decode_header(header)?;
        let end = 4 + n + size;
        let Some(payload) = buf.get(4 + n..end) else {
            return Ok(None);
        };
        Ok(Some((
            Frame {
                message,
                payload: payload.to_vec(),
            },
            end,
        )))
    }
}

/// Something frames can be read from, one exact read at a time.
#[allow(async_fn_in_trait)]
pub trait ByteSource {
    /// Fill `buf` completely. A closed peer is reported as [`ProtoError::Closed`].
    async fn read_exact_bytes(&mut self, buf: &mut [u8]) -> Result<()>;
}

/// Plaintext reader used during the handshake.
pub struct PlainReader<R>(pub R);

impl<R: AsyncRead + Unpin> ByteSource for PlainReader<R> {
    async fn read_exact_bytes(&mut self, buf: &mut [u8]) -> Result<()> {
        match self.0.read_exact(buf).await {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Err(ProtoError::Closed),
            Err(e) => Err(e.into()),
        }
    }
}

/// Read one frame from any [`ByteSource`].
pub async fn read_frame<S: ByteSource>(src: &mut S) -> Result<Frame> {
    let mut prefix = [0u8; 4];
    src.read_exact_bytes(&mut prefix).await?;
    let n = u32::from_be_bytes(prefix) as usize;
    check_header_len(n)?;
    let mut header = vec![0u8; n];
    src.read_exact_bytes(&mut header).await?;
    let (message, size) = decode_header(&header)?;
    let mut payload = vec![0u8; size];
    src.read_exact_bytes(&mut payload).await?;
    Ok(Frame { message, payload })
}

/// Write one plaintext frame and flush.
pub async fn write_plain_frame<W: AsyncWrite + Unpin>(
    w: &mut W,
    message: &Message,
    payload: &[u8],
) -> Result<()> {
    let bytes = encode_frame(message, payload)?;
    w.write_all(&bytes).await?;
    w.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::msg::{MAX_CHUNK, MAX_TEXT};
    use proptest::prelude::*;

    const UUID: &str = "11111111-1111-4111-8111-111111111111";

    fn text(len: usize) -> (Message, Vec<u8>) {
        (
            Message::Text {
                id: UUID.into(),
                origin: "ab".repeat(16),
                ts: 9,
                hash: "cd".repeat(32),
            },
            vec![b'x'; len],
        )
    }

    #[test]
    fn encode_then_decode() {
        let (m, p) = text(5);
        let bytes = encode_frame(&m, &p).unwrap();
        let frames = FrameDecoder::new().push(&bytes).unwrap();
        assert_eq!(
            frames,
            vec![Frame {
                message: m,
                payload: p
            }]
        );
    }

    #[test]
    fn decoder_handles_one_byte_at_a_time() {
        let (m, p) = text(300);
        let mut bytes = encode_frame(&m, &p).unwrap();
        bytes.extend(encode_frame(&Message::Ping, &[]).unwrap());
        let mut dec = FrameDecoder::new();
        let mut got = Vec::new();
        for b in &bytes {
            got.extend(dec.push(&[*b]).unwrap());
        }
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].payload.len(), 300);
        assert_eq!(got[1].message, Message::Ping);
    }

    #[test]
    fn decoder_handles_many_frames_in_one_buffer() {
        let mut bytes = Vec::new();
        for _ in 0..10 {
            bytes.extend(encode_frame(&Message::Pong, &[]).unwrap());
        }
        assert_eq!(FrameDecoder::new().push(&bytes).unwrap().len(), 10);
    }

    #[test]
    fn rejects_header_length_zero_and_too_big() {
        assert!(matches!(
            FrameDecoder::new().push(&[0, 0, 0, 0]),
            Err(ProtoError::Limit(_))
        ));
        let too_big = (MAX_HEADER as u32 + 1).to_be_bytes();
        assert!(matches!(
            FrameDecoder::new().push(&too_big),
            Err(ProtoError::Limit(_))
        ));
        let max = (MAX_HEADER as u32).to_be_bytes();
        assert!(FrameDecoder::new().push(&max).unwrap().is_empty());
    }

    #[test]
    fn rejects_oversized_payload_before_it_arrives() {
        let header = format!(
            r#"{{"t":"file_chunk","xfer_id":"{UUID}","offset":0,"size":{}}}"#,
            MAX_CHUNK + 1
        );
        let mut bytes = (header.len() as u32).to_be_bytes().to_vec();
        bytes.extend(header.as_bytes());
        assert!(matches!(
            FrameDecoder::new().push(&bytes),
            Err(ProtoError::Limit(_))
        ));
    }

    #[test]
    fn encode_rejects_oversized_payload() {
        let (m, _) = text(0);
        assert!(encode_frame(&m, &vec![0u8; MAX_TEXT + 1]).is_err());
        assert!(encode_frame(&Message::Ping, &[1]).is_err());
    }

    #[test]
    fn rejects_bad_json_and_missing_fields() {
        for header in [
            &b"{"[..],
            b"[]",
            br#"{"size":0}"#,
            br#"{"t":"ping"}"#,
            br#"{"t":"ping","size":-1}"#,
        ] {
            let mut bytes = (header.len() as u32).to_be_bytes().to_vec();
            bytes.extend(header);
            assert!(
                matches!(
                    FrameDecoder::new().push(&bytes),
                    Err(ProtoError::BadHeader(_))
                ),
                "{}",
                String::from_utf8_lossy(header)
            );
        }
    }

    #[test]
    fn rejects_known_type_with_wrong_fields() {
        let header = br#"{"t":"pull","size":0}"#;
        let mut bytes = (header.len() as u32).to_be_bytes().to_vec();
        bytes.extend(header);
        assert!(matches!(
            FrameDecoder::new().push(&bytes),
            Err(ProtoError::BadHeader(_))
        ));
    }

    #[test]
    fn unknown_types_decode_as_unknown_with_their_payload() {
        let header = br#"{"t":"from_the_future","size":3,"x":1}"#;
        let mut bytes = (header.len() as u32).to_be_bytes().to_vec();
        bytes.extend(header);
        bytes.extend(b"abc");
        let frames = FrameDecoder::new().push(&bytes).unwrap();
        assert_eq!(
            frames[0].message,
            Message::Unknown {
                t: "from_the_future".into()
            }
        );
        assert_eq!(frames[0].payload, b"abc");
    }

    #[tokio::test]
    async fn async_read_and_write() {
        let (a, b) = tokio::io::duplex(1024);
        let (mut r, _w1) = tokio::io::split(a);
        let (_r2, mut w) = tokio::io::split(b);
        let (m, p) = text(10);
        write_plain_frame(&mut w, &m, &p).await.unwrap();
        let f = read_frame(&mut PlainReader(&mut r)).await.unwrap();
        assert_eq!(f.message, m);
        drop(w);
        drop(_r2);
        assert!(matches!(
            read_frame(&mut PlainReader(&mut r)).await,
            Err(ProtoError::Closed)
        ));
    }

    proptest! {
        #[test]
        fn encode_decode_identity(payload in proptest::collection::vec(any::<u8>(), 0..2048), ts in any::<u64>(), split in 1usize..64) {
            let m = Message::Text { id: UUID.into(), origin: "ab".repeat(16), ts, hash: "cd".repeat(32) };
            let bytes = encode_frame(&m, &payload).unwrap();
            let mut dec = FrameDecoder::new();
            let mut got = Vec::new();
            for chunk in bytes.chunks(split) {
                got.extend(dec.push(chunk).unwrap());
            }
            prop_assert_eq!(got.len(), 1);
            prop_assert_eq!(&got[0].message, &m);
            prop_assert_eq!(&got[0].payload, &payload);
        }

        #[test]
        fn random_bytes_never_panic(data in proptest::collection::vec(any::<u8>(), 0..512)) {
            let _ = FrameDecoder::new().push(&data);
        }
    }
}
