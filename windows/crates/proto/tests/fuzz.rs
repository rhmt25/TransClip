//! Random-input tests (P7-01): hostile input must end in an error, never a panic.

use proptest::prelude::*;
use proto::beacon::Beacon;
use proto::frame::FrameDecoder;
use proto::pairing::PairingInfo;
use proto::path::validate_relative_path;
use proto::record;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    #[test]
    fn beacon_decode_never_panics(data in proptest::collection::vec(any::<u8>(), 0..512)) {
        let _ = Beacon::decode(&data);
    }

    #[test]
    fn pairing_parse_never_panics(s in ".{0,400}") {
        let _ = PairingInfo::parse(&s);
    }

    #[test]
    fn pairing_parse_survives_structured_garbage(
        k in "[A-Za-z0-9_%=&-]{0,60}",
        d in "[0-9a-f%&=]{0,40}",
        a in "[0-9.:,%A-F]{0,60}",
        x in "[0-9-]{0,24}",
    ) {
        let uri = format!("transclip://pair?v=1&k={k}&g=x&d={d}&a={a}&x={x}");
        let _ = PairingInfo::parse(&uri);
    }

    #[test]
    fn record_open_rejects_garbage(
        key in proptest::array::uniform32(any::<u8>()),
        counter in any::<u64>(),
        data in proptest::collection::vec(any::<u8>(), 0..300),
    ) {
        prop_assert!(record::open(&key, counter, &data).is_err());
    }

    #[test]
    fn path_validation_never_panics(s in ".{0,300}") {
        let _ = validate_relative_path(&s);
    }

    #[test]
    fn accepted_paths_are_always_safe(s in "[a-zA-Z0-9./\\: _-]{0,80}") {
        if let Ok(segments) = validate_relative_path(&s) {
            for seg in segments {
                prop_assert!(!seg.is_empty() && seg != "." && seg != "..");
                prop_assert!(!seg.contains(['/', '\\', ':']));
            }
        }
    }

    #[test]
    fn frame_decoder_never_panics_on_chunked_garbage(
        chunks in proptest::collection::vec(proptest::collection::vec(any::<u8>(), 0..200), 0..8),
    ) {
        let mut d = FrameDecoder::new();
        for c in chunks {
            if d.push(&c).is_err() {
                break;
            }
        }
    }
}
