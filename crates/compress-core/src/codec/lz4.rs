//! LZ4 via `lz4_flex` (pure Rust): the LZ4 **frame** (LZ4F) form and the raw
//! **block** form. They are not interchangeable — distinct codec names.

use std::io::Write;

use lz4_flex::frame::{FrameDecoder, FrameEncoder};

use crate::codec::drain_reader;
use crate::error::{CodecError, Result};

// --- frame (LZ4F) ----------------------------------------------------------

pub fn compress_frame(input: &[u8]) -> Result<Vec<u8>> {
    let mut enc = FrameEncoder::new(Vec::new());
    enc.write_all(input)
        .map_err(|e| CodecError::Corrupt(e.to_string()))?;
    enc.finish().map_err(|e| CodecError::Corrupt(e.to_string()))
}

pub fn decompress_frame(input: &[u8], cap: u64) -> Result<Vec<u8>> {
    drain_reader(FrameDecoder::new(input), cap)
}

// --- raw block -------------------------------------------------------------
//
// The raw block form carries no length, so we prepend the uncompressed size as
// a little-endian u32 (`compress_prepend_size`). On decode we read that prefix
// FIRST and reject a bomb before allocating, then decompress.

pub fn compress_block(input: &[u8]) -> Result<Vec<u8>> {
    Ok(lz4_flex::block::compress_prepend_size(input))
}

pub fn decompress_block(input: &[u8], cap: u64) -> Result<Vec<u8>> {
    // The 4-byte little-endian uncompressed-size prefix is the bomb-guard check:
    // refuse before decompressing if it exceeds the cap.
    if input.len() < 4 {
        return Err(CodecError::Corrupt(
            "lz4 block too short for size prefix".into(),
        ));
    }
    let declared = u32::from_le_bytes([input[0], input[1], input[2], input[3]]) as u64;
    if declared > cap {
        return Err(CodecError::OutputTooLarge(cap));
    }
    lz4_flex::block::decompress_size_prepended(input)
        .map_err(|e| CodecError::Corrupt(e.to_string()))
}
