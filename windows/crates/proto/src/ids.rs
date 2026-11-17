//! Identifiers and the small encodings the protocol uses (PROTOCOL.md section 1).

use crate::error::{ProtoError, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;

/// 16-byte random device identifier, shown as 32 lowercase hex characters.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceId(pub [u8; 16]);

impl DeviceId {
    /// Create a fresh random identifier.
    pub fn random() -> Result<Self> {
        let mut b = [0u8; 16];
        fill_random(&mut b)?;
        Ok(Self(b))
    }

    /// Lowercase hex form (32 characters).
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Parse the 32-character lowercase hex form.
    pub fn from_hex(s: &str) -> Result<Self> {
        let b = decode_hex_exact::<16>(s)?;
        Ok(Self(b))
    }
}

impl std::fmt::Debug for DeviceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DeviceId({})", self.to_hex())
    }
}

/// 8-byte group identifier derived from the group key, shown as 16 hex characters.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct GroupId(pub [u8; 8]);

impl GroupId {
    /// Lowercase hex form (16 characters).
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Parse the 16-character lowercase hex form.
    pub fn from_hex(s: &str) -> Result<Self> {
        Ok(Self(decode_hex_exact::<8>(s)?))
    }
}

impl std::fmt::Debug for GroupId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "GroupId({})", self.to_hex())
    }
}

/// Fill `buf` from the operating system RNG.
pub fn fill_random(buf: &mut [u8]) -> Result<()> {
    getrandom::fill(buf).map_err(|e| ProtoError::Io(std::io::Error::other(e.to_string())))
}

/// Decode exactly `N` bytes from lowercase hex of length `2 * N`.
pub fn decode_hex_exact<const N: usize>(s: &str) -> Result<[u8; N]> {
    if s.len() != N * 2 || !is_lower_hex(s) {
        return Err(ProtoError::BadHeader(format!(
            "expected {} lowercase hex chars",
            N * 2
        )));
    }
    let mut out = [0u8; N];
    hex::decode_to_slice(s, &mut out)
        .map_err(|_| ProtoError::BadHeader("invalid hex".to_string()))?;
    Ok(out)
}

/// True when every character is `0-9` or `a-f`.
pub fn is_lower_hex(s: &str) -> bool {
    s.bytes()
        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

/// True for a 64-character lowercase hex SHA-256 digest.
pub fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && is_lower_hex(s)
}

/// Encode as base64url without padding.
pub fn b64url_encode(data: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(data)
}

/// Decode base64url without padding. Padding or other alphabets are rejected.
pub fn b64url_decode(s: &str) -> Result<Vec<u8>> {
    URL_SAFE_NO_PAD
        .decode(s)
        .map_err(|_| ProtoError::BadHeader("invalid base64url".to_string()))
}

/// Decode base64url into exactly `N` bytes.
pub fn b64url_decode_exact<const N: usize>(s: &str) -> Result<[u8; N]> {
    let v = b64url_decode(s)?;
    v.try_into()
        .map_err(|_| ProtoError::BadHeader(format!("expected {N} bytes")))
}

/// True for a lowercase UUIDv4 string (`8-4-4-4-12`).
pub fn is_uuid_v4(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 36 {
        return false;
    }
    for (i, &c) in b.iter().enumerate() {
        let ok = match i {
            8 | 13 | 18 | 23 => c == b'-',
            14 => c == b'4',
            19 => matches!(c, b'8' | b'9' | b'a' | b'b'),
            _ => c.is_ascii_digit() || (b'a'..=b'f').contains(&c),
        };
        if !ok {
            return false;
        }
    }
    true
}

/// Generate a random UUIDv4 string.
pub fn new_uuid_v4() -> Result<String> {
    let mut b = [0u8; 16];
    fill_random(&mut b)?;
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h = hex::encode(b);
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_id_hex_round_trip() {
        let id = DeviceId([0xab; 16]);
        assert_eq!(id.to_hex(), "ab".repeat(16));
        assert_eq!(DeviceId::from_hex(&id.to_hex()).unwrap(), id);
    }

    #[test]
    fn hex_rejects_uppercase_and_wrong_length() {
        assert!(DeviceId::from_hex(&"AB".repeat(16)).is_err());
        assert!(DeviceId::from_hex("abcd").is_err());
        assert!(GroupId::from_hex("zz00000000000000").is_err());
    }

    #[test]
    fn device_id_orders_lexicographically() {
        let mut a = [0u8; 16];
        let mut b = [0u8; 16];
        a[15] = 1;
        b[0] = 1;
        assert!(DeviceId(a) < DeviceId(b));
    }

    #[test]
    fn b64url_has_no_padding_and_round_trips() {
        let s = b64url_encode(&[0xfb, 0xff, 0xfe, 0x01]);
        assert!(!s.contains('=') && !s.contains('+') && !s.contains('/'));
        assert_eq!(b64url_decode(&s).unwrap(), vec![0xfb, 0xff, 0xfe, 0x01]);
        assert!(b64url_decode("AA==").is_err());
        assert!(b64url_decode("a+b/").is_err());
    }

    #[test]
    fn uuid_validation() {
        let u = new_uuid_v4().unwrap();
        assert!(is_uuid_v4(&u), "{u}");
        assert!(!is_uuid_v4("00000000-0000-0000-0000-000000000000"));
        assert!(!is_uuid_v4(&u.to_uppercase()));
        assert!(!is_uuid_v4("nope"));
    }
}
