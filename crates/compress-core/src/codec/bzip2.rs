//! bzip2 via the `bzip2` crate (pure-Rust `libbz2-rs-sys` default backend).

use std::io::Write;

use bzip2::read::BzDecoder;
use bzip2::write::BzEncoder;
use bzip2::Compression;

use crate::codec::drain_reader;
use crate::error::{CodecError, Result};

pub fn compress(input: &[u8], lvl: i32) -> Result<Vec<u8>> {
    let level = Compression::new(lvl.clamp(1, 9) as u32);
    let mut enc = BzEncoder::new(Vec::new(), level);
    enc.write_all(input)
        .map_err(|e| CodecError::Corrupt(e.to_string()))?;
    enc.finish().map_err(|e| CodecError::Corrupt(e.to_string()))
}

pub fn decompress(input: &[u8], cap: u64) -> Result<Vec<u8>> {
    drain_reader(BzDecoder::new(input), cap)
}
