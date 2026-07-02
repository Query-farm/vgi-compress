//! `codecs() -> LIST<VARCHAR>` — the codec names this build supports.

use std::sync::Arc;

use arrow_array::{ArrayRef, RecordBatch};
use arrow_schema::{DataType, Field};
use vgi::{
    ArgSpec, BindParams, BindResponse, FunctionExample, FunctionMetadata, ProcessParams,
    ScalarFunction,
};
use vgi_rpc::{Result, RpcError};

use crate::arrow_io::list_string_single;

/// The `LIST<VARCHAR>` return type.
fn list_varchar() -> DataType {
    DataType::List(Arc::new(Field::new("item", DataType::Utf8, true)))
}

pub struct Codecs;

impl ScalarFunction for Codecs {
    fn name(&self) -> &str {
        "codecs"
    }

    fn metadata(&self) -> FunctionMetadata {
        let mut tags = crate::meta::object_tags(
            "Supported Codecs",
            "Return the list of codec names this build supports, so SQL can discover the surface \
             without a docs lookup. Reflects compiled-in features (e.g. which xz backend, or a \
             codec opt-out). Use the returned names as the codec argument to compress / \
             decompress / compressed_size / ratio / is_valid.",
            "List the codec names this build supports, e.g. `codecs()` → ['zstd','gzip',…]. \
             Returns LIST<VARCHAR>.",
            "codecs, supported codecs, list codecs, discovery, available, what codecs, capability",
        );
        tags.push(("vgi.example_queries".into(),
            "[{\"description\":\"List every codec this build supports.\",\"sql\":\"SELECT compress.main.codecs() AS codecs\"},{\"description\":\"Is zstd supported?\",\"sql\":\"SELECT list_contains(compress.main.codecs(), 'zstd') AS has_zstd\"}]".into()));
        tags.push(("vgi.category".into(), "discovery".into()));
        FunctionMetadata {
            description: "List the codec names this build supports (reflects feature flags)".into(),
            return_type: Some(list_varchar()),
            examples: vec![FunctionExample {
                sql: "SELECT compress.main.codecs();".into(),
                description: "List every supported codec name.".into(),
                expected_output: None,
            }],
            tags,
            ..Default::default()
        }
    }

    fn argument_specs(&self) -> Vec<ArgSpec> {
        Vec::new()
    }

    fn on_bind(&self, _params: &BindParams) -> Result<BindResponse> {
        Ok(BindResponse::result(list_varchar()))
    }

    fn process(&self, params: &ProcessParams, batch: &RecordBatch) -> Result<RecordBatch> {
        let rows = batch.num_rows();
        let names = compress_core::codecs();
        let arr: ArrayRef = list_string_single(&names, rows);
        RecordBatch::try_new(params.output_schema.clone(), vec![arr])
            .map_err(|e| RpcError::runtime_error(e.to_string()))
    }
}
