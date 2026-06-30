//! Resolving the decompression-bomb output cap (`max_output_bytes`).
//!
//! Precedence (first match wins):
//!   1. an explicit non-NULL `max_output_bytes` argument on the call;
//!   2. the `compress_max_output_bytes` ATTACH option / DuckDB setting;
//!   3. the `VGI_COMPRESS_MAX_OUTPUT_BYTES` environment variable;
//!   4. the built-in default, [`compress_core::DEFAULT_MAX_OUTPUT_BYTES`] (256 MiB).

use vgi::ProcessParams;

/// Resolve the effective output cap for a decode call. `per_call` is the
/// argument value for this row (`None` if the argument was absent or NULL).
pub fn resolve_cap(per_call: Option<i64>, params: &ProcessParams) -> u64 {
    if let Some(v) = per_call {
        if v > 0 {
            return v as u64;
        }
    }
    if let Some(v) = params.settings.get_i64("compress_max_output_bytes") {
        if v > 0 {
            return v as u64;
        }
    }
    if let Ok(s) = std::env::var("VGI_COMPRESS_MAX_OUTPUT_BYTES") {
        if let Ok(v) = s.trim().parse::<u64>() {
            if v > 0 {
                return v;
            }
        }
    }
    compress_core::DEFAULT_MAX_OUTPUT_BYTES
}

/// Narrow an `i64` level argument to the `i32` the engine expects, saturating
/// rather than wrapping (the engine clamps to each codec's range anyway).
pub fn level_i32(level: Option<i64>) -> Option<i32> {
    level.map(|v| v.clamp(i32::MIN as i64, i32::MAX as i64) as i32)
}
