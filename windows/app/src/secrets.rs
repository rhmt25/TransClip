//! Device id and group key, encrypted with DPAPI (user scope) in `secrets.bin`.

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use proto::group::GroupKey;
use proto::ids::{decode_hex_exact, DeviceId};
use serde::{Deserialize, Serialize};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB,
};
use zeroize::Zeroize;

/// Extra entropy so another program running as the same user cannot decrypt the file by
/// calling DPAPI with default parameters.
const ENTROPY: &[u8] = b"TransClip-v1 secrets";

/// What is stored.
pub struct Secrets {
    /// Stable id of this device.
    pub device_id: DeviceId,
    /// Group key; `None` until a group is created or joined.
    pub group_key: Option<GroupKey>,
}

#[derive(Serialize, Deserialize)]
struct Stored {
    device_id: String,
    group_key: Option<String>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn blob(data: &[u8]) -> Result<CRYPT_INTEGER_BLOB> {
    Ok(CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(data.len()).context("secret too large")?,
        pbData: data.as_ptr().cast_mut(),
    })
}

/// Copy a DPAPI output blob and free it.
///
/// SAFETY: `out` must have been filled by a successful DPAPI call (allocated with LocalAlloc).
unsafe fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
    let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
    let _ = LocalFree(Some(HLOCAL(out.pbData.cast())));
    bytes
}

/// Encrypt for the current user.
pub fn protect(plain: &[u8]) -> Result<Vec<u8>> {
    let input = blob(plain)?;
    let entropy = blob(ENTROPY)?;
    let mut out = CRYPT_INTEGER_BLOB::default();
    // SAFETY: both input blobs point to live slices for the duration of the call, and `out`
    // is a valid destination that DPAPI fills with a LocalAlloc'd buffer, handled by `take`.
    unsafe {
        CryptProtectData(
            &input,
            PCWSTR::null(),
            Some(&entropy),
            None,
            None,
            0,
            &mut out,
        )
        .context("CryptProtectData failed")?;
        Ok(take(out))
    }
}

/// Decrypt data produced by [`protect`] for the same user.
pub fn unprotect(cipher: &[u8]) -> Result<Vec<u8>> {
    let input = blob(cipher)?;
    let entropy = blob(ENTROPY)?;
    let mut out = CRYPT_INTEGER_BLOB::default();
    // SAFETY: as in `protect`.
    unsafe {
        CryptUnprotectData(&input, None, Some(&entropy), None, None, 0, &mut out)
            .context("CryptUnprotectData failed")?;
        Ok(take(out))
    }
}

impl Secrets {
    /// Read `path`; when it does not exist, create a new random device id and save it.
    pub fn load_or_create(path: &Path) -> Result<Self> {
        match std::fs::read(path) {
            Ok(cipher) => Self::decode(&cipher),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let s = Self {
                    device_id: DeviceId::random().map_err(|e| anyhow!("no randomness: {e}"))?,
                    group_key: None,
                };
                s.save(path)?;
                Ok(s)
            }
            Err(e) => Err(e).context("cannot read secrets.bin"),
        }
    }

    fn decode(cipher: &[u8]) -> Result<Self> {
        let mut plain = unprotect(cipher)?;
        let parsed: Result<Stored, _> = serde_json::from_slice(&plain);
        plain.zeroize();
        let stored = parsed.context("secrets.bin is corrupt")?;
        let device_id = DeviceId::from_hex(&stored.device_id)
            .map_err(|e| anyhow!("bad device id in secrets: {e}"))?;
        let group_key = match stored.group_key {
            Some(h) => Some(GroupKey(
                decode_hex_exact::<32>(&h).map_err(|e| anyhow!("bad group key in secrets: {e}"))?,
            )),
            None => None,
        };
        Ok(Self {
            device_id,
            group_key,
        })
    }

    /// Encrypt and write atomically.
    pub fn save(&self, path: &Path) -> Result<()> {
        let stored = Stored {
            device_id: self.device_id.to_hex(),
            group_key: self.group_key.as_ref().map(|k| hex(&k.0)),
        };
        let mut plain = serde_json::to_vec(&stored)?;
        let cipher = protect(&plain);
        plain.zeroize();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("bin.tmp");
        std::fs::write(&tmp, cipher?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dpapi_round_trip_and_ciphertext_is_opaque() {
        let secret = b"super secret group key material";
        let cipher = protect(secret).unwrap();
        assert!(!cipher.windows(secret.len()).any(|w| w == secret));
        assert_eq!(unprotect(&cipher).unwrap(), secret);
    }

    #[test]
    fn tampered_ciphertext_is_rejected() {
        let mut cipher = protect(b"data").unwrap();
        let last = cipher.len() - 1;
        cipher[last] ^= 1;
        assert!(unprotect(&cipher).is_err());
    }

    #[test]
    fn file_is_created_once_and_does_not_contain_plain_text() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("secrets.bin");
        let first = Secrets::load_or_create(&p).unwrap();
        let again = Secrets::load_or_create(&p).unwrap();
        assert_eq!(first.device_id, again.device_id);
        assert!(first.group_key.is_none());

        let with_key = Secrets {
            device_id: first.device_id,
            group_key: Some(GroupKey([0xab; 32])),
        };
        with_key.save(&p).unwrap();
        let raw = std::fs::read(&p).unwrap();
        let text = String::from_utf8_lossy(&raw);
        assert!(!text.contains(&first.device_id.to_hex()));
        assert!(!text.contains("abab"));
        let back = Secrets::load_or_create(&p).unwrap();
        assert_eq!(back.group_key.map(|k| k.0), Some([0xab; 32]));
    }
}
