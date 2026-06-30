//! Golden round-trip fixtures per codec, plus the cross-variant and bomb-guard
//! correctness gates. Every codec must round-trip an empty blob, small text,
//! binary, and a few-hundred-KB payload across its default / min / max levels;
//! the wrong frame-vs-raw variant must error cleanly (never crash); and a
//! crafted high-ratio bomb must abort at the cap rather than OOM.

use compress_core::{
    codec::Codec, compress, decompress, decompress_auto, decompressed_size, detect_codec, is_valid,
    ratio, CodecError, DEFAULT_MAX_OUTPUT_BYTES,
};

/// Every codec name the build supports.
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

/// A few representative payloads: empty, tiny text, small binary, and a larger
/// compressible blob.
fn payloads() -> Vec<Vec<u8>> {
    let mut big = Vec::new();
    for i in 0..200_000u32 {
        big.push((i % 251) as u8);
    }
    vec![
        Vec::new(),
        b"hello, world".to_vec(),
        (0u8..=255).cycle().take(1024).collect(),
        b"the quick brown fox jumps over the lazy dog\n".repeat(500),
        big,
    ]
}

/// Default + min + max level for a codec (deduplicated); `None` for level-less.
fn levels(codec: &str) -> Vec<Option<i32>> {
    let c = Codec::parse(codec).unwrap();
    match c.level_range() {
        None => vec![None],
        Some((lo, hi)) => {
            let def = c.default_level();
            let mut v = vec![None, Some(lo), Some(hi)];
            if let Some(d) = def {
                v.push(Some(d));
            }
            v.sort();
            v.dedup();
            v
        }
    }
}

#[test]
fn round_trip_every_codec_every_level() {
    for codec in CODECS {
        for data in payloads() {
            for level in levels(codec) {
                let packed = compress(codec, &data, level)
                    .unwrap_or_else(|e| panic!("compress {codec} lvl {level:?}: {e}"));
                let back = decompress(codec, &packed, DEFAULT_MAX_OUTPUT_BYTES)
                    .unwrap_or_else(|e| panic!("decompress {codec} lvl {level:?}: {e}"));
                assert_eq!(
                    back,
                    data,
                    "round-trip mismatch for {codec} (level {level:?}, {} bytes)",
                    data.len()
                );
            }
        }
    }
}

#[test]
fn empty_input_round_trips_to_empty() {
    for codec in CODECS {
        let packed = compress(codec, b"", None).unwrap();
        let back = decompress(codec, &packed, DEFAULT_MAX_OUTPUT_BYTES).unwrap();
        assert!(back.is_empty(), "{codec}: empty must round-trip to empty");
    }
}

#[test]
fn detect_resolves_magic_bearing_codecs() {
    let data = b"detect me: the quick brown fox".repeat(20);
    let expected = [
        ("zstd", "zstd"),
        ("gzip", "gzip"),
        ("zlib", "zlib"),
        ("lz4", "lz4"),
        ("snappy", "snappy"),
        ("xz", "xz"),
        ("lzma", "lzma"),
        ("bzip2", "bzip2"),
    ];
    for (codec, want) in expected {
        let packed = compress(codec, &data, None).unwrap();
        assert_eq!(
            detect_codec(&packed),
            Some(want),
            "{codec} should be detected as {want}"
        );
    }
}

#[test]
fn detect_returns_unknown_for_headerless_and_garbage() {
    let data = b"headerless payload".repeat(20);
    for codec in ["brotli", "deflate", "lz4_block", "snappy_raw"] {
        let packed = compress(codec, &data, None).unwrap();
        assert_eq!(
            detect_codec(&packed),
            None,
            "{codec} is headerless and must be 'unknown'"
        );
    }
    // Arbitrary bytes and a weak-magic near-miss (`78 xx` that is not zlib).
    assert_eq!(detect_codec(b"\x78\x00random non-zlib bytes here"), None);
    assert_eq!(detect_codec(b"not compressed at all"), None);
    assert_eq!(detect_codec(b""), None);
}

#[test]
fn decompress_auto_resolves_and_rejects() {
    let data = b"auto-detect this payload".repeat(50);
    // Magic-bearing: auto works.
    let packed = compress("gzip", &data, None).unwrap();
    assert_eq!(
        decompress_auto(&packed, DEFAULT_MAX_OUTPUT_BYTES).unwrap(),
        data
    );
    // Headerless: auto refuses with NoMagic.
    let raw = compress("brotli", &data, None).unwrap();
    assert!(matches!(
        decompress_auto(&raw, DEFAULT_MAX_OUTPUT_BYTES),
        Err(CodecError::NoMagic)
    ));
}

#[test]
fn wrong_variant_errors_cleanly() {
    let data = b"frame vs raw are not interchangeable".repeat(10);
    // lz4 frame bytes decoded as lz4_block (and vice versa) must error, not panic.
    let frame = compress("lz4", &data, None).unwrap();
    let block = compress("lz4_block", &data, None).unwrap();
    assert!(decompress("lz4_block", &frame, DEFAULT_MAX_OUTPUT_BYTES).is_err());
    assert!(decompress("lz4", &block, DEFAULT_MAX_OUTPUT_BYTES).is_err());

    // snappy framed vs raw.
    let s_frame = compress("snappy", &data, None).unwrap();
    let s_raw = compress("snappy_raw", &data, None).unwrap();
    assert!(decompress("snappy_raw", &s_frame, DEFAULT_MAX_OUTPUT_BYTES).is_err());
    assert!(decompress("snappy", &s_raw, DEFAULT_MAX_OUTPUT_BYTES).is_err());
}

#[test]
fn bomb_guard_aborts_at_cap_without_oom() {
    // 8 MiB of zeros — whatever its packed size, it decodes back to 8 MiB and
    // must abort at a tiny cap rather than OOM. (The strong codecs crush this to
    // a few KB; the pure-Rust lzma-rs xz/lzma encoder barely shrinks it, but the
    // DECODE-side guard is what this test asserts, independent of ratio.)
    let zeros = vec![0u8; 8 * 1024 * 1024];
    for codec in ["zstd", "gzip", "xz", "bzip2", "brotli", "lz4", "snappy"] {
        let packed = compress(codec, &zeros, None).unwrap();
        // Tiny explicit cap: decode must trip the guard.
        let tiny = decompress(codec, &packed, 4096);
        assert!(
            matches!(tiny, Err(CodecError::OutputTooLarge(4096))),
            "{codec}: expected OutputTooLarge at cap 4096, got {tiny:?}"
        );
        // A cap above the true size decodes fine, proving the scan survives.
        let ok = decompress(codec, &packed, 16 * 1024 * 1024).unwrap();
        assert_eq!(ok.len(), zeros.len());
    }
}

#[test]
fn malformed_input_never_panics_and_reports() {
    for codec in CODECS {
        for bad in [
            b"".as_slice(),
            b"\x00",
            b"not a valid stream at all, just text",
            b"\xff\xfe\xfd\xfc\xfb\xfa",
        ] {
            // Decompress: clean Result, never a panic.
            let _ = decompress(codec, bad, DEFAULT_MAX_OUTPUT_BYTES);
            // is_valid: total, never throws.
            let _ = is_valid(codec, bad);
        }
    }
}

#[test]
fn is_valid_true_for_real_streams_false_for_garbage() {
    let data = b"validate me".repeat(100);
    for codec in CODECS {
        let packed = compress(codec, &data, None).unwrap();
        assert!(is_valid(codec, &packed), "{codec}: real stream is valid");
    }
    assert!(!is_valid("gzip", b"definitely not gzip"));
    assert!(!is_valid("zstd", b"\x00\x01\x02"));
    assert!(!is_valid("nonexistent_codec", b"anything"));
}

#[test]
fn decompressed_size_and_ratio() {
    let data = b"abc".repeat(1000); // 3000 bytes, very compressible
    for codec in CODECS {
        let packed = compress(codec, &data, None).unwrap();
        let sz = decompressed_size(codec, &packed, DEFAULT_MAX_OUTPUT_BYTES).unwrap();
        assert_eq!(sz, Some(3000), "{codec}: decompressed_size");
        // ratio() must equal compressed_size / input_len exactly for every codec.
        let r = ratio(codec, &data, None).unwrap().unwrap();
        assert!(
            (r - packed.len() as f64 / 3000.0).abs() < 1e-12,
            "{codec}: ratio arithmetic"
        );
    }
    // The real compressors crush highly repetitive data well below 1.0. (xz/lzma
    // use the pure-Rust lzma-rs encoder, which barely compresses — that is the
    // documented default-backend trade-off, so they are excluded from the shrink
    // assertion; their round-trip and decompressed_size are still exact above.)
    for codec in [
        "zstd",
        "gzip",
        "zlib",
        "deflate",
        "brotli",
        "lz4",
        "lz4_block",
        "snappy",
        "snappy_raw",
        "bzip2",
    ] {
        let r = ratio(codec, &data, None).unwrap().unwrap();
        assert!(
            r < 1.0,
            "{codec}: repetitive data should shrink (ratio {r})"
        );
    }
    // Ratio on empty input is NULL.
    assert_eq!(ratio("zstd", b"", None).unwrap(), None);
}

#[test]
fn truncated_stream_is_not_too_large() {
    // A truncated valid stream must surface Corrupt, not OutputTooLarge.
    let data = b"truncate me".repeat(200);
    let packed = compress("xz", &data, None).unwrap();
    let truncated = &packed[..packed.len() / 2];
    match decompress("xz", truncated, DEFAULT_MAX_OUTPUT_BYTES) {
        Err(CodecError::Corrupt(_)) => {}
        other => panic!("expected Corrupt for a truncated xz stream, got {other:?}"),
    }
}
