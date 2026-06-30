//! Codec auto-detection by magic bytes, with a trial-decode confirmation for the
//! weak/short signatures.
//!
//! Only **magic-bearing** codecs can be detected: zstd, gzip, zlib, xz, lzma,
//! bzip2, lz4-frame, snappy-framed. The headerless codecs — brotli, raw
//! deflate, lz4_block, snappy_raw — carry no signature and always return
//! `None` (the worker surfaces `'unknown'`). This is a documented limitation:
//! auto-detection is magic-byte-only and best-effort.

use crate::codec::Codec;
use crate::error::CodecError;

/// Trial-decode cap for confirming a weak-magic hit: enough to decode the
/// leading block, bounded so confirmation is cheap and bomb-safe.
const TRIAL_CAP: u64 = 1 << 20; // 1 MiB

/// Detect the codec of `input` by magic bytes. Signatures are checked
/// strongest-first (longest / most unique). Weak/short signatures (`zlib`,
/// `lzma`) are confirmed with a bounded trial decode so a chance `78 xx` or
/// `5d 00 00` in arbitrary bytes is rejected rather than mis-reported. Returns
/// `None` for headerless codecs and for input that matches nothing. Never errors.
pub fn detect(input: &[u8]) -> Option<Codec> {
    // Snappy framed: stream identifier chunk `ff 06 00 00 73 4e 61 50 70 59`.
    if input.starts_with(&[0xff, 0x06, 0x00, 0x00, 0x73, 0x4e, 0x61, 0x50, 0x70, 0x59]) {
        return Some(Codec::Snappy);
    }
    // xz: `fd 37 7a 58 5a 00`.
    if input.starts_with(&[0xfd, 0x37, 0x7a, 0x58, 0x5a, 0x00]) {
        return Some(Codec::Xz);
    }
    // lz4 frame: `04 22 4d 18`.
    if input.starts_with(&[0x04, 0x22, 0x4d, 0x18]) {
        return Some(Codec::Lz4);
    }
    // zstd: `28 b5 2f fd`.
    if input.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
        return Some(Codec::Zstd);
    }
    // bzip2: "BZh" then a block-size digit '1'..'9'.
    if input.len() >= 4 && &input[0..3] == b"BZh" && (b'1'..=b'9').contains(&input[3]) {
        return Some(Codec::Bzip2);
    }
    // gzip: `1f 8b` (checked before zlib; both are flate families).
    if input.starts_with(&[0x1f, 0x8b]) {
        return Some(Codec::Gzip);
    }
    // zlib (weak): CMF/FLG must be a valid deflate header AND the FCHECK mod-31
    // must hold; then confirm with a trial decode.
    if looks_like_zlib(input) && trial_ok(Codec::Zlib, input) {
        return Some(Codec::Zlib);
    }
    // lzma alone (weak `5d 00 00`): confirm with a trial decode.
    if input.len() >= 13 && input[0] == 0x5d && trial_ok(Codec::Lzma, input) {
        return Some(Codec::Lzma);
    }
    None
}

/// Cheap structural check for a zlib (RFC 1950) header: CM == 8 (deflate),
/// CINFO <= 7, and the 2-byte header is a multiple of 31.
fn looks_like_zlib(input: &[u8]) -> bool {
    if input.len() < 2 {
        return false;
    }
    let (cmf, flg) = (input[0], input[1]);
    let cm = cmf & 0x0f;
    let cinfo = cmf >> 4;
    let check = ((cmf as u16) << 8) | flg as u16;
    cm == 8 && cinfo <= 7 && check.is_multiple_of(31)
}

/// True if a bounded trial decode of `input` as `codec` produces a valid stream
/// (full decode within the trial cap, or hitting the cap — both mean the bytes
/// really are this codec). A corrupt/truncated trial rejects the guess.
fn trial_ok(codec: Codec, input: &[u8]) -> bool {
    match crate::codec::decompress(codec, input, TRIAL_CAP) {
        Ok(_) => true,
        Err(CodecError::OutputTooLarge(_)) => true,
        Err(_) => false,
    }
}
