//! `compress_version()` — return the worker's version string.

use std::sync::Arc;

use arrow_array::{ArrayRef, RecordBatch, StringArray};
use arrow_schema::DataType;
use vgi::{
    ArgSpec, BindParams, BindResponse, FunctionExample, FunctionMetadata, ProcessParams,
    ScalarFunction,
};
use vgi_rpc::{Result, RpcError};

pub struct CompressVersion;

impl ScalarFunction for CompressVersion {
    fn name(&self) -> &str {
        "compress_version"
    }

    fn metadata(&self) -> FunctionMetadata {
        let mut tags = crate::meta::object_tags(
            "Compress Worker Version",
            "Return the semantic version string of the running compress worker binary. Useful \
             for diagnostics and confirming which build is attached.",
            "Return the compress worker version string, e.g. `compress_version()` → '0.1.0'.",
            "version, build version, compress_version, diagnostics, worker version, semver",
        );
        tags.push(("vgi.category".into(), "discovery".into()));
        FunctionMetadata {
            description: "Returns the compress worker version string".into(),
            return_type: Some(DataType::Utf8),
            examples: vec![FunctionExample {
                sql: "SELECT compress.main.compress_version();".into(),
                description: "Return the compress worker version string.".into(),
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
        Ok(BindResponse::result(DataType::Utf8))
    }

    fn process(&self, params: &ProcessParams, batch: &RecordBatch) -> Result<RecordBatch> {
        let rows = batch.num_rows();
        let out: ArrayRef = Arc::new(StringArray::from(vec![compress_core::version(); rows]));
        RecordBatch::try_new(params.output_schema.clone(), vec![out])
            .map_err(|e| RpcError::runtime_error(e.to_string()))
    }
}
