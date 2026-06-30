//! The decompression-bomb guard.
//!
//! [`BoundedWriter`] is a counting [`std::io::Write`] sink that aborts the
//! moment a decoder would push the total output past a configured cap. A 1 KB
//! zstd / bzip2 / brotli blob can legally expand to many GB; without this guard
//! an unbounded `decompress` over a hostile column would OOM and kill the
//! worker. Instead the write fails, the decoder unwinds cleanly via `?`, and the
//! caller turns it into a per-row error — the scan keeps running.

use std::io::{self, Write};

/// A sentinel message stamped onto the `io::Error` the writer raises when the
/// cap is exceeded, so the decode wrapper can tell a bomb (→ `OutputTooLarge`)
/// apart from genuinely corrupt input (→ `Corrupt`). Also recorded as a flag.
pub const OVERFLOW_MSG: &str = "vgi-compress: output exceeds max_output_bytes";

/// A bounded, in-memory output sink. Collects decoded bytes into a `Vec` but
/// refuses to grow past `cap`; the `(cap + 1)`-th byte fails the write.
pub struct BoundedWriter {
    buf: Vec<u8>,
    cap: u64,
    written: u64,
    /// Set once the cap is exceeded, so the caller can distinguish a bomb from a
    /// corrupt-stream `io::Error` even after the error has been mapped.
    pub overflowed: bool,
}

impl BoundedWriter {
    /// A new sink that accepts at most `cap` bytes. Pre-reserves a small,
    /// cap-bounded buffer so a tiny expected output does not over-allocate.
    pub fn new(cap: u64) -> Self {
        // Reserve up to 64 KiB up front (never more than the cap) to avoid
        // repeated tiny re-allocations on the common small-output path.
        let reserve = cap.min(64 * 1024) as usize;
        BoundedWriter {
            buf: Vec::with_capacity(reserve),
            cap,
            written: 0,
            overflowed: false,
        }
    }

    /// The configured cap.
    pub fn cap(&self) -> u64 {
        self.cap
    }

    /// The number of bytes accepted so far.
    pub fn len(&self) -> u64 {
        self.written
    }

    /// Whether nothing has been written yet.
    pub fn is_empty(&self) -> bool {
        self.written == 0
    }

    /// Consume the writer and return the collected bytes.
    pub fn into_inner(self) -> Vec<u8> {
        self.buf
    }
}

impl Write for BoundedWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let next = self.written.saturating_add(data.len() as u64);
        if next > self.cap {
            self.overflowed = true;
            return Err(io::Error::new(io::ErrorKind::WriteZero, OVERFLOW_MSG));
        }
        self.buf.extend_from_slice(data);
        self.written = next;
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_up_to_cap() {
        let mut w = BoundedWriter::new(4);
        assert!(w.write_all(b"abcd").is_ok());
        assert_eq!(w.len(), 4);
        assert!(!w.overflowed);
        assert_eq!(w.into_inner(), b"abcd");
    }

    #[test]
    fn rejects_past_cap() {
        let mut w = BoundedWriter::new(3);
        let err = w.write_all(b"abcd").unwrap_err();
        assert_eq!(err.to_string(), OVERFLOW_MSG);
        assert!(w.overflowed);
    }

    #[test]
    fn rejects_in_a_single_oversized_write() {
        let mut w = BoundedWriter::new(2);
        assert!(w.write(b"xyz").is_err());
        assert!(w.overflowed);
    }
}
