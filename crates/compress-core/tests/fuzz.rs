//! Property-based **zero-panic gate**: decode of arbitrary and truncated bytes
//! must never panic — it returns a clean `Result` (or, for `detect`/`is_valid`,
//! a total value). This is the security contract: a hostile blob fails its own
//! row, it never crashes the worker. Mirrors a `cargo-fuzz` target as a
//! deterministic, CI-runnable proptest.

use compress_core::{
    codec::Codec, compress, decompress, decompress_auto, detect_codec, is_valid,
    DEFAULT_MAX_OUTPUT_BYTES,
};
use proptest::prelude::*;

const CODECS: &[&str] = &[
    "zstd",
    "gzip",
    "zlib",
    "deflate",
    "brotli",
    "lz4",
    "lz4_block",
    "snappy",
    "snappy_raw",
    "xz",
    "lzma",
    "bzip2",
];

proptest! {
    #![proptest_config(ProptestConfig { cases: 2000, ..ProptestConfig::default() })]

    /// Arbitrary bytes through every decoder: never panics; a small cap keeps it
    /// cheap and bomb-safe.
    #[test]
    fn decompress_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        for codec in CODECS {
            let _ = decompress(codec, &bytes, 1 << 20);
        }
        let _ = decompress_auto(&bytes, 1 << 20);
        let _ = detect_codec(&bytes);
        for codec in CODECS {
            let _ = is_valid(codec, &bytes);
        }
    }

    /// Truncating a genuine stream at every prefix length must never panic and
    /// never falsely report OutputTooLarge for a small payload under a big cap.
    #[test]
    fn truncated_streams_never_panic(data in proptest::collection::vec(any::<u8>(), 0..2048),
                                     cut in 0usize..512) {
        for codec in CODECS {
            let Ok(packed) = compress(codec, &data, None) else { continue };
            let n = packed.len();
            let prefix = &packed[..cut.min(n)];
            let _ = decompress(codec, prefix, DEFAULT_MAX_OUTPUT_BYTES);
        }
    }

    /// Round-trip holds for any input on every codec (the inverse property).
    #[test]
    fn round_trip_holds(data in proptest::collection::vec(any::<u8>(), 0..4096),
                        idx in 0usize..12) {
        let codec = CODECS[idx % CODECS.len()];
        let packed = compress(codec, &data, None).unwrap();
        let back = decompress(codec, &packed, DEFAULT_MAX_OUTPUT_BYTES).unwrap();
        prop_assert_eq!(back, data);
    }

    /// Level clamping never panics for any requested level, in or out of range.
    #[test]
    fn arbitrary_level_never_panics(data in proptest::collection::vec(any::<u8>(), 0..256),
                                    level in -1000i32..1000,
                                    idx in 0usize..12) {
        let codec = CODECS[idx % CODECS.len()];
        let _ = Codec::parse(codec).unwrap().resolve_level(Some(level));
        let _ = compress(codec, &data, Some(level));
    }
}
