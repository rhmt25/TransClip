//! Authenticated key exchange (PROTOCOL.md section 6).
//!
//! ```text
//! I -> R  hs1 {v, gid, eph_i, n_i}
//! R -> I  hs2 {v, eph_r, n_r, mac_r}
//! I -> R  hs3 {mac_i}
//! ```
//!
//! Both sides then speak the encrypted record layer. The group key is mixed into the key
//! derivation and into the MACs, so only holders of the group key can complete the handshake.

use crate::error::{ProtoError, Result};
use crate::frame::{read_frame, write_plain_frame, PlainReader};
use crate::group::GroupKey;
use crate::ids::{b64url_decode_exact, b64url_encode, fill_random, GroupId};
use crate::msg::{Message, VERSION};
use crate::record::{SecureReader, SecureWriter};
use hkdf::Hkdf;
use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};
use std::time::Duration;
use subtle::ConstantTimeEq;
use tokio::io::{split, AsyncRead, AsyncWrite, ReadHalf, WriteHalf};
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Whole-handshake deadline.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

const TRANSCRIPT_DOMAIN: &[u8] = b"TransClip-v1 transcript";
const KEYS_INFO: &[u8] = b"TransClip-v1 keys";

/// Everything needed to derive the session keys. All fields are raw bytes.
pub struct HandshakeInputs<'a> {
    /// The shared group key.
    pub gk: &'a GroupKey,
    /// Group id both sides agreed on.
    pub gid: &'a GroupId,
    /// Initiator ephemeral public key.
    pub eph_i: &'a [u8; 32],
    /// Initiator nonce.
    pub n_i: &'a [u8; 16],
    /// Responder ephemeral public key.
    pub eph_r: &'a [u8; 32],
    /// Responder nonce.
    pub n_r: &'a [u8; 16],
    /// X25519 shared secret.
    pub dh: &'a [u8; 32],
}

/// Keys and MAC tags derived from a handshake.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct Derived {
    /// Transcript hash.
    pub th: [u8; 32],
    /// AEAD key for initiator-to-responder records.
    pub k_i2r: [u8; 32],
    /// AEAD key for responder-to-initiator records.
    pub k_r2i: [u8; 32],
    /// Tag the responder sends in `hs2`.
    pub mac_r: [u8; 32],
    /// Tag the initiator sends in `hs3`.
    pub mac_i: [u8; 32],
}

/// Hash of everything both sides have seen in the clear.
pub fn transcript_hash(
    gid: &GroupId,
    eph_i: &[u8; 32],
    n_i: &[u8; 16],
    eph_r: &[u8; 32],
    n_r: &[u8; 16],
) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(TRANSCRIPT_DOMAIN);
    h.update(gid.0);
    h.update(eph_i);
    h.update(n_i);
    h.update(eph_r);
    h.update(n_r);
    h.finalize().into()
}

fn hmac_tag(key: &[u8], label: &[u8], th: &[u8; 32]) -> Result<[u8; 32]> {
    // HMAC accepts any key length, so this cannot fail in practice. Failing closed (instead
    // of returning a constant tag) means a bug here can never weaken authentication.
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(key).map_err(|_| ProtoError::Auth)?;
    mac.update(label);
    mac.update(th);
    Ok(mac.finalize().into_bytes().into())
}

/// Derive the session keys and MAC tags (PROTOCOL.md section 6.1).
pub fn derive(inp: &HandshakeInputs<'_>) -> Result<Derived> {
    let th = transcript_hash(inp.gid, inp.eph_i, inp.n_i, inp.eph_r, inp.n_r);
    let mut ikm = [0u8; 64];
    ikm[..32].copy_from_slice(inp.dh);
    ikm[32..].copy_from_slice(&inp.gk.0);
    let hk = Hkdf::<Sha256>::new(Some(&th), &ikm);
    ikm.zeroize();
    let mut okm = [0u8; 128];
    // 128 bytes is far below the HKDF-SHA256 limit, so this only fails on a bug; fail closed.
    hk.expand(KEYS_INFO, &mut okm)
        .map_err(|_| ProtoError::Auth)?;
    let mut d = Derived {
        th,
        k_i2r: [0; 32],
        k_r2i: [0; 32],
        mac_r: [0; 32],
        mac_i: [0; 32],
    };
    d.k_i2r.copy_from_slice(&okm[..32]);
    d.k_r2i.copy_from_slice(&okm[32..64]);
    d.mac_r = hmac_tag(&okm[64..96], b"R", &th)?;
    d.mac_i = hmac_tag(&okm[96..128], b"I", &th)?;
    okm.zeroize();
    Ok(d)
}

/// An established encrypted connection, ready to be split between reader and writer tasks.
pub struct SecureConn<S> {
    /// Receiving half.
    pub reader: SecureReader<ReadHalf<S>>,
    /// Sending half.
    pub writer: SecureWriter<WriteHalf<S>>,
}

/// Run the initiator side with fresh random ephemeral key and nonce.
pub async fn initiate<S>(stream: S, gk: &GroupKey) -> Result<SecureConn<S>>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut eph = [0u8; 32];
    let mut nonce = [0u8; 16];
    fill_random(&mut eph)?;
    fill_random(&mut nonce)?;
    initiate_with(stream, gk, eph, nonce).await
}

/// Run the responder side with fresh random ephemeral key and nonce.
pub async fn respond<S>(stream: S, gk: &GroupKey) -> Result<SecureConn<S>>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut eph = [0u8; 32];
    let mut nonce = [0u8; 16];
    fill_random(&mut eph)?;
    fill_random(&mut nonce)?;
    respond_with(stream, gk, eph, nonce).await
}

/// Initiator with caller-chosen secrets (used by tests and test vectors).
pub async fn initiate_with<S>(
    stream: S,
    gk: &GroupKey,
    eph_secret: [u8; 32],
    nonce: [u8; 16],
) -> Result<SecureConn<S>>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    tokio::time::timeout(
        HANDSHAKE_TIMEOUT,
        initiate_inner(stream, gk, eph_secret, nonce),
    )
    .await
    .map_err(|_| ProtoError::Timeout)?
}

/// Responder with caller-chosen secrets (used by tests and test vectors).
pub async fn respond_with<S>(
    stream: S,
    gk: &GroupKey,
    eph_secret: [u8; 32],
    nonce: [u8; 16],
) -> Result<SecureConn<S>>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    tokio::time::timeout(
        HANDSHAKE_TIMEOUT,
        respond_inner(stream, gk, eph_secret, nonce),
    )
    .await
    .map_err(|_| ProtoError::Timeout)?
}

fn verify(expected: &[u8; 32], got: &[u8; 32]) -> Result<()> {
    if bool::from(expected.ct_eq(got)) {
        Ok(())
    } else {
        Err(ProtoError::Auth)
    }
}

async fn initiate_inner<S>(
    stream: S,
    gk: &GroupKey,
    mut eph_secret: [u8; 32],
    nonce: [u8; 16],
) -> Result<SecureConn<S>>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let gid = gk.group_id();
    let secret = StaticSecret::from(eph_secret);
    eph_secret.zeroize();
    let eph_i = PublicKey::from(&secret).to_bytes();

    let (rd, mut wr) = split(stream);
    let mut rd = PlainReader(rd);
    let hs1 = Message::Hs1 {
        v: VERSION,
        gid: gid.to_hex(),
        eph_i: b64url_encode(&eph_i),
        n_i: b64url_encode(&nonce),
    };
    write_plain_frame(&mut wr, &hs1, &[]).await?;

    let reply = read_frame(&mut rd).await?;
    let (eph_r, n_r, mac_r) = match reply.message {
        Message::Hs2 {
            v,
            eph_r,
            n_r,
            mac_r,
        } => {
            if v != VERSION {
                return Err(ProtoError::Version(v));
            }
            (
                b64url_decode_exact::<32>(&eph_r)?,
                b64url_decode_exact::<16>(&n_r)?,
                b64url_decode_exact::<32>(&mac_r)?,
            )
        }
        Message::Error { code, .. } if code == "wrong_group" => return Err(ProtoError::WrongGroup),
        Message::Error { code, .. } if code == "version" => return Err(ProtoError::Version(0)),
        other => {
            return Err(ProtoError::BadHeader(format!(
                "expected hs2, got {}",
                other.type_name()
            )))
        }
    };

    let shared = secret.diffie_hellman(&PublicKey::from(eph_r));
    if !shared.was_contributory() {
        return Err(ProtoError::Auth);
    }
    let keys = derive(&HandshakeInputs {
        gk,
        gid: &gid,
        eph_i: &eph_i,
        n_i: &nonce,
        eph_r: &eph_r,
        n_r: &n_r,
        dh: shared.as_bytes(),
    })?;
    verify(&keys.mac_r, &mac_r)?;

    let hs3 = Message::Hs3 {
        mac_i: b64url_encode(&keys.mac_i),
    };
    write_plain_frame(&mut wr, &hs3, &[]).await?;

    Ok(SecureConn {
        reader: SecureReader::new(rd.0, keys.k_r2i),
        writer: SecureWriter::new(wr, keys.k_i2r),
    })
}

async fn respond_inner<S>(
    stream: S,
    gk: &GroupKey,
    mut eph_secret: [u8; 32],
    nonce: [u8; 16],
) -> Result<SecureConn<S>>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let gid = gk.group_id();
    let secret = StaticSecret::from(eph_secret);
    eph_secret.zeroize();
    let eph_r = PublicKey::from(&secret).to_bytes();

    let (rd, mut wr) = split(stream);
    let mut rd = PlainReader(rd);

    let first = read_frame(&mut rd).await?;
    let Message::Hs1 {
        v,
        gid: their_gid,
        eph_i,
        n_i,
    } = first.message
    else {
        return Err(ProtoError::BadHeader("expected hs1".into()));
    };
    if v != VERSION {
        let _ = write_plain_frame(&mut wr, &error_message("version"), &[]).await;
        return Err(ProtoError::Version(v));
    }
    if GroupId::from_hex(&their_gid)? != gid {
        let _ = write_plain_frame(&mut wr, &error_message("wrong_group"), &[]).await;
        return Err(ProtoError::WrongGroup);
    }
    let eph_i = b64url_decode_exact::<32>(&eph_i)?;
    let n_i = b64url_decode_exact::<16>(&n_i)?;

    let shared = secret.diffie_hellman(&PublicKey::from(eph_i));
    if !shared.was_contributory() {
        return Err(ProtoError::Auth);
    }
    let keys = derive(&HandshakeInputs {
        gk,
        gid: &gid,
        eph_i: &eph_i,
        n_i: &n_i,
        eph_r: &eph_r,
        n_r: &nonce,
        dh: shared.as_bytes(),
    })?;

    let hs2 = Message::Hs2 {
        v: VERSION,
        eph_r: b64url_encode(&eph_r),
        n_r: b64url_encode(&nonce),
        mac_r: b64url_encode(&keys.mac_r),
    };
    write_plain_frame(&mut wr, &hs2, &[]).await?;

    let last = read_frame(&mut rd).await?;
    let Message::Hs3 { mac_i } = last.message else {
        return Err(ProtoError::BadHeader("expected hs3".into()));
    };
    verify(&keys.mac_i, &b64url_decode_exact::<32>(&mac_i)?)?;

    Ok(SecureConn {
        reader: SecureReader::new(rd.0, keys.k_i2r),
        writer: SecureWriter::new(wr, keys.k_r2i),
    })
}

fn error_message(code: &str) -> Message {
    Message::Error {
        code: code.to_string(),
        msg: None,
        reference: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    fn gk(n: u8) -> GroupKey {
        GroupKey([n; 32])
    }

    #[tokio::test]
    async fn both_sides_agree_and_can_talk() {
        let (a, b) = duplex(1 << 16);
        let key = gk(1);
        let (i, r) = tokio::join!(initiate(a, &key), respond(b, &key));
        let (mut i, mut r) = (i.unwrap(), r.unwrap());
        i.writer.write_frame(&Message::Ping, &[]).await.unwrap();
        assert_eq!(r.reader.read_frame().await.unwrap().message, Message::Ping);
        r.writer.write_frame(&Message::Pong, &[]).await.unwrap();
        assert_eq!(i.reader.read_frame().await.unwrap().message, Message::Pong);
    }

    #[tokio::test]
    async fn different_groups_fail_with_wrong_group_on_both_sides() {
        let (a, b) = duplex(1 << 16);
        let (k1, k2) = (gk(1), gk(2));
        let (i, r) = tokio::join!(initiate(a, &k1), respond(b, &k2));
        assert!(matches!(i, Err(ProtoError::WrongGroup)), "{:?}", i.err());
        assert!(matches!(r, Err(ProtoError::WrongGroup)), "{:?}", r.err());
    }

    #[tokio::test]
    async fn bad_mac_from_initiator_is_rejected() {
        let (a, b) = duplex(1 << 16);
        let key = gk(3);
        let responder = respond(b, &key);
        let attacker = async {
            let (rd, mut wr) = split(a);
            let mut rd = PlainReader(rd);
            let hs1 = Message::Hs1 {
                v: VERSION,
                gid: key.group_id().to_hex(),
                eph_i: b64url_encode(&PublicKey::from(&StaticSecret::from([5u8; 32])).to_bytes()),
                n_i: b64url_encode(&[1u8; 16]),
            };
            write_plain_frame(&mut wr, &hs1, &[]).await.unwrap();
            let _hs2 = read_frame(&mut rd).await.unwrap();
            let hs3 = Message::Hs3 {
                mac_i: b64url_encode(&[0u8; 32]),
            };
            write_plain_frame(&mut wr, &hs3, &[]).await.unwrap();
        };
        let (r, ()) = tokio::join!(responder, attacker);
        assert!(matches!(r, Err(ProtoError::Auth)), "{:?}", r.err());
    }

    #[tokio::test]
    async fn bad_mac_from_responder_is_rejected() {
        let (a, b) = duplex(1 << 16);
        let key = gk(4);
        let initiator = initiate(a, &key);
        let fake = async {
            let (rd, mut wr) = split(b);
            let mut rd = PlainReader(rd);
            let _hs1 = read_frame(&mut rd).await.unwrap();
            let hs2 = Message::Hs2 {
                v: VERSION,
                eph_r: b64url_encode(&PublicKey::from(&StaticSecret::from([6u8; 32])).to_bytes()),
                n_r: b64url_encode(&[2u8; 16]),
                mac_r: b64url_encode(&[0u8; 32]),
            };
            write_plain_frame(&mut wr, &hs2, &[]).await.unwrap();
        };
        let (i, ()) = tokio::join!(initiator, fake);
        assert!(matches!(i, Err(ProtoError::Auth)), "{:?}", i.err());
    }

    #[tokio::test]
    async fn low_order_public_key_is_rejected() {
        let (a, b) = duplex(1 << 16);
        let key = gk(5);
        let responder = respond(b, &key);
        let attacker = async {
            let (_rd, mut wr) = split(a);
            let hs1 = Message::Hs1 {
                v: VERSION,
                gid: key.group_id().to_hex(),
                eph_i: b64url_encode(&[0u8; 32]),
                n_i: b64url_encode(&[1u8; 16]),
            };
            write_plain_frame(&mut wr, &hs1, &[]).await.unwrap();
        };
        let (r, ()) = tokio::join!(responder, attacker);
        assert!(matches!(r, Err(ProtoError::Auth)), "{:?}", r.err());
    }

    #[tokio::test]
    async fn unsupported_version_is_reported() {
        let (a, b) = duplex(1 << 16);
        let key = gk(6);
        let responder = respond(b, &key);
        let peer = async {
            let (rd, mut wr) = split(a);
            let mut rd = PlainReader(rd);
            let hs1 = Message::Hs1 {
                v: 9,
                gid: key.group_id().to_hex(),
                eph_i: b64url_encode(&[1u8; 32]),
                n_i: b64url_encode(&[1u8; 16]),
            };
            write_plain_frame(&mut wr, &hs1, &[]).await.unwrap();
            read_frame(&mut rd).await.unwrap().message
        };
        let (r, reply) = tokio::join!(responder, peer);
        assert!(matches!(r, Err(ProtoError::Version(9))));
        assert_eq!(reply, error_message("version"));
    }

    #[tokio::test(start_paused = true)]
    async fn silent_peer_times_out() {
        let (a, _b) = duplex(1 << 16);
        let key = gk(7);
        let r = initiate(a, &key).await;
        assert!(matches!(r, Err(ProtoError::Timeout)), "{:?}", r.err());
    }

    #[test]
    fn derivation_is_deterministic_and_input_sensitive() {
        let key = gk(8);
        let gid = key.group_id();
        let base = |dh: [u8; 32], n_r: [u8; 16]| {
            derive(&HandshakeInputs {
                gk: &key,
                gid: &gid,
                eph_i: &[1; 32],
                n_i: &[2; 16],
                eph_r: &[3; 32],
                n_r: &n_r,
                dh: &dh,
            })
            .unwrap()
        };
        let a = base([9; 32], [4; 16]);
        let b = base([9; 32], [4; 16]);
        assert_eq!(
            (a.k_i2r, a.k_r2i, a.mac_r, a.mac_i),
            (b.k_i2r, b.k_r2i, b.mac_r, b.mac_i)
        );
        let c = base([10; 32], [4; 16]);
        let d = base([9; 32], [5; 16]);
        assert_ne!(a.k_i2r, c.k_i2r);
        assert_ne!(a.k_i2r, d.k_i2r);
        assert_ne!(a.k_i2r, a.k_r2i);
        assert_ne!(a.mac_r, a.mac_i);
    }
}
