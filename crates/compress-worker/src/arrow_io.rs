//! Arrow input/output helpers shared across the scalar functions: reading a
//! BLOB/VARCHAR input cell, reading the optional numeric (`level` /
//! `max_output_bytes`) and `codec` arguments, and small column builders.
//!
//! Scalar arguments — including SQL literals — arrive as columns in the process
//! batch, so `codec` / `level` / `max_output_bytes` are all read per row. This
//! lets a query pass a constant (`compress(b, 'zstd', 19)`) or a column
//! (`decompress(payload, codec_col)`) uniformly.

use std::sync::Arc;

use arrow_array::builder::{
    BinaryBuilder, Float64Builder, ListBuilder, StringBuilder, UInt64Builder,
};
use arrow_array::cast::AsArray;
use arrow_array::types::{
    Int16Type, Int32Type, Int64Type, Int8Type, UInt16Type, UInt32Type, UInt64Type, UInt8Type,
};
use arrow_array::{Array, ArrayRef};
use arrow_schema::DataType;
use vgi_rpc::{Result, RpcError};

/// Borrow the raw bytes of a BLOB/VARCHAR input cell at `row`, or `None` if null.
pub fn blob_bytes(col: &ArrayRef, row: usize) -> Result<Option<&[u8]>> {
    if col.is_null(row) {
        return Ok(None);
    }
    Ok(Some(match col.data_type() {
        DataType::Binary => col.as_binary::<i32>().value(row),
        DataType::LargeBinary => col.as_binary::<i64>().value(row),
        DataType::Utf8 => col.as_string::<i32>().value(row).as_bytes(),
        DataType::LargeUtf8 => col.as_string::<i64>().value(row).as_bytes(),
        other => {
            return Err(RpcError::value_error(format!(
                "expected a BLOB or VARCHAR argument, got {other:?}"
            )))
        }
    }))
}

/// Read a VARCHAR cell as `&str`, or `None` if null. Errors on a non-string type.
pub fn text_str(col: &ArrayRef, row: usize) -> Result<Option<&str>> {
    if col.is_null(row) {
        return Ok(None);
    }
    Ok(Some(match col.data_type() {
        DataType::Utf8 => col.as_string::<i32>().value(row),
        DataType::LargeUtf8 => col.as_string::<i64>().value(row),
        other => {
            return Err(RpcError::value_error(format!(
                "expected a VARCHAR (codec) argument, got {other:?}"
            )))
        }
    }))
}

/// Read an optional integer cell (`level` or `max_output_bytes`) as `i64`, or
/// `None` if null. Accepts any of DuckDB's signed/unsigned integer widths.
/// Errors on a non-integer column.
pub fn int_val(col: &ArrayRef, row: usize) -> Result<Option<i64>> {
    if col.is_null(row) {
        return Ok(None);
    }
    Ok(Some(match col.data_type() {
        DataType::Int64 => col.as_primitive::<Int64Type>().value(row),
        DataType::Int32 => col.as_primitive::<Int32Type>().value(row) as i64,
        DataType::Int16 => col.as_primitive::<Int16Type>().value(row) as i64,
        DataType::Int8 => col.as_primitive::<Int8Type>().value(row) as i64,
        DataType::UInt64 => col.as_primitive::<UInt64Type>().value(row) as i64,
        DataType::UInt32 => col.as_primitive::<UInt32Type>().value(row) as i64,
        DataType::UInt16 => col.as_primitive::<UInt16Type>().value(row) as i64,
        DataType::UInt8 => col.as_primitive::<UInt8Type>().value(row) as i64,
        other => {
            return Err(RpcError::value_error(format!(
                "expected an integer (level / max_output_bytes) argument, got {other:?}"
            )))
        }
    }))
}

/// Build a nullable BLOB column from per-row optional byte vectors.
pub fn binary_array(col: &[Option<Vec<u8>>]) -> ArrayRef {
    let mut b = BinaryBuilder::new();
    for v in col {
        match v {
            Some(bytes) => b.append_value(bytes),
            None => b.append_null(),
        }
    }
    Arc::new(b.finish())
}

/// Build a nullable VARCHAR column.
pub fn string_array(col: &[Option<String>]) -> ArrayRef {
    let mut b = StringBuilder::new();
    for v in col {
        match v {
            Some(s) => b.append_value(s),
            None => b.append_null(),
        }
    }
    Arc::new(b.finish())
}

/// Build a nullable UBIGINT column.
pub fn u64_opt_array(col: &[Option<u64>]) -> ArrayRef {
    let mut b = UInt64Builder::new();
    for v in col {
        match v {
            Some(x) => b.append_value(*x),
            None => b.append_null(),
        }
    }
    Arc::new(b.finish())
}

/// Build a nullable DOUBLE column.
pub fn f64_opt_array(col: &[Option<f64>]) -> ArrayRef {
    let mut b = Float64Builder::new();
    for v in col {
        match v {
            Some(x) => b.append_value(*x),
            None => b.append_null(),
        }
    }
    Arc::new(b.finish())
}

/// Build a single-row `LIST<VARCHAR>` column from a list of names (used by
/// `codecs()`), repeated for `rows` rows so it matches a scalar batch.
pub fn list_string_single(values: &[&str], rows: usize) -> ArrayRef {
    let mut b = ListBuilder::new(StringBuilder::new());
    for _ in 0..rows {
        for v in values {
            b.values().append_value(v);
        }
        b.append(true);
    }
    Arc::new(b.finish())
}
