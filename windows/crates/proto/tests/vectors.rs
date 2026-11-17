//! Reads `docs/test-vectors/*.json` and checks them against the implementation.
//!
//! These checks do not reuse the generator logic, so they catch both a stale vector file and
//! a regression in the codec. The Kotlin test suite reads the same files.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use proto::beacon::{Beacon, Platform};
use proto::frame::FrameDecoder;
use proto::group::GroupKey;
use proto::handshake::{derive, HandshakeInputs};
use proto::ids::{DeviceId, GroupId};
use proto::msg::Message;
use proto::pairing::PairingInfo;
use proto::path::{validate_relative_path, with_counter};
use proto::record::open;
use proto::ProtoError;
use serde_json::Value;
use std::path::PathBuf;
use x25519_dalek::{PublicKey, StaticSecret};

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../docs/test-vectors")
}

fn load(name: &str) -> Value {
    let text = std::fs::read_to_string(dir().join(name))
        .unwrap_or_else(|e| panic!("missing {name}: {e}. Run `cargo run -p tvgen`."));
    serde_json::from_str(&text).unwrap()
}

fn unhex(v: &Value) -> Vec<u8> {
    hex::decode(v.as_str().unwrap()).unwrap()
}

fn arr<const N: usize>(v: &Value) -> [u8; N] {
    unhex(v).try_into().unwrap()
}

#[test]
fn vector_files_are_up_to_date() {
    for (name, value) in proto::vectors::generate().unwrap() {
        let on_disk = std::fs::read_to_string(dir().join(name))
            .unwrap_or_else(|e| panic!("missing {name}: {e}. Run `cargo run -p tvgen`."));
        let mut expected = serde_json::to_string_pretty(&value).unwrap();
        expected.push('\n');
        assert_eq!(
            on_disk.replace("\r\n", "\n"),
            expected,
            "{name} is stale; run `cargo run -p tvgen` and commit the result"
        );
    }
}

#[test]
fn frame_vectors() {
    let v = load("frames.json");
    for case in v["valid"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let frames = FrameDecoder::new()
            .push(&unhex(&case["encoded_hex"]))
            .unwrap();
        assert_eq!(frames.len(), 1, "{name}");
        assert_eq!(frames[0].payload, unhex(&case["payload_hex"]), "{name}");
        if case.get("unknown").is_some() {
            assert!(
                matches!(frames[0].message, Message::Unknown { .. }),
                "{name}"
            );
        } else {
            let expected: Message = serde_json::from_value(case["header"].clone()).unwrap();
            assert_eq!(frames[0].message, expected, "{name}");
        }
    }
    for case in v["invalid"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let result = FrameDecoder::new().push(&unhex(&case["encoded_hex"]));
        match (case["error"].as_str().unwrap(), result) {
            ("limit", Err(ProtoError::Limit(_)))
            | ("bad_header", Err(ProtoError::BadHeader(_))) => {}
            (want, other) => panic!("{name}: wanted {want}, got {other:?}"),
        }
    }
}

#[test]
fn handshake_vector() {
    let v = load("handshake.json");
    let gk = GroupKey(arr(&v["gk_hex"]));
    assert_eq!(gk.group_id().to_hex(), v["gid_hex"].as_str().unwrap());
    let secret_i = StaticSecret::from(arr::<32>(&v["eph_i_secret_hex"]));
    let secret_r = StaticSecret::from(arr::<32>(&v["eph_r_secret_hex"]));
    assert_eq!(
        PublicKey::from(&secret_i).to_bytes().to_vec(),
        unhex(&v["eph_i_public_hex"])
    );
    assert_eq!(
        PublicKey::from(&secret_r).to_bytes().to_vec(),
        unhex(&v["eph_r_public_hex"])
    );
    let eph_i: [u8; 32] = arr(&v["eph_i_public_hex"]);
    let eph_r: [u8; 32] = arr(&v["eph_r_public_hex"]);
    let dh_i = secret_i.diffie_hellman(&PublicKey::from(eph_r));
    let dh_r = secret_r.diffie_hellman(&PublicKey::from(eph_i));
    assert_eq!(dh_i.as_bytes(), dh_r.as_bytes());
    assert_eq!(dh_i.as_bytes().to_vec(), unhex(&v["dh_hex"]));
    let keys = derive(&HandshakeInputs {
        gk: &gk,
        gid: &gk.group_id(),
        eph_i: &eph_i,
        n_i: &arr(&v["n_i_hex"]),
        eph_r: &eph_r,
        n_r: &arr(&v["n_r_hex"]),
        dh: dh_i.as_bytes(),
    })
    .unwrap();
    assert_eq!(keys.th.to_vec(), unhex(&v["th_hex"]));
    assert_eq!(keys.k_i2r.to_vec(), unhex(&v["k_i2r_hex"]));
    assert_eq!(keys.k_r2i.to_vec(), unhex(&v["k_r2i_hex"]));
    assert_eq!(keys.mac_r.to_vec(), unhex(&v["mac_r_hex"]));
    assert_eq!(keys.mac_i.to_vec(), unhex(&v["mac_i_hex"]));
    for key in ["hs1_frame_hex", "hs2_frame_hex", "hs3_frame_hex"] {
        let frames = FrameDecoder::new().push(&unhex(&v[key])).unwrap();
        assert_eq!(frames.len(), 1, "{key}");
        assert!(frames[0].message.validate().is_ok() || key == "hs3_frame_hex");
    }
}

#[test]
fn record_vectors() {
    let v = load("records.json");
    let key: [u8; 32] = arr(&v["key_hex"]);
    for case in v["cases"].as_array().unwrap() {
        let counter = case["counter"].as_u64().unwrap();
        let plain = open(&key, counter, &unhex(&case["ciphertext_hex"])).unwrap();
        assert_eq!(plain, unhex(&case["plaintext_hex"]), "counter {counter}");
        assert!(open(&key, counter + 1, &unhex(&case["ciphertext_hex"])).is_err());
    }
}

#[test]
fn beacon_vectors() {
    let v = load("beacon.json");
    for case in v["valid"].as_array().unwrap() {
        let b = Beacon::decode(&unhex(&case["encoded_hex"])).unwrap();
        assert_eq!(b.gid, GroupId(arr(&case["gid_hex"])));
        assert_eq!(b.device_id, DeviceId(arr(&case["device_id_hex"])));
        assert_eq!(b.port as u64, case["port"].as_u64().unwrap());
        assert_eq!(b.name, case["name"].as_str().unwrap());
        let want = if case["platform"] == "windows" {
            Platform::Windows
        } else {
            Platform::Android
        };
        assert_eq!(b.platform, want);
    }
    for case in v["invalid"].as_array().unwrap() {
        assert!(
            Beacon::decode(&unhex(&case["encoded_hex"])).is_err(),
            "{}",
            case["name"]
        );
    }
}

#[test]
fn pairing_vectors() {
    let v = load("pairing.json");
    for case in v["valid"].as_array().unwrap() {
        let p = PairingInfo::parse(case["uri"].as_str().unwrap()).unwrap();
        assert_eq!(p.gk.0.to_vec(), unhex(&case["gk_hex"]));
        assert_eq!(p.group_name, case["group_name"].as_str().unwrap());
        assert_eq!(p.inviter.to_hex(), case["inviter_hex"].as_str().unwrap());
        let addrs: Vec<&str> = case["addrs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a.as_str().unwrap())
            .collect();
        assert_eq!(p.addrs, addrs);
        assert_eq!(p.expires, case["expires"].as_u64().unwrap());
    }
    for uri in v["invalid"].as_array().unwrap() {
        assert!(PairingInfo::parse(uri.as_str().unwrap()).is_err(), "{uri}");
    }
}

#[test]
fn path_vectors() {
    let v = load("paths.json");
    for p in v["valid"].as_array().unwrap() {
        assert!(validate_relative_path(p.as_str().unwrap()).is_ok(), "{p}");
    }
    for p in v["invalid"].as_array().unwrap() {
        assert!(validate_relative_path(p.as_str().unwrap()).is_err(), "{p}");
    }
    for c in v["conflict_names"].as_array().unwrap() {
        let got = with_counter(c["name"].as_str().unwrap(), c["n"].as_u64().unwrap() as u32);
        assert_eq!(got, c["result"].as_str().unwrap());
    }
}
