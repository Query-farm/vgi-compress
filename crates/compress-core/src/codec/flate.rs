//! gzip / zlib / raw-deflate via `flate2` (miniz_oxide pure-Rust backend).

use std::io::Write;

use flate2::read::{DeflateDecoder, GzDecoder, ZlibDecoder};
use flate2::write::{DeflateEncoder, GzEncoder, ZlibEncoder};
use flate2::Compression;

use crate::codec::drain_reader;
use crate::error::{CodecError, Result};

fn level(l: i32) -> Compression {
    Compression::new(l.clamp(0, 9) as u32)
}

fn ce(e: impl std::fmt::Display) -> CodecError {
    CodecError::Corrupt(e.to_string())
}

pub fn compress_gzip(input: &[u8], lvl: i32) -> Result<Vec<u8>> {
    let mut enc = GzEncoder::new(Vec::new(), level(lvl));
    enc.write_all(input).map_err(ce)?;
    enc.finish().map_err(ce)
}

pub fn compress_zlib(input: &[u8], lvl: i32) -> Result<Vec<u8>> {
    let mut enc = ZlibEncoder::new(Vec::new(), level(lvl));
    enc.write_all(input).map_err(ce)?;
    enc.finish().map_err(ce)
}

pub fn compress_deflate(input: &[u8], lvl: i32) -> Result<Vec<u8>> {
    let mut enc = DeflateEncoder::new(Vec::new(), level(lvl));
    enc.write_all(input).map_err(ce)?;
    enc.finish().map_err(ce)
}

pub fn decompress_gzip(input: &[u8], cap: u64) -> Result<Vec<u8>> {
    drain_reader(GzDecoder::new(input), cap)
}

pub fn decompress_zlib(input: &[u8], cap: u64) -> Result<Vec<u8>> {
    drain_reader(ZlibDecoder::new(input), cap)
}

pub fn decompress_deflate(input: &[u8], cap: u64) -> Result<Vec<u8>> {
    drain_reader(DeflateDecoder::new(input), cap)
}

/// Read the gzip ISIZE trailer (uncompressed size mod 2^32) without decoding.
/// `None` for a stream too short to carry the 4-byte trailer.
pub fn gzip_isize(input: &[u8]) -> Option<u64> {
    if input.len() < 8 {
        return None;
    }
    let n = input.len();
    let isize = u32::from_le_bytes([input[n - 4], input[n - 3], input[n - 2], input[n - 1]]);
    Some(isize as u64)
}
