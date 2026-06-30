//! Brotli (RFC 7932) via the pure-Rust `brotli` crate. Headerless — no magic
//! bytes, so `detect_codec` cannot identify it (callers use explicit codec).

use std::io::Write;

use crate::codec::drain_reader;
use crate::error::{CodecError, Result};

/// Default brotli window (lg) — 22 is the format's common maximum window.
const LGWIN: u32 = 22;
/// Internal streaming buffer size for the encoder/decoder.
const BUF: usize = 8 * 1024;

pub fn compress(input: &[u8], lvl: i32) -> Result<Vec<u8>> {
    let quality = lvl.clamp(0, 11) as u32;
    let mut enc = brotli::CompressorWriter::new(Vec::new(), BUF, quality, LGWIN);
    enc.write_all(input)
        .map_err(|e| CodecError::Corrupt(e.to_string()))?;
    enc.flush()
        .map_err(|e| CodecError::Corrupt(e.to_string()))?;
    Ok(enc.into_inner())
}

pub fn decompress(input: &[u8], cap: u64) -> Result<Vec<u8>> {
    drain_reader(brotli::Decompressor::new(input, BUF), cap)
}
