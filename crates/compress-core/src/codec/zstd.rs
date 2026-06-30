//! zstd via the `zstd` crate (bundled libzstd, used under its permissive BSD
//! option). Streaming decode through the bomb guard.

use crate::codec::drain_reader;
use crate::error::{CodecError, Result};

pub fn compress(input: &[u8], lvl: i32) -> Result<Vec<u8>> {
    zstd::stream::encode_all(input, lvl).map_err(|e| CodecError::Corrupt(e.to_string()))
}

pub fn decompress(input: &[u8], cap: u64) -> Result<Vec<u8>> {
    // `zstd::stream::read::Decoder` is a `Read` over the frame; draining it into
    // the BoundedWriter enforces the cap regardless of the frame's advertised
    // content size.
    let decoder =
        zstd::stream::read::Decoder::new(input).map_err(|e| CodecError::Corrupt(e.to_string()))?;
    drain_reader(decoder, cap)
}
