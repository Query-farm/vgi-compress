//! Snappy via `snap` (pure Rust): the **framed** stream format and the **raw**
//! block form. Distinct codec names — Kafka/Hadoop ship both in the wild.

use std::io::Write;

use crate::codec::drain_reader;
use crate::error::{CodecError, Result};

// --- framed (stream format) ------------------------------------------------

pub fn compress_framed(input: &[u8]) -> Result<Vec<u8>> {
    let mut enc = snap::write::FrameEncoder::new(Vec::new());
    enc.write_all(input)
        .map_err(|e| CodecError::Corrupt(e.to_string()))?;
    enc.into_inner()
        .map_err(|e| CodecError::Corrupt(e.to_string()))
}

pub fn decompress_framed(input: &[u8], cap: u64) -> Result<Vec<u8>> {
    drain_reader(snap::read::FrameDecoder::new(input), cap)
}

// --- raw block -------------------------------------------------------------

pub fn compress_raw(input: &[u8]) -> Result<Vec<u8>> {
    snap::raw::Encoder::new()
        .compress_vec(input)
        .map_err(|e| CodecError::Corrupt(e.to_string()))
}

pub fn decompress_raw(input: &[u8], cap: u64) -> Result<Vec<u8>> {
    // Snappy raw stores the uncompressed length as a leading varint; read it
    // first and reject a bomb before allocating the output buffer.
    match snap::raw::decompress_len(input) {
        Ok(len) if len as u64 > cap => return Err(CodecError::OutputTooLarge(cap)),
        Ok(_) => {}
        Err(e) => return Err(CodecError::Corrupt(e.to_string())),
    }
    snap::raw::Decoder::new()
        .decompress_vec(input)
        .map_err(|e| CodecError::Corrupt(e.to_string()))
}
