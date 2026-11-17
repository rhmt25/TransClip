//! Deterministic test vectors shared by every implementation (PROTOCOL.md section 12).
//!
//! `tvgen` writes these to `docs/test-vectors/*.json`. Both the Rust and the Kotlin test
//! suites read the files, so a change to the wire format shows up on both sides.

use crate::beacon::{Beacon, Platform};
use crate::error::Result;
use crate::frame::encode_frame;
use crate::group::GroupKey;
use crate::handshake::{derive, HandshakeInputs};
use crate::ids::{b64url_encode, DeviceId, GroupId};
use crate::msg::{sample_messages, Message, SAMPLE_UUID_A};
use crate::pairing::PairingInfo;
use crate::path::{with_counter, EXAMPLE_INVALID_PATHS, EXAMPLE_VALID_PATHS};
use crate::record::seal;
use serde_json::{json, Value};
use x25519_dalek::{PublicKey, StaticSecret};

/// All vector files as `(file name, content)`.
pub fn generate() -> Result<Vec<(&'static str, Value)>> {
    Ok(vec![
        ("frames.json", frames()?),
        ("handshake.json", handshake()?),
        ("records.json", records()?),
        ("beacon.json", beacon()?),
        ("pairing.json", pairing()?),
        ("paths.json", paths()),
    ])
}

fn payload_for(message: &Message) -> Vec<u8> {
    match message {
        Message::Text { .. } => b"hello".to_vec(),
        Message::Image { .. } => (0u8..16).collect(),
        Message::FileChunk { .. } => (0u8..32).collect(),
        _ => Vec::new(),
    }
}

fn raw_frame(header: &str, payload: &[u8]) -> Vec<u8> {
    let mut out = (header.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(payload);
    out
}

fn frames() -> Result<Value> {
    let mut valid = Vec::new();
    for m in sample_messages() {
        let payload = payload_for(&m);
        let encoded = encode_frame(&m, &payload)?;
        valid.push(json!({
            "name": m.type_name(),
            "header": serde_json::to_value(&m).unwrap_or(Value::Null),
            "payload_hex": hex::encode(&payload),
            "encoded_hex": hex::encode(encoded),
        }));
    }
    // An unknown type must be decoded as "unknown" and its payload skipped.
    let unknown = raw_frame(r#"{"t":"from_the_future","size":3,"x":1}"#, b"abc");
    valid.push(json!({
        "name": "unknown_type",
        "header": {"t": "from_the_future"},
        "payload_hex": hex::encode(b"abc"),
        "encoded_hex": hex::encode(unknown),
        "unknown": true,
    }));

    let big_chunk =
        format!(r#"{{"t":"file_chunk","xfer_id":"{SAMPLE_UUID_A}","offset":0,"size":65537}}"#);
    let invalid = [
        ("header_length_zero", vec![0, 0, 0, 0], "limit"),
        (
            "header_length_too_big",
            65_537u32.to_be_bytes().to_vec(),
            "limit",
        ),
        ("header_not_json", raw_frame("{", &[]), "bad_header"),
        ("header_not_object", raw_frame("[]", &[]), "bad_header"),
        (
            "missing_size",
            raw_frame(r#"{"t":"ping"}"#, &[]),
            "bad_header",
        ),
        (
            "negative_size",
            raw_frame(r#"{"t":"ping","size":-1}"#, &[]),
            "bad_header",
        ),
        (
            "missing_type",
            raw_frame(r#"{"size":0}"#, &[]),
            "bad_header",
        ),
        (
            "known_type_missing_fields",
            raw_frame(r#"{"t":"pull","size":0}"#, &[]),
            "bad_header",
        ),
        ("payload_over_limit", raw_frame(&big_chunk, &[]), "limit"),
        (
            "ping_with_payload",
            raw_frame(r#"{"t":"ping","size":1}"#, &[0]),
            "limit",
        ),
    ];
    let invalid: Vec<Value> = invalid
        .into_iter()
        .map(|(name, bytes, error)| {
            json!({"name": name, "encoded_hex": hex::encode(bytes), "error": error})
        })
        .collect();
    Ok(json!({"valid": valid, "invalid": invalid}))
}

fn handshake() -> Result<Value> {
    let gk = GroupKey([0x11; 32]);
    let gid = gk.group_id();
    let secret_i = StaticSecret::from([0x22u8; 32]);
    let secret_r = StaticSecret::from([0x33u8; 32]);
    let eph_i = PublicKey::from(&secret_i).to_bytes();
    let eph_r = PublicKey::from(&secret_r).to_bytes();
    let n_i = [0x44u8; 16];
    let n_r = [0x55u8; 16];
    let dh = secret_i.diffie_hellman(&PublicKey::from(eph_r));
    let keys = derive(&HandshakeInputs {
        gk: &gk,
        gid: &gid,
        eph_i: &eph_i,
        n_i: &n_i,
        eph_r: &eph_r,
        n_r: &n_r,
        dh: dh.as_bytes(),
    })?;
    let hs1 = Message::Hs1 {
        v: 1,
        gid: gid.to_hex(),
        eph_i: b64url_encode(&eph_i),
        n_i: b64url_encode(&n_i),
    };
    let hs2 = Message::Hs2 {
        v: 1,
        eph_r: b64url_encode(&eph_r),
        n_r: b64url_encode(&n_r),
        mac_r: b64url_encode(&keys.mac_r),
    };
    let hs3 = Message::Hs3 {
        mac_i: b64url_encode(&keys.mac_i),
    };
    Ok(json!({
        "gk_hex": hex::encode(gk.0),
        "gid_hex": gid.to_hex(),
        "eph_i_secret_hex": hex::encode([0x22u8; 32]),
        "eph_i_public_hex": hex::encode(eph_i),
        "n_i_hex": hex::encode(n_i),
        "eph_r_secret_hex": hex::encode([0x33u8; 32]),
        "eph_r_public_hex": hex::encode(eph_r),
        "n_r_hex": hex::encode(n_r),
        "dh_hex": hex::encode(dh.as_bytes()),
        "th_hex": hex::encode(keys.th),
        "k_i2r_hex": hex::encode(keys.k_i2r),
        "k_r2i_hex": hex::encode(keys.k_r2i),
        "mac_r_hex": hex::encode(keys.mac_r),
        "mac_i_hex": hex::encode(keys.mac_i),
        "hs1_frame_hex": hex::encode(encode_frame(&hs1, &[])?),
        "hs2_frame_hex": hex::encode(encode_frame(&hs2, &[])?),
        "hs3_frame_hex": hex::encode(encode_frame(&hs3, &[])?),
    }))
}

fn records() -> Result<Value> {
    let key = [0x66u8; 32];
    let pattern: Vec<u8> = (0..1000).map(|i| (i % 251) as u8).collect();
    let mut cases = Vec::new();
    for (counter, plaintext) in [
        (0u64, b"a".to_vec()),
        (1, b"hello".to_vec()),
        (2, pattern.clone()),
        (255, b"counter 255".to_vec()),
        (65_536, b"counter 65536".to_vec()),
        (4_294_967_295, b"last valid counter".to_vec()),
    ] {
        let ct = seal(&key, counter, &plaintext)?;
        cases.push(json!({
            "counter": counter,
            "plaintext_hex": hex::encode(&plaintext),
            "ciphertext_hex": hex::encode(ct),
        }));
    }
    Ok(json!({"key_hex": hex::encode(key), "cases": cases}))
}

fn beacon() -> Result<Value> {
    let mut valid = Vec::new();
    for (name, platform, port) in [
        ("", Platform::Windows, 47820u16),
        ("PC Rahmat", Platform::Windows, 47820),
        ("Pixel 8", Platform::Android, 50000),
        ("\u{65e5}\u{672c}\u{8a9e} \u{1f600}", Platform::Android, 1),
    ] {
        let b = Beacon {
            gid: GroupId([0xa1; 8]),
            device_id: DeviceId([0xb2; 16]),
            port,
            platform,
            name: name.to_string(),
        };
        valid.push(json!({
            "gid_hex": b.gid.to_hex(),
            "device_id_hex": b.device_id.to_hex(),
            "port": port,
            "platform": if platform == Platform::Windows { "windows" } else { "android" },
            "name": name,
            "encoded_hex": hex::encode(b.encode()?),
        }));
    }
    let good = Beacon {
        gid: GroupId([0xa1; 8]),
        device_id: DeviceId([0xb2; 16]),
        port: 1,
        platform: Platform::Windows,
        name: "x".into(),
    }
    .encode()?;
    let mut bad_magic = good.clone();
    bad_magic[0] = b'X';
    let mut bad_version = good.clone();
    bad_version[4] = 2;
    let mut bad_platform = good.clone();
    bad_platform[31] = 9;
    let mut long_name = good.clone();
    long_name.extend(vec![b'a'; 65]);
    let mut bad_utf8 = good.clone();
    bad_utf8.push(0xff);
    let invalid = [
        ("too_short", good[..43].to_vec()),
        ("bad_magic", bad_magic),
        ("bad_version", bad_version),
        ("bad_platform", bad_platform),
        ("name_too_long", long_name),
        ("name_not_utf8", bad_utf8),
    ];
    let invalid: Vec<Value> = invalid
        .into_iter()
        .map(|(name, bytes)| json!({"name": name, "encoded_hex": hex::encode(bytes)}))
        .collect();
    Ok(json!({"valid": valid, "invalid": invalid}))
}

fn pairing() -> Result<Value> {
    let mut valid = Vec::new();
    for (name, addrs) in [
        ("Rumah", vec![]),
        ("Kantor & Rumah", vec!["192.168.1.5:47820".to_string()]),
        (
            "a=b?c#d%e",
            vec!["10.0.0.2:47820".to_string(), "[fe80::1]:47820".to_string()],
        ),
        ("\u{65e5}\u{672c}\u{8a9e} \u{1f600}", vec![]),
    ] {
        let p = PairingInfo {
            gk: GroupKey([5; 32]),
            group_name: name.to_string(),
            inviter: DeviceId([6; 16]),
            addrs: addrs.clone(),
            expires: 1_800_000_000,
        };
        valid.push(json!({
            "uri": p.to_uri(),
            "gk_hex": hex::encode(p.gk.0),
            "group_name": name,
            "inviter_hex": p.inviter.to_hex(),
            "addrs": addrs,
            "expires": p.expires,
        }));
    }
    let good = PairingInfo {
        gk: GroupKey([5; 32]),
        group_name: "x".into(),
        inviter: DeviceId([6; 16]),
        addrs: vec![],
        expires: 1_800_000_000,
    }
    .to_uri();
    let invalid = [
        "https://example.com".to_string(),
        good.replace("v=1", "v=2"),
        good.replace("&x=1800000000", ""),
        good.replace("&g=x", "&g=%ZZ"),
        good.replace("&g=x", "&g=%FF"),
        good.replace("k=", "k=AAAA"),
    ];
    Ok(json!({"valid": valid, "invalid": invalid}))
}

fn paths() -> Value {
    let conflicts: Vec<Value> = [
        ("a.txt", 1u32),
        ("archive.tar.gz", 2),
        (".bashrc", 1),
        ("noext", 3),
    ]
    .iter()
    .map(|(name, n)| json!({"name": name, "n": n, "result": with_counter(name, *n)}))
    .collect();
    json!({
        "valid": EXAMPLE_VALID_PATHS,
        "invalid": EXAMPLE_INVALID_PATHS,
        "conflict_names": conflicts,
    })
}
