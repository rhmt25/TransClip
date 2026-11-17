//! Encrypted record layer (PROTOCOL.md section 7).
//!
//! Every record is `[u32 BE length L][L bytes ChaCha20-Poly1305 ciphertext]` with
//! `17 <= L <= 65552`. The nonce is `0x00000000 || counter (u64 BE)`, the counter is kept
//! per direction and starts at 0. Plaintext of consecutive records forms one byte stream.

use crate::error::{ProtoError, Result};
use crate::frame::{encode_frame, read_frame, ByteSource, Frame};
use crate::msg::Message;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use zeroize::Zeroize;

/// Largest plaintext carried by one record.
pub const MAX_RECORD_PLAINTEXT: usize = 65_536;
/// Authentication tag size.
pub const TAG_LEN: usize = 16;
/// Smallest valid record length (one byte of plaintext plus the tag).
pub const MIN_RECORD_LEN: usize = 1 + TAG_LEN;
/// Largest valid record length.
pub const MAX_RECORD_LEN: usize = MAX_RECORD_PLAINTEXT + TAG_LEN;
/// Counters at or above this value end the connection.
pub const COUNTER_LIMIT: u64 = 1 << 32;

fn nonce(counter: u64) -> Nonce {
    let mut n = [0u8; 12];
    n[4..].copy_from_slice(&counter.to_be_bytes());
    Nonce::from(n)
}

/// Encrypt one record body (without the length prefix).
pub fn seal(key: &[u8; 32], counter: u64, plaintext: &[u8]) -> Result<Vec<u8>> {
    if plaintext.is_empty() || plaintext.len() > MAX_RECORD_PLAINTEXT {
        return Err(ProtoError::Limit("record plaintext size".into()));
    }
    if counter >= COUNTER_LIMIT {
        return Err(ProtoError::Limit("record counter".into()));
    }
    let cipher = ChaCha20Poly1305::new(&Key::from(*key));
    cipher
        .encrypt(&nonce(counter), plaintext)
        .map_err(|_| ProtoError::Auth)
}

/// Decrypt one record body (without the length prefix).
pub fn open(key: &[u8; 32], counter: u64, ciphertext: &[u8]) -> Result<Vec<u8>> {
    if !(MIN_RECORD_LEN..=MAX_RECORD_LEN).contains(&ciphertext.len()) {
        return Err(ProtoError::Limit("record length".into()));
    }
    if counter >= COUNTER_LIMIT {
        return Err(ProtoError::Limit("record counter".into()));
    }
    let cipher = ChaCha20Poly1305::new(&Key::from(*key));
    cipher
        .decrypt(&nonce(counter), ciphertext)
        .map_err(|_| ProtoError::Auth)
}

/// Sending half of an encrypted connection.
pub struct SecureWriter<W> {
    w: W,
    key: [u8; 32],
    counter: u64,
}

impl<W: AsyncWrite + Unpin> SecureWriter<W> {
    pub(crate) fn new(w: W, key: [u8; 32]) -> Self {
        Self { w, key, counter: 0 }
    }

    /// Encrypt and send `data` as one or more records. Does not flush.
    pub async fn write_bytes(&mut self, data: &[u8]) -> Result<()> {
        for chunk in data.chunks(MAX_RECORD_PLAINTEXT) {
            let body = seal(&self.key, self.counter, chunk)?;
            self.counter += 1;
            let mut out = Vec::with_capacity(4 + body.len());
            out.extend_from_slice(&(body.len() as u32).to_be_bytes());
            out.extend_from_slice(&body);
            self.w.write_all(&out).await?;
        }
        Ok(())
    }

    /// Send one frame and flush so the peer sees it immediately.
    pub async fn write_frame(&mut self, message: &Message, payload: &[u8]) -> Result<()> {
        let bytes = encode_frame(message, payload)?;
        self.write_bytes(&bytes).await?;
        self.w.flush().await?;
        Ok(())
    }

    /// Number of records sent so far.
    pub fn records_sent(&self) -> u64 {
        self.counter
    }
}

impl<W> Drop for SecureWriter<W> {
    fn drop(&mut self) {
        self.key.zeroize();
    }
}

/// Receiving half of an encrypted connection.
pub struct SecureReader<R> {
    r: R,
    key: [u8; 32],
    counter: u64,
    pending: Vec<u8>,
    pos: usize,
}

impl<R: AsyncRead + Unpin> SecureReader<R> {
    pub(crate) fn new(r: R, key: [u8; 32]) -> Self {
        Self {
            r,
            key,
            counter: 0,
            pending: Vec::new(),
            pos: 0,
        }
    }

    async fn next_record(&mut self) -> Result<()> {
        let mut prefix = [0u8; 4];
        read_all(&mut self.r, &mut prefix).await?;
        let len = u32::from_be_bytes(prefix) as usize;
        if !(MIN_RECORD_LEN..=MAX_RECORD_LEN).contains(&len) {
            return Err(ProtoError::Limit(format!("record length {len}")));
        }
        let mut body = vec![0u8; len];
        read_all(&mut self.r, &mut body).await?;
        let plain = open(&self.key, self.counter, &body)?;
        self.counter += 1;
        self.pending.drain(..self.pos);
        self.pos = 0;
        self.pending.extend_from_slice(&plain);
        Ok(())
    }

    /// Read the next frame from the decrypted stream.
    pub async fn read_frame(&mut self) -> Result<Frame> {
        read_frame(self).await
    }

    /// Number of records received so far.
    pub fn records_received(&self) -> u64 {
        self.counter
    }
}

impl<R: AsyncRead + Unpin> ByteSource for SecureReader<R> {
    async fn read_exact_bytes(&mut self, buf: &mut [u8]) -> Result<()> {
        while self.pending.len() - self.pos < buf.len() {
            self.next_record().await?;
        }
        buf.copy_from_slice(&self.pending[self.pos..self.pos + buf.len()]);
        self.pos += buf.len();
        Ok(())
    }
}

impl<R> Drop for SecureReader<R> {
    fn drop(&mut self) {
        self.key.zeroize();
        self.pending.zeroize();
    }
}

async fn read_all<R: AsyncRead + Unpin>(r: &mut R, buf: &mut [u8]) -> Result<()> {
    match r.read_exact(buf).await {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Err(ProtoError::Closed),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{duplex, split};

    const KEY: [u8; 32] = [7u8; 32];
    const UUID: &str = "11111111-1111-4111-8111-111111111111";

    fn text(len: usize) -> (Message, Vec<u8>) {
        (
            Message::Text {
                id: UUID.into(),
                origin: "ab".repeat(16),
                ts: 1,
                hash: "cd".repeat(32),
            },
            (0..len).map(|i| (i % 251) as u8).collect(),
        )
    }

    #[test]
    fn seal_open_round_trip() {
        let ct = seal(&KEY, 0, b"hello").unwrap();
        assert_eq!(ct.len(), 5 + TAG_LEN);
        assert_eq!(open(&KEY, 0, &ct).unwrap(), b"hello");
    }

    #[test]
    fn wrong_counter_key_or_flipped_bit_fails() {
        let ct = seal(&KEY, 3, b"hello").unwrap();
        assert!(matches!(open(&KEY, 4, &ct), Err(ProtoError::Auth)));
        assert!(matches!(open(&[8u8; 32], 3, &ct), Err(ProtoError::Auth)));
        for i in 0..ct.len() {
            let mut bad = ct.clone();
            bad[i] ^= 1;
            assert!(
                matches!(open(&KEY, 3, &bad), Err(ProtoError::Auth)),
                "byte {i}"
            );
        }
    }

    #[test]
    fn length_and_counter_limits() {
        assert!(seal(&KEY, 0, &[]).is_err());
        assert!(seal(&KEY, 0, &vec![0u8; MAX_RECORD_PLAINTEXT + 1]).is_err());
        assert!(seal(&KEY, COUNTER_LIMIT, b"x").is_err());
        assert!(open(&KEY, 0, &[0u8; MIN_RECORD_LEN - 1]).is_err());
        assert!(open(&KEY, 0, &vec![0u8; MAX_RECORD_LEN + 1]).is_err());
    }

    #[tokio::test]
    async fn frames_round_trip_with_increasing_counters() {
        let (a, b) = duplex(1 << 20);
        let (_ar, aw) = split(a);
        let (br, _bw) = split(b);
        let mut w = SecureWriter::new(aw, KEY);
        let mut r = SecureReader::new(br, KEY);
        for len in [0usize, 1, 100, 65_000, 65_536, 200_000] {
            let (m, p) = text(len);
            w.write_frame(&m, &p).await.unwrap();
            let f = r.read_frame().await.unwrap();
            assert_eq!(f.message, m);
            assert_eq!(f.payload, p, "len {len}");
        }
        assert_eq!(w.records_sent(), r.records_received());
    }

    #[tokio::test]
    async fn tampered_stream_is_rejected() {
        let (a, b) = duplex(1 << 16);
        let (_ar, mut aw) = split(a);
        let (br, _bw) = split(b);
        let mut r = SecureReader::new(br, KEY);
        let mut body = seal(&KEY, 0, b"hello").unwrap();
        body[0] ^= 0x80;
        aw.write_all(&(body.len() as u32).to_be_bytes())
            .await
            .unwrap();
        aw.write_all(&body).await.unwrap();
        assert!(matches!(r.read_frame().await, Err(ProtoError::Auth)));
    }

    #[tokio::test]
    async fn bad_record_length_is_rejected() {
        let (a, b) = duplex(64);
        let (_ar, mut aw) = split(a);
        let (br, _bw) = split(b);
        let mut r = SecureReader::new(br, KEY);
        aw.write_all(&((MAX_RECORD_LEN as u32) + 1).to_be_bytes())
            .await
            .unwrap();
        assert!(matches!(r.read_frame().await, Err(ProtoError::Limit(_))));
    }

    #[tokio::test]
    async fn truncated_record_reports_closed() {
        let (a, b) = duplex(64);
        let (_ar, mut aw) = split(a);
        let (br, _bw) = split(b);
        let mut r = SecureReader::new(br, KEY);
        aw.write_all(&40u32.to_be_bytes()).await.unwrap();
        aw.write_all(&[1, 2, 3]).await.unwrap();
        drop(aw);
        drop(_ar);
        assert!(matches!(r.read_frame().await, Err(ProtoError::Closed)));
    }
}
