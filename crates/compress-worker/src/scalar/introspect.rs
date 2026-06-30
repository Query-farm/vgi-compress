//! Introspection scalars: `compressed_size`, `decompressed_size`, `ratio`,
//! `is_valid`. Each shares the `(blob, codec, [level|max_output_bytes])` shape
//! and the optional-trailing-arg overload pattern.

use arrow_array::{ArrayRef, RecordBatch};
use arrow_schema::DataType;
use vgi::{
    ArgSpec, BindParams, BindResponse, FunctionExample, FunctionMetadata, ProcessParams,
    ScalarFunction,
};
use vgi_rpc::{Result, RpcError};

use crate::arrow_io::{blob_bytes, f64_opt_array, int_val, text_str, u64_opt_array};
use crate::config::{level_i32, resolve_cap};

fn require_codec(col: &ArrayRef, i: usize) -> Result<&str> {
    text_str(col, i)?.ok_or_else(|| RpcError::value_error("codec must not be NULL"))
}

// --- compressed_size -------------------------------------------------------

pub struct CompressedSize {
    pub with_level: bool,
}

impl ScalarFunction for CompressedSize {
    fn name(&self) -> &str {
        "compressed_size"
    }

    fn metadata(&self) -> FunctionMetadata {
        let mut tags = crate::meta::object_tags(
            "Estimate Compressed Byte Length",
            "Return the byte length of compress(input, codec, level) — how small the input would \
             get — without keeping the compressed bytes around. The optional level matches \
             compress (NULL → codec default; out-of-range clamps; ignored for level-less codecs). \
             For 'how small would this get?' audits over a table. NULL input → NULL.",
            "Byte length of `compress(input, codec, level)` without materializing it, e.g. \
             `compressed_size(b, 'zstd', 19)`. Returns UBIGINT.",
            "compressed_size, size, bytes, audit, how small, compress size, codec, level",
        );
        tags.push(("vgi.example_queries".into(),
            "[{\"description\":\"How many bytes would this blob be as zstd level 19?\",\"sql\":\"SELECT compress.main.compressed_size('the quick brown fox'::BLOB, 'zstd', 19) AS bytes_out\"}]".into()));
        FunctionMetadata {
            description: if self.with_level {
                "Byte length of compress(input, codec, level) at an explicit level, without materializing the output"
            } else {
                "Byte length of compress(input, codec) at the codec default level, without materializing the output"
            }
            .into(),
            return_type: Some(DataType::UInt64),
            examples: vec![FunctionExample {
                sql: "SELECT compress.main.compressed_size('the quick brown fox'::BLOB, 'zstd', 19);"
                    .into(),
                description: "Compressed byte length without keeping the output.".into(),
                expected_output: None,
            }],
            tags,
            ..Default::default()
        }
    }

    fn argument_specs(&self) -> Vec<ArgSpec> {
        let mut specs = vec![
            ArgSpec::any_column("input", 0, "The BLOB whose compressed size to measure."),
            ArgSpec::column_typed("codec", 1, DataType::Utf8, "The codec to compress with."),
        ];
        if self.with_level {
            specs.push(ArgSpec::any_column(
                "level",
                2,
                "Compression level (NULL → codec default; out-of-range clamps; ignored for \
                 level-less codecs).",
            ));
        }
        specs
    }

    fn on_bind(&self, _params: &BindParams) -> Result<BindResponse> {
        Ok(BindResponse::result(DataType::UInt64))
    }

    fn process(&self, params: &ProcessParams, batch: &RecordBatch) -> Result<RecordBatch> {
        let input = batch.column(0);
        let codec = batch.column(1);
        let level_col = self.with_level.then(|| batch.column(2));
        let rows = batch.num_rows();
        let mut out: Vec<Option<u64>> = Vec::with_capacity(rows);
        for i in 0..rows {
            match blob_bytes(input, i)? {
                None => out.push(None),
                Some(bytes) => {
                    let c = require_codec(codec, i)?;
                    let level = level_col
                        .map(|col| int_val(col, i))
                        .transpose()?
                        .and_then(level_i32);
                    let sz = compress_core::compressed_size(c, bytes, level)
                        .map_err(|e| RpcError::value_error(e.to_string()))?;
                    out.push(Some(sz));
                }
            }
        }
        let arr: ArrayRef = u64_opt_array(&out);
        RecordBatch::try_new(params.output_schema.clone(), vec![arr])
            .map_err(|e| RpcError::runtime_error(e.to_string()))
    }
}

// --- decompressed_size -----------------------------------------------------

pub struct DecompressedSize {
    pub with_cap: bool,
}

impl ScalarFunction for DecompressedSize {
    fn name(&self) -> &str {
        "decompressed_size"
    }

    fn metadata(&self) -> FunctionMetadata {
        let mut tags = crate::meta::object_tags(
            "Decoded Output Byte Length",
            "Return the decompressed byte length of a compressed BLOB. For codecs that record the \
             size (gzip ISIZE) it is read directly and cheaply; otherwise the stream is counted \
             under the decompression-bomb guard. Returns NULL if a full decode would exceed the \
             cap (the optional max_output_bytes; default 256 MiB). NULL input → NULL. Malformed \
             input → per-row error.",
            "Decompressed byte length of a compressed BLOB, e.g. `decompressed_size(b, 'gzip')`. \
             NULL if decoding would exceed the cap. Returns UBIGINT.",
            "decompressed_size, uncompressed size, original size, isize, expand size, codec, \
             max_output_bytes, audit",
        );
        tags.push(("vgi.example_queries".into(),
            "[{\"description\":\"How big does this gzip blob expand to?\",\"sql\":\"SELECT compress.main.decompressed_size(compress.main.compress('the quick brown fox'::BLOB,'gzip'), 'gzip') AS bytes\"}]".into()));
        FunctionMetadata {
            description: if self.with_cap {
                "Decompressed byte length of a compressed BLOB, capped at max_output_bytes (NULL past the cap)"
            } else {
                "Decompressed byte length of a compressed BLOB under the default output cap (NULL past it)"
            }
            .into(),
            return_type: Some(DataType::UInt64),
            examples: vec![FunctionExample {
                sql: "SELECT compress.main.decompressed_size(compress.main.compress('the quick brown fox'::BLOB,'gzip'), 'gzip');".into(),
                description: "Decompressed byte length, read from the trailer where possible."
                    .into(),
                expected_output: None,
            }],
            tags,
            ..Default::default()
        }
    }

    fn argument_specs(&self) -> Vec<ArgSpec> {
        let mut specs = vec![
            ArgSpec::any_column(
                "input",
                0,
                "The compressed BLOB whose decoded size to measure.",
            ),
            ArgSpec::column_typed(
                "codec",
                1,
                DataType::Utf8,
                "The codec the input is compressed with.",
            ),
        ];
        if self.with_cap {
            specs.push(ArgSpec::any_column(
                "max_output_bytes",
                2,
                "Bomb-guard cap; return NULL rather than decode past this many bytes. NULL → the \
                 worker default (256 MiB).",
            ));
        }
        specs
    }

    fn on_bind(&self, _params: &BindParams) -> Result<BindResponse> {
        Ok(BindResponse::result(DataType::UInt64))
    }

    fn process(&self, params: &ProcessParams, batch: &RecordBatch) -> Result<RecordBatch> {
        let input = batch.column(0);
        let codec = batch.column(1);
        let cap_col = self.with_cap.then(|| batch.column(2));
        let rows = batch.num_rows();
        let mut out: Vec<Option<u64>> = Vec::with_capacity(rows);
        for i in 0..rows {
            match blob_bytes(input, i)? {
                None => out.push(None),
                Some(bytes) => {
                    let c = require_codec(codec, i)?;
                    let per_call = cap_col.map(|col| int_val(col, i)).transpose()?.flatten();
                    let cap = resolve_cap(per_call, params);
                    let sz = compress_core::decompressed_size(c, bytes, cap)
                        .map_err(|e| RpcError::value_error(e.to_string()))?;
                    out.push(sz);
                }
            }
        }
        let arr: ArrayRef = u64_opt_array(&out);
        RecordBatch::try_new(params.output_schema.clone(), vec![arr])
            .map_err(|e| RpcError::runtime_error(e.to_string()))
    }
}

// --- ratio -----------------------------------------------------------------

pub struct Ratio {
    pub with_level: bool,
}

impl ScalarFunction for Ratio {
    fn name(&self) -> &str {
        "ratio"
    }

    fn metadata(&self) -> FunctionMetadata {
        let mut tags = crate::meta::object_tags(
            "Compression Ratio",
            "Return the output-over-input compression ratio compressed_size / length(input), so a \
             value < 1.0 means the input shrank. The optional level matches compress. NULL on \
             empty input. For a compression audit over a table.",
            "Output-over-input ratio `compressed_size / length(input)`, e.g. `ratio(b, 'zstd', \
             19)`; < 1.0 means it shrank. Returns DOUBLE (NULL on empty input).",
            "ratio, compression ratio, savings, shrink, out over in, audit, codec, level",
        );
        tags.push(("vgi.example_queries".into(),
            "[{\"description\":\"What compression ratio does zstd-19 achieve on this blob?\",\"sql\":\"SELECT compress.main.ratio('the quick brown fox jumps'::BLOB, 'zstd', 19) AS ratio\"}]".into()));
        FunctionMetadata {
            description: if self.with_level {
                "Output-over-input compression ratio at an explicit level (< 1.0 means it shrank; NULL if empty)"
            } else {
                "Output-over-input compression ratio at the codec default level (< 1.0 means it shrank; NULL if empty)"
            }
            .into(),
            return_type: Some(DataType::Float64),
            examples: vec![FunctionExample {
                sql: "SELECT compress.main.ratio('the quick brown fox jumps'::BLOB, 'zstd', 19);"
                    .into(),
                description: "Compression ratio (out/in).".into(),
                expected_output: None,
            }],
            tags,
            ..Default::default()
        }
    }

    fn argument_specs(&self) -> Vec<ArgSpec> {
        let mut specs = vec![
            ArgSpec::any_column("input", 0, "The BLOB whose compression ratio to measure."),
            ArgSpec::column_typed("codec", 1, DataType::Utf8, "The codec to compress with."),
        ];
        if self.with_level {
            specs.push(ArgSpec::any_column(
                "level",
                2,
                "Compression level (NULL → codec default; out-of-range clamps; ignored for \
                 level-less codecs).",
            ));
        }
        specs
    }

    fn on_bind(&self, _params: &BindParams) -> Result<BindResponse> {
        Ok(BindResponse::result(DataType::Float64))
    }

    fn process(&self, params: &ProcessParams, batch: &RecordBatch) -> Result<RecordBatch> {
        let input = batch.column(0);
        let codec = batch.column(1);
        let level_col = self.with_level.then(|| batch.column(2));
        let rows = batch.num_rows();
        let mut out: Vec<Option<f64>> = Vec::with_capacity(rows);
        for i in 0..rows {
            match blob_bytes(input, i)? {
                None => out.push(None),
                Some(bytes) => {
                    let c = require_codec(codec, i)?;
                    let level = level_col
                        .map(|col| int_val(col, i))
                        .transpose()?
                        .and_then(level_i32);
                    let r = compress_core::ratio(c, bytes, level)
                        .map_err(|e| RpcError::value_error(e.to_string()))?;
                    out.push(r);
                }
            }
        }
        let arr: ArrayRef = f64_opt_array(&out);
        RecordBatch::try_new(params.output_schema.clone(), vec![arr])
            .map_err(|e| RpcError::runtime_error(e.to_string()))
    }
}

// --- is_valid --------------------------------------------------------------

pub struct IsValid;

impl ScalarFunction for IsValid {
    fn name(&self) -> &str {
        "is_valid"
    }

    fn metadata(&self) -> FunctionMetadata {
        let mut tags = crate::meta::object_tags(
            "Is Valid Stream",
            "Return TRUE iff input is a well-formed stream for codec — a trial decode under the \
             bomb guard, output discarded. Never errors: corrupt, truncated, or wrong-codec input \
             is FALSE, not a throw (and a stream that would exceed the default cap is FALSE). NULL \
             input → NULL.",
            "TRUE iff a BLOB is a well-formed stream for a codec, e.g. `is_valid(b, 'gzip')`. \
             Never throws — bad input is FALSE. Returns BOOLEAN.",
            "is_valid, valid, well formed, check, verify, trial decode, codec, blob, total",
        );
        tags.push(("vgi.example_queries".into(),
            "[{\"description\":\"Is this blob a valid gzip stream?\",\"sql\":\"SELECT compress.main.is_valid(compress.main.compress('hi'::BLOB,'gzip'), 'gzip') AS ok\"}]".into()));
        FunctionMetadata {
            description: "TRUE iff the BLOB is a well-formed stream for the codec (never errors)"
                .into(),
            return_type: Some(DataType::Boolean),
            examples: vec![FunctionExample {
                sql: "SELECT compress.main.is_valid(compress.main.compress('hi'::BLOB,'gzip'), 'gzip');".into(),
                description: "Test a blob for gzip well-formedness.".into(),
                expected_output: None,
            }],
            tags,
            ..Default::default()
        }
    }

    fn argument_specs(&self) -> Vec<ArgSpec> {
        vec![
            ArgSpec::any_column("input", 0, "The BLOB to test for well-formedness."),
            ArgSpec::column_typed("codec", 1, DataType::Utf8, "The codec to validate against."),
        ]
    }

    fn on_bind(&self, _params: &BindParams) -> Result<BindResponse> {
        Ok(BindResponse::result(DataType::Boolean))
    }

    fn process(&self, params: &ProcessParams, batch: &RecordBatch) -> Result<RecordBatch> {
        use arrow_array::builder::BooleanBuilder;
        let input = batch.column(0);
        let codec = batch.column(1);
        let rows = batch.num_rows();
        // NULL input → NULL; otherwise a total boolean (never errors).
        let mut b = BooleanBuilder::new();
        for i in 0..rows {
            match blob_bytes(input, i)? {
                None => b.append_null(),
                Some(bytes) => match text_str(codec, i)? {
                    None => b.append_null(),
                    Some(c) => b.append_value(compress_core::is_valid(c, bytes)),
                },
            }
        }
        let arr: ArrayRef = std::sync::Arc::new(b.finish());
        RecordBatch::try_new(params.output_schema.clone(), vec![arr])
            .map_err(|e| RpcError::runtime_error(e.to_string()))
    }
}
