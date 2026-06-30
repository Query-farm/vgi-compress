//! The per-row error taxonomy for the (de)compression engine.
//!
//! Every decode path returns a [`CodecError`] rather than panicking, so a
//! hostile or malformed blob fails its own row and never crashes the scan. The
//! worker maps these onto per-row SQL errors (or NULL / FALSE, depending on the
//! function's contract).

use std::fmt;

/// An error from a compress / decompress / detect operation. Each variant maps
/// to a clear, stable per-row message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodecError {
    /// The `codec` string is not one this build supports.
    UnknownCodec(String),
    /// The decoded output would exceed the configured `max_output_bytes` cap —
    /// the decompression-bomb guard tripped. Carries the cap that was hit.
    OutputTooLarge(u64),
    /// The input is not a well-formed stream for the codec (truncated, trailing
    /// garbage, wrong codec, corrupt). Carries a short reason.
    Corrupt(String),
    /// `decompress_auto` could not identify the codec by magic bytes (the input
    /// is a headerless codec — brotli / deflate / lz4_block / snappy_raw — or
    /// simply not compressed).
    NoMagic,
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CodecError::UnknownCodec(name) => {
                write!(f, "unknown codec '{name}'")
            }
            CodecError::OutputTooLarge(cap) => {
                write!(f, "output exceeds max_output_bytes ({cap})")
            }
            CodecError::Corrupt(why) => write!(f, "corrupt or truncated input: {why}"),
            CodecError::NoMagic => {
                write!(f, "cannot auto-detect codec (no magic bytes)")
            }
        }
    }
}

impl std::error::Error for CodecError {}

/// Convenient result alias for the engine.
pub type Result<T> = std::result::Result<T, CodecError>;
