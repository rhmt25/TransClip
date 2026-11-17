//! Pairing URI shown as a QR code or a copyable text code (PROTOCOL.md section 3.1).
//!
//! ```text
//! transclip://pair?v=1&k=<b64url GK>&g=<name>&d=<inviter device id>&a=<ip:port,...>&x=<expiry>
//! ```

use crate::error::{ProtoError, Result};
use crate::group::GroupKey;
use crate::ids::{b64url_decode_exact, b64url_encode, DeviceId};

const PREFIX: &str = "transclip://pair?";

/// Everything a new device learns from a pairing code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingInfo {
    /// The shared group key.
    pub gk: GroupKey,
    /// Human readable group name.
    pub group_name: String,
    /// Device that created the code.
    pub inviter: DeviceId,
    /// Addresses (`ip:port`) the inviter had when the code was made. May be empty.
    pub addrs: Vec<String>,
    /// Unix time in seconds after which the code should be refused.
    pub expires: u64,
}

impl PairingInfo {
    /// Build the URI.
    pub fn to_uri(&self) -> String {
        let mut uri = format!(
            "{PREFIX}v=1&k={}&g={}&d={}",
            b64url_encode(&self.gk.0),
            percent_encode(&self.group_name),
            self.inviter.to_hex()
        );
        if !self.addrs.is_empty() {
            uri.push_str("&a=");
            uri.push_str(&percent_encode(&self.addrs.join(",")));
        }
        uri.push_str(&format!("&x={}", self.expires));
        uri
    }

    /// Parse a URI. Unknown parameters are ignored; missing required ones are errors.
    pub fn parse(uri: &str) -> Result<Self> {
        let query = uri
            .trim()
            .strip_prefix(PREFIX)
            .ok_or_else(|| bad("not a transclip pairing code"))?;
        let (mut v, mut k, mut g, mut d, mut a, mut x) = (None, None, None, None, None, None);
        for pair in query.split('&') {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            match key {
                "v" => v = Some(value),
                "k" => k = Some(value),
                "g" => g = Some(percent_decode(value)?),
                "d" => d = Some(value),
                "a" => a = Some(percent_decode(value)?),
                "x" => x = Some(value),
                _ => {}
            }
        }
        let version: u64 = v
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| bad("missing version"))?;
        if version != 1 {
            return Err(ProtoError::Version(version));
        }
        let gk = GroupKey(b64url_decode_exact::<32>(
            k.ok_or_else(|| bad("missing key"))?,
        )?);
        let group_name = g.ok_or_else(|| bad("missing group name"))?;
        let inviter = DeviceId::from_hex(d.ok_or_else(|| bad("missing inviter"))?)?;
        let expires: u64 = x
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| bad("missing expiry"))?;
        let addrs = a
            .map(|s| {
                s.split(',')
                    .filter(|p| !p.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self {
            gk,
            group_name,
            inviter,
            addrs,
            expires,
        })
    }

    /// True when the code is past its expiry at `now_secs` (Unix seconds).
    pub fn is_expired(&self, now_secs: u64) -> bool {
        now_secs > self.expires
    }
}

fn bad(msg: &str) -> ProtoError {
    ProtoError::BadHeader(msg.to_string())
}

/// Percent-encode everything except RFC 3986 unreserved characters.
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Decode percent-encoding into a UTF-8 string.
pub fn percent_decode(s: &str) -> Result<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
                .ok_or_else(|| bad("bad percent-encoding"))?;
            out.push(hex);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| bad("not UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(name: &str, addrs: &[&str]) -> PairingInfo {
        PairingInfo {
            gk: GroupKey([5; 32]),
            group_name: name.to_string(),
            inviter: DeviceId([6; 16]),
            addrs: addrs.iter().map(|s| s.to_string()).collect(),
            expires: 1_800_000_000,
        }
    }

    #[test]
    fn round_trip_with_awkward_names() {
        for name in [
            "Rumah",
            "Kantor & Rumah",
            "a=b?c#d%e",
            "\u{65e5}\u{672c}\u{8a9e} \u{1f600}",
            "",
        ] {
            let p = sample(name, &["192.168.1.5:47820", "[fe80::1]:47820"]);
            assert_eq!(PairingInfo::parse(&p.to_uri()).unwrap(), p, "{name:?}");
        }
    }

    #[test]
    fn addresses_are_optional() {
        let p = sample("x", &[]);
        let uri = p.to_uri();
        assert!(!uri.contains("&a="));
        assert_eq!(PairingInfo::parse(&uri).unwrap(), p);
    }

    #[test]
    fn unknown_parameters_are_ignored_and_whitespace_trimmed() {
        let uri = format!("  {}&future=1\n", sample("x", &[]).to_uri());
        assert!(PairingInfo::parse(&uri).is_ok());
    }

    #[test]
    fn rejects_bad_input() {
        let good = sample("x", &[]).to_uri();
        assert!(PairingInfo::parse("https://example.com").is_err());
        assert!(PairingInfo::parse(&good.replace("v=1", "v=2")).is_err());
        assert!(PairingInfo::parse(&good.replace("&x=1800000000", "")).is_err());
        assert!(PairingInfo::parse(&good.replace("k=", "k=AAAA")).is_err());
        assert!(PairingInfo::parse(&good.replace("&g=x", "&g=%ZZ")).is_err());
        assert!(PairingInfo::parse(&good.replace("&g=x", "&g=%FF")).is_err());
    }

    #[test]
    fn expiry() {
        let p = sample("x", &[]);
        assert!(!p.is_expired(1_800_000_000));
        assert!(p.is_expired(1_800_000_001));
    }

    #[test]
    fn percent_round_trip() {
        for s in ["", "abc", "a b", "\u{e9}\u{1f600}", "100%"] {
            assert_eq!(percent_decode(&percent_encode(s)).unwrap(), s);
        }
    }
}
