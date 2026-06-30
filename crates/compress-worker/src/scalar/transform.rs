//! The core (de)compression scalars over a BLOB column: `compress`,
//! `decompress`, `decompress_auto`.
//!
//! Each optional trailing argument (`level` on `compress`, `max_output_bytes` on
//! the decode functions) ships as a 2-arg and a 3-arg (resp. 1-arg / 2-arg)
//! arity overload, because DuckDB binds a registered signature by arity. The
//! `blob` and `codec` arguments are read per row, so SQL literals and columns
//! both work.

use arrow_array::{ArrayRef, RecordBatch};
use arrow_schema::DataType;
use compress_core::CodecError;
use vgi::{
    ArgSpec, BindParams, BindResponse, FunctionExample, FunctionMetadata, ProcessParams,
    ScalarFunction,
};
use vgi_rpc::{Result, RpcError};

use crate::arrow_io::{binary_array, blob_bytes, int_val, text_str};
use crate::config::{level_i32, resolve_cap};

/// Map an engine error to a per-row DuckDB error. The worker never panics; a
/// hostile/corrupt blob or a tripped bomb guard surfaces here as a clean error
/// string, and the scan moves on.
fn codec_err(e: CodecError) -> RpcError {
    RpcError::value_error(e.to_string())
}

fn require_codec(col: &ArrayRef, i: usize) -> Result<&str> {
    text_str(col, i)?.ok_or_else(|| RpcError::value_error("codec must not be NULL"))
}

// --- compress --------------------------------------------------------------

pub struct Compress {
    /// Whether this overload accepts the optional positional `level` argument.
    pub with_level: bool,
}

impl ScalarFunction for Compress {
    fn name(&self) -> &str {
        "compress"
    }

    fn metadata(&self) -> FunctionMetadata {
        let example = if self.with_level {
            FunctionExample {
                sql: "SELECT compress.main.compress('hello'::BLOB, 'zstd', 19);".into(),
                description: "Compress a blob with zstd at level 19.".into(),
                expected_output: None,
            }
        } else {
            FunctionExample {
                sql: "SELECT compress.main.compress('hello'::BLOB, 'gzip');".into(),
                description: "Compress a blob with gzip at the default level.".into(),
                expected_output: None,
            }
        };
        let mut tags = crate::meta::object_tags(
            "Compress a BLOB",
            "Compress a BLOB with the named codec (case-insensitive): zstd, gzip, zlib, deflate, \
             brotli, lz4, lz4_block, snappy, snappy_raw, xz, lzma, bzip2. The optional third \
             argument is the compression level — NULL or omitted uses the codec default; an \
             out-of-range level clamps to the codec's range; it is ignored for level-less codecs \
             (snappy/snappy_raw). Returns the compressed bytes in the codec's canonical container \
             (framed where the codec has a frame). NULL input → NULL; empty input → the codec's \
             valid empty stream. The inverse of decompress.",
            "Compress a BLOB with a codec, e.g. `compress(b, 'zstd', 19)`. Level is optional \
             (codec default when omitted). Returns a BLOB; the inverse of `decompress`.",
            "compress, deflate, gzip, zstd, brotli, lz4, snappy, xz, lzma, bzip2, encode, pack, \
             shrink, codec, blob, level, transcode",
        );
        tags.push(("vgi.example_queries".into(), if self.with_level {
            "[{\"description\":\"Compress a blob with zstd level 19.\",\"sql\":\"SELECT compress.main.compress('the quick brown fox'::BLOB, 'zstd', 19) AS packed\"}]"
        } else {
            "[{\"description\":\"Compress a blob with gzip at the default level.\",\"sql\":\"SELECT compress.main.compress('the quick brown fox'::BLOB, 'gzip') AS packed\"}]"
        }.into()));
        // VGI509: at least one guaranteed-runnable, output-verified example.
        tags.push(("vgi.executable_examples".into(),
            "[{\"description\":\"Round-trip a blob through gzip and read it back as text.\",\"sql\":\"SELECT compress.main.decompress(compress.main.compress('hello, world'::BLOB, 'gzip'), 'gzip')::VARCHAR AS roundtrip\",\"expected_result\":[{\"roundtrip\":\"hello, world\"}]},{\"description\":\"Detect the codec of a zstd-compressed blob.\",\"sql\":\"SELECT compress.main.detect_codec(compress.main.compress('payload'::BLOB, 'zstd')) AS codec\",\"expected_result\":[{\"codec\":\"zstd\"}]}]".into()));
        FunctionMetadata {
            description: if self.with_level {
                "Compress a BLOB with the given codec and explicit level (codec default if NULL)"
            } else {
                "Compress a BLOB with the given codec at the codec's default level"
            }
            .into(),
            return_type: Some(DataType::Binary),
            examples: vec![example],
            tags,
            ..Default::default()
        }
    }

    fn argument_specs(&self) -> Vec<ArgSpec> {
        let mut specs = vec![
            ArgSpec::any_column(
                "input",
                0,
                "The payload to compress — one row's worth of bytes.",
            ),
            ArgSpec::column_typed(
                "codec",
                1,
                DataType::Utf8,
                "The codec name (case-insensitive): zstd, gzip, zlib, deflate, brotli, lz4, \
                 lz4_block, snappy, snappy_raw, xz, lzma, bzip2.",
            ),
        ];
        if self.with_level {
            specs.push(ArgSpec::any_column(
                "level",
                2,
                "Compression level. NULL → the codec default; out-of-range clamps to the codec's \
                 range; ignored for level-less codecs (snappy/snappy_raw).",
            ));
        }
        specs
    }

    fn on_bind(&self, _params: &BindParams) -> Result<BindResponse> {
        Ok(BindResponse::result(DataType::Binary))
    }

    fn process(&self, params: &ProcessParams, batch: &RecordBatch) -> Result<RecordBatch> {
        let input = batch.column(0);
        let codec = batch.column(1);
        let level_col = if self.with_level {
            Some(batch.column(2))
        } else {
            None
        };
        let rows = batch.num_rows();
        let mut out: Vec<Option<Vec<u8>>> = Vec::with_capacity(rows);
        for i in 0..rows {
            match blob_bytes(input, i)? {
                None => out.push(None),
                Some(bytes) => {
                    let c = require_codec(codec, i)?;
                    let level = match level_col {
                        Some(col) => level_i32(int_val(col, i)?),
                        None => None,
                    };
                    let packed = compress_core::compress(c, bytes, level).map_err(codec_err)?;
                    out.push(Some(packed));
                }
            }
        }
        let arr: ArrayRef = binary_array(&out);
        RecordBatch::try_new(params.output_schema.clone(), vec![arr])
            .map_err(|e| RpcError::runtime_error(e.to_string()))
    }
}

// --- decompress ------------------------------------------------------------

pub struct Decompress {
    /// Whether this overload accepts the optional positional `max_output_bytes`.
    pub with_cap: bool,
}

impl ScalarFunction for Decompress {
    fn name(&self) -> &str {
        "decompress"
    }

    fn metadata(&self) -> FunctionMetadata {
        let mut tags = crate::meta::object_tags(
            "Decompress a BLOB",
            "Decompress a BLOB with the named codec (the inverse of compress). The optional third \
             argument max_output_bytes is the decompression-bomb guard: decoding aborts the row \
             once output would exceed the cap and returns a per-row error, so the worker never \
             OOMs. NULL/omitted uses the worker's configured default cap (the \
             compress_max_output_bytes ATTACH option / env, default 256 MiB); pass a large value \
             to opt out per call. Malformed, truncated, or wrong-codec input returns a clean \
             per-row error, never a panic. NULL input → NULL.",
            "Decompress a BLOB with a codec, e.g. `decompress(b, 'gzip')`. Optional \
             `max_output_bytes` caps output (bomb guard; default 256 MiB). Returns a BLOB; the \
             inverse of `compress`.",
            "decompress, inflate, gunzip, unzstd, decode, unpack, expand, codec, blob, \
             decompression bomb, max_output_bytes, gzip, zstd, brotli, lz4, snappy, xz, bzip2",
        );
        tags.push(("vgi.example_queries".into(), if self.with_cap {
            "[{\"description\":\"Decompress a gzip blob with a 64 MiB output cap.\",\"sql\":\"SELECT compress.main.decompress(compress.main.compress('hi'::BLOB,'gzip'), 'gzip', 67108864) AS body\"}]"
        } else {
            "[{\"description\":\"Decompress a gzip blob and cast to text.\",\"sql\":\"SELECT compress.main.decompress(compress.main.compress('hi'::BLOB,'gzip'), 'gzip')::VARCHAR AS body\"}]"
        }.into()));
        FunctionMetadata {
            description: if self.with_cap {
                "Decompress a BLOB with the given codec, capping output at max_output_bytes"
            } else {
                "Decompress a BLOB with the given codec (default bomb-guard output cap)"
            }
            .into(),
            return_type: Some(DataType::Binary),
            examples: vec![FunctionExample {
                sql: "SELECT compress.main.decompress(compress.main.compress('hi'::BLOB,'gzip'), 'gzip')::VARCHAR;".into(),
                description: "Decompress a gzip'd blob and cast to text.".into(),
                expected_output: None,
            }],
            tags,
            ..Default::default()
        }
    }

    fn argument_specs(&self) -> Vec<ArgSpec> {
        let mut specs = vec![
            ArgSpec::any_column("input", 0, "The compressed BLOB to decode."),
            ArgSpec::column_typed(
                "codec",
                1,
                DataType::Utf8,
                "The codec the input is compressed with (case-insensitive): zstd, gzip, zlib, \
                 deflate, brotli, lz4, lz4_block, snappy, snappy_raw, xz, lzma, bzip2.",
            ),
        ];
        if self.with_cap {
            specs.push(ArgSpec::any_column(
                "max_output_bytes",
                2,
                "Decompression-bomb guard: abort the row if decoded output would exceed this many \
                 bytes. NULL → the worker default cap (compress_max_output_bytes, 256 MiB).",
            ));
        }
        specs
    }

    fn on_bind(&self, _params: &BindParams) -> Result<BindResponse> {
        Ok(BindResponse::result(DataType::Binary))
    }

    fn process(&self, params: &ProcessParams, batch: &RecordBatch) -> Result<RecordBatch> {
        let input = batch.column(0);
        let codec = batch.column(1);
        let cap_col = if self.with_cap {
            Some(batch.column(2))
        } else {
            None
        };
        let rows = batch.num_rows();
        let mut out: Vec<Option<Vec<u8>>> = Vec::with_capacity(rows);
        for i in 0..rows {
            match blob_bytes(input, i)? {
                None => out.push(None),
                Some(bytes) => {
                    let c = require_codec(codec, i)?;
                    let per_call = match cap_col {
                        Some(col) => int_val(col, i)?,
                        None => None,
                    };
                    let cap = resolve_cap(per_call, params);
                    let plain = compress_core::decompress(c, bytes, cap).map_err(codec_err)?;
                    out.push(Some(plain));
                }
            }
        }
        let arr: ArrayRef = binary_array(&out);
        RecordBatch::try_new(params.output_schema.clone(), vec![arr])
            .map_err(|e| RpcError::runtime_error(e.to_string()))
    }
}

// --- decompress_auto -------------------------------------------------------

pub struct DecompressAuto {
    /// Whether this overload accepts the optional positional `max_output_bytes`.
    pub with_cap: bool,
}

impl ScalarFunction for DecompressAuto {
    fn name(&self) -> &str {
        "decompress_auto"
    }

    fn metadata(&self) -> FunctionMetadata {
        let mut tags = crate::meta::object_tags(
            "Auto-detect & Decompress a BLOB",
            "Detect the codec by magic bytes, then decompress (the inverse of compress without \
             naming the codec). Resolves only magic-bearing codecs: zstd, gzip, zlib, xz, lzma, \
             bzip2, lz4 (frame), snappy (framed). Headerless codecs (brotli, deflate, lz4_block, \
             snappy_raw) carry no signature and return a per-row 'cannot auto-detect codec (no \
             magic bytes)' error — use the explicit decompress form. Same decompression-bomb \
             guard as decompress (optional max_output_bytes; default 256 MiB). NULL input → NULL.",
            "Auto-detect the codec by magic bytes and decompress, e.g. \
             `decompress_auto(value, 67108864)`. Only magic-bearing codecs resolve; headerless \
             ones error. Returns a BLOB.",
            "decompress_auto, auto-detect, magic bytes, sniff codec, mixed codec, kafka, decode, \
             gunzip, unzstd, blob, decompression bomb, max_output_bytes",
        );
        tags.push(("vgi.example_queries".into(),
            "[{\"description\":\"Auto-detect and decompress a gzip blob with a 64 MiB cap.\",\"sql\":\"SELECT compress.main.decompress_auto(compress.main.compress('hi'::BLOB,'gzip'), 67108864) AS plaintext\"}]".into()));
        FunctionMetadata {
            description: if self.with_cap {
                "Auto-detect the codec by magic bytes and decompress, capping output at max_output_bytes"
            } else {
                "Auto-detect the codec by magic bytes and decompress (default bomb-guard cap)"
            }
            .into(),
            return_type: Some(DataType::Binary),
            examples: vec![FunctionExample {
                sql: "SELECT compress.main.decompress_auto(compress.main.compress('hi'::BLOB,'gzip'), 67108864);".into(),
                description: "Auto-detect and decompress a blob, cap 64 MiB/row.".into(),
                expected_output: None,
            }],
            tags,
            ..Default::default()
        }
    }

    fn argument_specs(&self) -> Vec<ArgSpec> {
        // Type `input` as BLOB (not ANY) so this function declares a concrete
        // parameter type (VGI310); DuckDB implicitly casts a VARCHAR payload.
        let mut specs = vec![ArgSpec::column_typed(
            "input",
            0,
            DataType::Binary,
            "The compressed BLOB to auto-detect (by magic bytes) and decode.",
        )];
        if self.with_cap {
            specs.push(ArgSpec::any_column(
                "max_output_bytes",
                1,
                "Decompression-bomb guard: abort the row if decoded output would exceed this many \
                 bytes. NULL → the worker default cap (compress_max_output_bytes, 256 MiB).",
            ));
        }
        specs
    }

    fn on_bind(&self, _params: &BindParams) -> Result<BindResponse> {
        Ok(BindResponse::result(DataType::Binary))
    }

    fn process(&self, params: &ProcessParams, batch: &RecordBatch) -> Result<RecordBatch> {
        let input = batch.column(0);
        let cap_col = if self.with_cap {
            Some(batch.column(1))
        } else {
            None
        };
        let rows = batch.num_rows();
        let mut out: Vec<Option<Vec<u8>>> = Vec::with_capacity(rows);
        for i in 0..rows {
            match blob_bytes(input, i)? {
                None => out.push(None),
                Some(bytes) => {
                    let per_call = match cap_col {
                        Some(col) => int_val(col, i)?,
                        None => None,
                    };
                    let cap = resolve_cap(per_call, params);
                    let plain = compress_core::decompress_auto(bytes, cap).map_err(codec_err)?;
                    out.push(Some(plain));
                }
            }
        }
        let arr: ArrayRef = binary_array(&out);
        RecordBatch::try_new(params.output_schema.clone(), vec![arr])
            .map_err(|e| RpcError::runtime_error(e.to_string()))
    }
}
