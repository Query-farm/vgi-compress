//! xz (LZMA2 container) and legacy `.lzma` (alone) streams.
//!
//! Default backend: pure-Rust `lzma-rs` (MIT, no C, smaller trust surface).
//! With `--features liblzma`, xz routes to the C `xz-utils` `liblzma` FFI for
//! throughput; the legacy `.lzma` form always uses `lzma-rs`. `codecs()` reports
//! which backend is compiled in.
//!
//! NOTE: the pure-Rust `lzma-rs` backend does not expose a compression-level
//! knob, so `level` is accepted and clamped but is a no-op there; the stream is
//! still a fully valid, round-trippable xz / lzma container.

use crate::codec::finish_sink;
use crate::error::{CodecError, Result};
use crate::guard::BoundedWriter;

// --- xz (LZMA2 container) ---------------------------------------------------

#[cfg(not(feature = "liblzma"))]
pub fn compress_xz(input: &[u8], _lvl: i32) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut src = input;
    lzma_rs::xz_compress(&mut src, &mut out).map_err(|e| CodecError::Corrupt(e.to_string()))?;
    Ok(out)
}

#[cfg(not(feature = "liblzma"))]
pub fn decompress_xz(input: &[u8], cap: u64) -> Result<Vec<u8>> {
    let mut sink = BoundedWriter::new(cap);
    let mut src = input;
    let res = lzma_rs::xz_decompress(&mut src, &mut sink);
    finish_sink(sink, res, cap)
}

#[cfg(feature = "liblzma")]
pub fn compress_xz(input: &[u8], lvl: i32) -> Result<Vec<u8>> {
    use std::io::Read;
    let preset = lvl.clamp(0, 9) as u32;
    let mut enc = liblzma::read::XzEncoder::new(input, preset);
    let mut out = Vec::new();
    enc.read_to_end(&mut out)
        .map_err(|e| CodecError::Corrupt(e.to_string()))?;
    Ok(out)
}

#[cfg(feature = "liblzma")]
pub fn decompress_xz(input: &[u8], cap: u64) -> Result<Vec<u8>> {
    crate::codec::drain_reader(liblzma::read::XzDecoder::new(input), cap)
}

// --- legacy `.lzma` (alone) stream -----------------------------------------
// Always uses the pure-Rust `lzma-rs` backend (the C path's alone format is less
// commonly needed; keep one well-tested implementation).

pub fn compress_lzma(input: &[u8], _lvl: i32) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut src = input;
    lzma_rs::lzma_compress(&mut src, &mut out).map_err(|e| CodecError::Corrupt(e.to_string()))?;
    Ok(out)
}

pub fn decompress_lzma(input: &[u8], cap: u64) -> Result<Vec<u8>> {
    let mut sink = BoundedWriter::new(cap);
    let mut src = input;
    let res = lzma_rs::lzma_decompress(&mut src, &mut sink);
    finish_sink(sink, res, cap)
}
