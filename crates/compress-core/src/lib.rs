//! `compress-core` — the pure-compute multi-codec (de)compression engine behind
//! the `compress` VGI worker.
//!
//! This crate carries **no** Arrow or VGI dependency: it operates on `&[u8]` in
//! and `Vec<u8>` / numbers out, so it is independently testable and reusable.
//! The worker crate (`compress-worker`) maps these results onto DuckDB's Arrow
//! type system.
//!
//! # Codecs
//!
//! `zstd`, `gzip`, `zlib`, `deflate` (raw), `brotli`, `lz4` (frame),
//! `lz4_block`, `snappy` (framed), `snappy_raw`, `xz`, `lzma`, `bzip2`. Frame
//! and raw variants are distinct codec names because their byte layout differs
//! and a uniform API must not silently guess between them.
//!
//! # Untrusted-input discipline (the load-bearing part)
//!
//! Every decode path is bounded by [`guard::BoundedWriter`]: a decompression
//! bomb (a 1 KB blob that legally expands to many GB) aborts the **one row** at
//! the `max_output_bytes` cap with [`CodecError::OutputTooLarge`] — the worker
//! never OOMs. Truncated / garbage / wrong-codec input is a clean
//! [`CodecError::Corrupt`], never a panic. Zero panics on arbitrary/truncated
//! bytes is a proptest gate (`tests/fuzz.rs`).

pub mod codec;
pub mod detect;
pub mod error;
pub mod guard;

pub use codec::Codec;
pub use error::{CodecError, Result};

/// The default decompression-bomb cap when a call passes no explicit
/// `max_output_bytes` and the worker has no `compress_max_output_bytes`
/// ATTACH-option / env override: **256 MiB**.
pub const DEFAULT_MAX_OUTPUT_BYTES: u64 = 256 * 1024 * 1024;

/// The engine (and worker) semantic version string.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// The codec names this build supports, in stable order. Reflects compiled-in
/// features; the active xz backend is reported separately by [`xz_backend`].
pub fn codecs() -> Vec<&'static str> {
    codec::ALL.iter().map(|c| c.name()).collect()
}

/// Which xz/lzma backend is compiled in (`"lzma-rs"` or `"liblzma"`).
pub fn xz_backend() -> &'static str {
    codec::xz_backend()
}

/// Compress `input` with the named `codec` (case-insensitive). `level` `None` →
/// codec default; out-of-range clamps to the codec's range; ignored for
/// level-less codecs. Empty input yields the codec's valid empty stream.
pub fn compress(codec: &str, input: &[u8], level: Option<i32>) -> Result<Vec<u8>> {
    codec::compress(Codec::parse(codec)?, input, level)
}

/// Decompress `input` with the named `codec`, capping output at `max_output`
/// bytes. The inverse of [`compress`].
pub fn decompress(codec: &str, input: &[u8], max_output: u64) -> Result<Vec<u8>> {
    codec::decompress(Codec::parse(codec)?, input, max_output)
}

/// Detect the codec by magic bytes, then decompress. Resolves only
/// magic-bearing codecs; headerless codecs (brotli / deflate / lz4_block /
/// snappy_raw) yield [`CodecError::NoMagic`] — use the explicit form.
pub fn decompress_auto(input: &[u8], max_output: u64) -> Result<Vec<u8>> {
    match detect::detect(input) {
        Some(c) => codec::decompress(c, input, max_output),
        None => Err(CodecError::NoMagic),
    }
}

/// The codec name detected by magic bytes, or `None` (`'unknown'`) when no
/// signature matches — which includes every headerless codec. Never errors.
pub fn detect_codec(input: &[u8]) -> Option<&'static str> {
    detect::detect(input).map(|c| c.name())
}

/// Byte length of `compress(input, codec, level)`. For "how small would this
/// get?" audits.
pub fn compressed_size(codec: &str, input: &[u8], level: Option<i32>) -> Result<u64> {
    Ok(compress(codec, input, level)?.len() as u64)
}

/// Decompressed byte length. For `gzip` the ISIZE trailer is read directly and
/// cheaply (note: gzip records the size mod 2^32); otherwise the stream is
/// counted under the bomb guard. `None` when a full decode would exceed `cap`.
pub fn decompressed_size(codec: &str, input: &[u8], cap: u64) -> Result<Option<u64>> {
    let c = Codec::parse(codec)?;
    if c == Codec::Gzip {
        if let Some(n) = codec::flate::gzip_isize(input) {
            if n <= cap {
                return Ok(Some(n));
            }
        }
    }
    match codec::decompress(c, input, cap) {
        Ok(v) => Ok(Some(v.len() as u64)),
        Err(CodecError::OutputTooLarge(_)) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Output-over-input compression ratio: `compressed_size / length(input)`, so
/// `< 1.0` means it shrank. `None` on empty input.
pub fn ratio(codec: &str, input: &[u8], level: Option<i32>) -> Result<Option<f64>> {
    if input.is_empty() {
        return Ok(None);
    }
    let out = compressed_size(codec, input, level)?;
    Ok(Some(out as f64 / input.len() as f64))
}

/// `true` iff `input` is a well-formed stream for `codec` (trial decode under
/// the default bomb guard, output discarded). Never errors — corrupt input, or
/// a stream that would exceed the default cap, is `false`, not a throw.
pub fn is_valid(codec: &str, input: &[u8]) -> bool {
    match Codec::parse(codec) {
        Ok(c) => codec::decompress(c, input, DEFAULT_MAX_OUTPUT_BYTES).is_ok(),
        Err(_) => false,
    }
}
