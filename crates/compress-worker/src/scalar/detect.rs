//! `detect_codec(BLOB) -> VARCHAR` — codec name by magic bytes, or 'unknown'.

use arrow_array::{ArrayRef, RecordBatch};
use arrow_schema::DataType;
use vgi::{
    ArgSpec, BindParams, BindResponse, FunctionExample, FunctionMetadata, ProcessParams,
    ScalarFunction,
};
use vgi_rpc::{Result, RpcError};

use crate::arrow_io::{blob_bytes, string_array};

pub struct DetectCodec;

impl ScalarFunction for DetectCodec {
    fn name(&self) -> &str {
        "detect_codec"
    }

    fn metadata(&self) -> FunctionMetadata {
        let mut tags = crate::meta::object_tags(
            "Detect Codec by Magic Bytes",
            "Return the codec name a BLOB is compressed with ('gzip', 'zstd', 'xz', …) by \
             magic-byte match, validated against a trial decode of the leading block to reject \
             weak-magic false positives. Returns 'unknown' when no signature matches — which \
             includes every HEADERLESS codec (brotli, deflate, lz4_block, snappy_raw carry no \
             magic and can never be detected; use the explicit decompress form for those). Cheap: \
             inspects only the first few bytes plus a bounded trial. Never errors. NULL input → \
             NULL.",
            "Return the codec of a BLOB by magic bytes, e.g. `detect_codec(b)` → 'gzip', or \
             'unknown' for headerless/unrecognized input. Never errors.",
            "detect_codec, sniff, magic bytes, identify codec, gzip, zstd, xz, bzip2, lz4, \
             snappy, auto-detect, unknown",
        );
        tags.push(("vgi.example_queries".into(),
            "[{\"description\":\"Identify the codec of a compressed blob.\",\"sql\":\"SELECT compress.main.detect_codec(compress.main.compress('hi'::BLOB,'zstd')) AS codec\"}]".into()));
        FunctionMetadata {
            description: "Return the codec name of a BLOB by magic bytes, or 'unknown'".into(),
            return_type: Some(DataType::Utf8),
            examples: vec![FunctionExample {
                sql:
                    "SELECT compress.main.detect_codec(compress.main.compress('hi'::BLOB,'gzip'));"
                        .into(),
                description: "Identify the codec of a compressed blob.".into(),
                expected_output: None,
            }],
            tags,
            ..Default::default()
        }
    }

    fn argument_specs(&self) -> Vec<ArgSpec> {
        vec![ArgSpec::any_column(
            "input",
            0,
            "The BLOB to identify by magic bytes. Headerless codecs return 'unknown'.",
        )]
    }

    fn on_bind(&self, _params: &BindParams) -> Result<BindResponse> {
        Ok(BindResponse::result(DataType::Utf8))
    }

    fn process(&self, params: &ProcessParams, batch: &RecordBatch) -> Result<RecordBatch> {
        let input = batch.column(0);
        let rows = batch.num_rows();
        let mut out: Vec<Option<String>> = Vec::with_capacity(rows);
        for i in 0..rows {
            out.push(blob_bytes(input, i)?.map(|bytes| {
                compress_core::detect_codec(bytes)
                    .unwrap_or("unknown")
                    .to_string()
            }));
        }
        let arr: ArrayRef = string_array(&out);
        RecordBatch::try_new(params.output_schema.clone(), vec![arr])
            .map_err(|e| RpcError::runtime_error(e.to_string()))
    }
}
