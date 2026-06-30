//! The codec registry and dispatch: parse a codec name, clamp a level into the
//! codec's range, and route compress / decompress to the backing crate.
//!
//! Every decompress path threads through the [`crate::guard::BoundedWriter`] so
//! a decompression bomb aborts at the cap instead of OOMing the worker.

use std::io::{self, Read};

use crate::error::{CodecError, Result};
use crate::guard::BoundedWriter;

pub mod brotli;
pub mod bzip2;
pub mod flate;
pub mod lz4;
pub mod snappy;
pub mod xz;
pub mod zstd;

/// One supported (de)compression codec. Frame-vs-raw variants are **distinct**
/// codecs because their byte layout differs and a uniform API must not silently
/// guess between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Codec {
    /// Zstandard (RFC 8478), always framed. Magic `28 b5 2f fd`.
    Zstd,
    /// gzip member (RFC 1952): CRC32 + ISIZE trailer. Magic `1f 8b`.
    Gzip,
    /// zlib stream (RFC 1950): 2-byte header + Adler-32. Magic `78 xx`.
    Zlib,
    /// Raw DEFLATE (RFC 1951): no header/trailer. Headerless.
    Deflate,
    /// Brotli (RFC 7932). Headerless (no magic bytes).
    Brotli,
    /// LZ4 frame (LZ4F). Magic `04 22 4d 18`.
    Lz4,
    /// LZ4 raw block, no frame header. Length-prefixed by this worker. Headerless.
    Lz4Block,
    /// Snappy framed (stream format). Magic `ff 06 00 00 73 4e 61 50 70 59`.
    Snappy,
    /// Snappy raw block (varint length prefix). Headerless.
    SnappyRaw,
    /// xz container (LZMA2 + CRC). Magic `fd 37 7a 58 5a 00`.
    Xz,
    /// Legacy `.lzma` (alone) stream. Weak magic `5d 00 00`.
    Lzma,
    /// bzip2 stream. Magic `42 5a 68` ("BZh").
    Bzip2,
}

/// Every codec this build supports, in a stable, documented order. Reflects
/// compiled-in features (the xz backend label is reported by [`xz_backend`]).
pub const ALL: &[Codec] = &[
    Codec::Zstd,
    Codec::Gzip,
    Codec::Zlib,
    Codec::Deflate,
    Codec::Brotli,
    Codec::Lz4,
    Codec::Lz4Block,
    Codec::Snappy,
    Codec::SnappyRaw,
    Codec::Xz,
    Codec::Lzma,
    Codec::Bzip2,
];

/// Which xz/lzma backend is compiled in: `"liblzma"` (the C `xz-utils` FFI,
/// `--features liblzma`) or `"lzma-rs"` (the pure-Rust default). Surfaced by the
/// worker's `codecs()` so SQL can discover the build.
pub fn xz_backend() -> &'static str {
    if cfg!(feature = "liblzma") {
        "liblzma"
    } else {
        "lzma-rs"
    }
}

impl Codec {
    /// Parse a codec name (case-insensitive; `-` and `_` are interchangeable).
    pub fn parse(name: &str) -> Result<Codec> {
        let n = name.trim().to_ascii_lowercase().replace('-', "_");
        Ok(match n.as_str() {
            "zstd" | "zstandard" | "zst" => Codec::Zstd,
            "gzip" | "gz" => Codec::Gzip,
            "zlib" => Codec::Zlib,
            "deflate" | "raw_deflate" => Codec::Deflate,
            "brotli" | "br" => Codec::Brotli,
            "lz4" | "lz4_frame" | "lz4frame" => Codec::Lz4,
            "lz4_block" | "lz4block" | "lz4_raw" => Codec::Lz4Block,
            "snappy" | "snappy_framed" | "snappy_frame" => Codec::Snappy,
            "snappy_raw" | "snappy_block" => Codec::SnappyRaw,
            "xz" => Codec::Xz,
            "lzma" | "lzma1" | "alone" => Codec::Lzma,
            "bzip2" | "bz2" | "bzip" => Codec::Bzip2,
            _ => return Err(CodecError::UnknownCodec(name.to_string())),
        })
    }

    /// The canonical lowercase name (the inverse of the primary [`Codec::parse`]
    /// spelling).
    pub fn name(&self) -> &'static str {
        match self {
            Codec::Zstd => "zstd",
            Codec::Gzip => "gzip",
            Codec::Zlib => "zlib",
            Codec::Deflate => "deflate",
            Codec::Brotli => "brotli",
            Codec::Lz4 => "lz4",
            Codec::Lz4Block => "lz4_block",
            Codec::Snappy => "snappy",
            Codec::SnappyRaw => "snappy_raw",
            Codec::Xz => "xz",
            Codec::Lzma => "lzma",
            Codec::Bzip2 => "bzip2",
        }
    }

    /// Whether this codec takes a compression level at all (`snappy*` do not).
    pub fn has_levels(&self) -> bool {
        self.level_range().is_some()
    }

    /// The codec's default compression level, or `None` for the level-less
    /// codecs (`snappy`, `snappy_raw`).
    pub fn default_level(&self) -> Option<i32> {
        match self {
            Codec::Zstd => Some(3),
            Codec::Gzip | Codec::Zlib | Codec::Deflate => Some(6),
            Codec::Brotli => Some(11),
            Codec::Lz4 | Codec::Lz4Block => Some(0),
            Codec::Xz | Codec::Lzma => Some(6),
            Codec::Bzip2 => Some(9),
            Codec::Snappy | Codec::SnappyRaw => None,
        }
    }

    /// The inclusive `(min, max)` level range, or `None` for level-less codecs.
    pub fn level_range(&self) -> Option<(i32, i32)> {
        match self {
            // zstd accepts negative "fast" levels down to -22.
            Codec::Zstd => Some((-22, 22)),
            Codec::Gzip | Codec::Zlib | Codec::Deflate => Some((0, 9)),
            Codec::Brotli => Some((0, 11)),
            Codec::Lz4 | Codec::Lz4Block => Some((0, 16)),
            Codec::Xz | Codec::Lzma => Some((0, 9)),
            Codec::Bzip2 => Some((1, 9)),
            Codec::Snappy | Codec::SnappyRaw => None,
        }
    }

    /// Resolve the effective level for a compress call. `None` → the codec
    /// default; an out-of-range value clamps to the codec's range. Returns
    /// `(level, clamped)`; `clamped` is `true` when the requested level was
    /// outside the range (the caller may log a non-fatal warning). Level-less
    /// codecs always return `(0, false)`.
    pub fn resolve_level(&self, requested: Option<i32>) -> (i32, bool) {
        let Some((lo, hi)) = self.level_range() else {
            return (0, false);
        };
        match requested {
            None => (self.default_level().unwrap_or(lo), false),
            Some(v) if v < lo => (lo, true),
            Some(v) if v > hi => (hi, true),
            Some(v) => (v, false),
        }
    }
}

/// Compress `input` with `codec` at `level` (resolved/clamped via
/// [`Codec::resolve_level`]). Returns the compressed bytes in the codec's
/// canonical container. Empty input yields the codec's valid empty stream (which
/// round-trips back to empty).
pub fn compress(codec: Codec, input: &[u8], level: Option<i32>) -> Result<Vec<u8>> {
    let (lvl, _clamped) = codec.resolve_level(level);
    match codec {
        Codec::Zstd => zstd::compress(input, lvl),
        Codec::Gzip => flate::compress_gzip(input, lvl),
        Codec::Zlib => flate::compress_zlib(input, lvl),
        Codec::Deflate => flate::compress_deflate(input, lvl),
        Codec::Brotli => brotli::compress(input, lvl),
        Codec::Lz4 => lz4::compress_frame(input),
        Codec::Lz4Block => lz4::compress_block(input),
        Codec::Snappy => snappy::compress_framed(input),
        Codec::SnappyRaw => snappy::compress_raw(input),
        Codec::Xz => xz::compress_xz(input, lvl),
        Codec::Lzma => xz::compress_lzma(input, lvl),
        Codec::Bzip2 => bzip2::compress(input, lvl),
    }
}

/// Decompress `input` with `codec`, capping output at `max_output` bytes (the
/// decompression-bomb guard). Malformed / truncated / wrong-codec input is a
/// clean [`CodecError::Corrupt`]; exceeding the cap is
/// [`CodecError::OutputTooLarge`]. Never panics.
pub fn decompress(codec: Codec, input: &[u8], max_output: u64) -> Result<Vec<u8>> {
    match codec {
        Codec::Zstd => zstd::decompress(input, max_output),
        Codec::Gzip => flate::decompress_gzip(input, max_output),
        Codec::Zlib => flate::decompress_zlib(input, max_output),
        Codec::Deflate => flate::decompress_deflate(input, max_output),
        Codec::Brotli => brotli::decompress(input, max_output),
        Codec::Lz4 => lz4::decompress_frame(input, max_output),
        Codec::Lz4Block => lz4::decompress_block(input, max_output),
        Codec::Snappy => snappy::decompress_framed(input, max_output),
        Codec::SnappyRaw => snappy::decompress_raw(input, max_output),
        Codec::Xz => xz::decompress_xz(input, max_output),
        Codec::Lzma => xz::decompress_lzma(input, max_output),
        Codec::Bzip2 => bzip2::decompress(input, max_output),
    }
}

// --- shared decode plumbing ------------------------------------------------

/// Drain a streaming decoder (`Read`) into a [`BoundedWriter`], turning a cap
/// overflow into [`CodecError::OutputTooLarge`] and any other I/O failure into
/// [`CodecError::Corrupt`]. Used by every `Read`-based codec backend.
pub(crate) fn drain_reader<R: Read>(mut reader: R, cap: u64) -> Result<Vec<u8>> {
    let mut sink = BoundedWriter::new(cap);
    match io::copy(&mut reader, &mut sink) {
        Ok(_) => Ok(sink.into_inner()),
        Err(_) if sink.overflowed => Err(CodecError::OutputTooLarge(cap)),
        Err(e) => Err(CodecError::Corrupt(e.to_string())),
    }
}

/// Finish a `Write`-based decoder: inspect the writer's overflow flag to map the
/// failure, otherwise return the collected bytes. Used by the codec backends
/// (lzma-rs) that write into a sink rather than expose a `Read`.
pub(crate) fn finish_sink(
    sink: BoundedWriter,
    res: std::result::Result<(), impl std::fmt::Display>,
    cap: u64,
) -> Result<Vec<u8>> {
    match res {
        Ok(()) => Ok(sink.into_inner()),
        Err(_) if sink.overflowed => Err(CodecError::OutputTooLarge(cap)),
        Err(e) => Err(CodecError::Corrupt(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_is_case_and_separator_insensitive() {
        assert_eq!(Codec::parse("ZSTD").unwrap(), Codec::Zstd);
        assert_eq!(Codec::parse("lz4-block").unwrap(), Codec::Lz4Block);
        assert_eq!(Codec::parse(" Snappy_Raw ").unwrap(), Codec::SnappyRaw);
        assert!(Codec::parse("nope").is_err());
    }

    #[test]
    fn level_clamps_and_defaults() {
        assert_eq!(Codec::Gzip.resolve_level(None), (6, false));
        assert_eq!(Codec::Gzip.resolve_level(Some(99)), (9, true));
        assert_eq!(Codec::Gzip.resolve_level(Some(-1)), (0, true));
        assert_eq!(Codec::Snappy.resolve_level(Some(5)), (0, false));
        assert_eq!(Codec::Zstd.resolve_level(Some(-5)), (-5, false));
    }
}
