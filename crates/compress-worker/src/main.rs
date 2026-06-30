//! The `compress` VGI worker.
//!
//! A standalone binary DuckDB launches and talks to over Apache Arrow IPC
//! (`ATTACH 'compress' (TYPE vgi, LOCATION '…')`). It exposes multi-codec
//! compress / decompress scalar functions over a DuckDB `BLOB` column —
//! `zstd`, `gzip`, `zlib`, `deflate`, `brotli`, `lz4` (frame + block),
//! `snappy` (framed + raw), `xz`, `lzma`, `bzip2` — plus codec auto-detection
//! by magic bytes, level control, and size/ratio introspection, under the
//! catalog `compress`, schema `main`:
//!
//! ```sql
//! ATTACH 'compress' (TYPE vgi, LOCATION './target/release/compress-worker');
//! SET search_path = 'compress.main';
//!
//! SELECT decompress(body_gz, 'gzip')::VARCHAR FROM http_logs;
//! SELECT detect_codec(value), decompress_auto(value, 67108864) FROM kafka_messages;
//! SELECT ratio(blob, 'zstd', 19), compress(blob, 'zstd', 19) FROM report;
//! ```
//!
//! Pure in-engine local CPU — no network, no state, zero egress. Every decode
//! path is bounded by a decompression-bomb guard (`max_output_bytes`, default
//! 256 MiB) so a hostile blob fails its own row instead of OOMing the worker.

mod arrow_io;
mod config;
mod meta;
mod scalar;

use vgi::catalog::{CatSchema, CatalogModel};
use vgi::Worker;

/// Catalog + schema metadata surfaced to DuckDB and the `vgi-lint` metadata
/// linter. The function objects themselves are served from the registered
/// scalars.
fn catalog_metadata(name: &str) -> CatalogModel {
    CatalogModel {
        name: name.to_string(),
        comment: Some(
            "Multi-codec (de)compression over a DuckDB BLOB column — zstd, gzip, zlib, deflate, \
             brotli, lz4, snappy, xz, lzma, bzip2 — with codec auto-detection and a \
             decompression-bomb guard."
                .to_string(),
        ),
        tags: vec![
            (
                "vgi.title".to_string(),
                "Multi-Codec Compression Codec".to_string(),
            ),
            (
                "vgi.keywords".to_string(),
                meta::keywords_json(
                    "compress, decompress, compression, codec, zstd, gzip, zlib, deflate, brotli, \
                     lz4, snappy, xz, lzma, bzip2, blob, decode, encode, transcode, \
                     decompression bomb, kafka, auto-detect, magic bytes, ratio",
                ),
            ),
            (
                "vgi.doc_llm".to_string(),
                "Compress and decompress a DuckDB BLOB column with many codecs in SQL: zstd, \
                 gzip, zlib, deflate (raw), brotli, lz4 (frame) and lz4_block, snappy (framed) \
                 and snappy_raw, xz, lzma, and bzip2. `compress(blob, codec, level)` and \
                 `decompress(blob, codec, max_output_bytes)` are inverses; `decompress_auto(blob, \
                 max_output_bytes)` sniffs the codec by magic bytes first; `detect_codec(blob)` \
                 returns the codec name or 'unknown'. Introspect with `compressed_size`, \
                 `decompressed_size`, `ratio`, `is_valid`, and discover the surface with \
                 `codecs()`. Fills the gap DuckDB core leaves open — there is no scalar to decode \
                 a COLUMN of compressed payloads (snappy/lz4/zstd Kafka frames, gzip'd HTTP \
                 bodies, brotli web responses) at scan time, transcode in place (gzip → zstd), or \
                 audit size/ratio — all without leaving the engine. Every decode path enforces a \
                 decompression-bomb guard (max_output_bytes, default 256 MiB): a 1 KB blob that \
                 legally expands to many GB aborts that one row with a per-row error, the worker \
                 never OOMs, and malformed/truncated input is a clean per-row error, never a \
                 crash. Pure in-engine local CPU: no network, no state, zero egress (safe for \
                 air-gapped / regulated data)."
                    .to_string(),
            ),
            (
                "vgi.doc_md".to_string(),
                "# compress\n\n**Multi-codec (de)compression over a DuckDB `BLOB` column** — \
                 `zstd`, `gzip`, `zlib`, `deflate`, `brotli`, `lz4` (frame + block), `snappy` \
                 (framed + raw), `xz`, `lzma`, `bzip2` — plus codec auto-detection by magic \
                 bytes, level control, and size/ratio introspection. Fills the one gap DuckDB \
                 core leaves open: core decompresses *files*, but there is no `decompress(blob, \
                 codec)` scalar to decode a *column* of compressed payloads at scan time. Decode \
                 a mixed-codec Kafka payload column, transcode a cold archive in place (gzip → \
                 zstd), or audit compression — all without leaving the engine.\n\n**Security — \
                 decompression bombs.** Every decode path enforces a bounded `max_output_bytes` \
                 cap (default 256 MiB): a hostile high-ratio blob aborts that one row with a \
                 per-row error instead of OOMing the worker. Pure in-engine local CPU — no \
                 network, no state, zero egress."
                    .to_string(),
            ),
            ("vgi.author".to_string(), "Query.Farm".to_string()),
            (
                "vgi.copyright".to_string(),
                "Copyright 2026 Query Farm LLC - https://query.farm".to_string(),
            ),
            ("vgi.license".to_string(), "MIT".to_string()),
            (
                "vgi.support_contact".to_string(),
                "https://github.com/Query-farm/vgi-compress/issues".to_string(),
            ),
            (
                "vgi.support_policy_url".to_string(),
                "https://github.com/Query-farm/vgi-compress/blob/main/README.md".to_string(),
            ),
        ],
        source_url: Some("https://github.com/Query-farm/vgi-compress".to_string()),
        schemas: vec![CatSchema {
            name: "main".to_string(),
            comment: Some(
                "Multi-codec compress / decompress / detect / introspect functions over a BLOB."
                    .to_string(),
            ),
            tags: vec![
                ("vgi.title".to_string(), "Compress — main".to_string()),
                (
                    "vgi.keywords".to_string(),
                    meta::keywords_json(
                        "compress, decompress, decompress_auto, detect_codec, compressed_size, \
                         decompressed_size, ratio, is_valid, codecs, zstd, gzip, lz4, snappy, xz, \
                         bzip2, brotli, blob",
                    ),
                ),
                ("domain".to_string(), "data-engineering".to_string()),
                ("category".to_string(), "compression".to_string()),
                (
                    "topic".to_string(),
                    "multi-codec-blob-compression".to_string(),
                ),
                (
                    "vgi.doc_llm".to_string(),
                    "Functions for multi-codec (de)compression over a BLOB: `compress`, \
                     `decompress`, `decompress_auto`, `detect_codec`, `compressed_size`, \
                     `decompressed_size`, `ratio`, `is_valid`, `codecs`, and `compress_version`. \
                     Codecs: zstd, gzip, zlib, deflate, brotli, lz4, lz4_block, snappy, \
                     snappy_raw, xz, lzma, bzip2. Every decode is bounded by a decompression-bomb \
                     guard (max_output_bytes)."
                        .to_string(),
                ),
                (
                    "vgi.doc_md".to_string(),
                    "The single schema for the `compress` worker — the catalog name matches the \
                     `ATTACH` name, so qualify calls as `compress.main.<fn>(...)`. Holds the \
                     compress / decompress / decompress_auto / detect_codec scalars, the \
                     compressed_size / decompressed_size / ratio / is_valid introspection \
                     scalars, and `codecs()` discovery."
                        .to_string(),
                ),
                (
                    "vgi.example_queries".to_string(),
                    "SELECT compress.main.decompress(body_gz, 'gzip')::VARCHAR FROM http_logs;\n\
                     SELECT compress.main.detect_codec(value) FROM kafka_messages;\n\
                     SELECT compress.main.decompress_auto(value, 67108864) FROM kafka_messages;\n\
                     SELECT compress.main.ratio(blob, 'zstd', 19) FROM report;\n\
                     SELECT compress.main.compressed_size(blob, 'zstd', 19) FROM report;\n\
                     SELECT compress.main.codecs();"
                        .to_string(),
                ),
            ],
            views: Vec::new(),
            macros: Vec::new(),
            tables: Vec::new(),
        }],
        ..Default::default()
    }
}

fn main() {
    // Logs MUST go to stderr — stdout is the Arrow-IPC channel.
    let _ = env_logger::Builder::from_env(env_logger::Env::default().filter_or("VGI_LOG", "info"))
        .format_timestamp_millis()
        .try_init();

    if std::env::var_os("VGI_WORKER_CATALOG_NAME").is_none() {
        std::env::set_var("VGI_WORKER_CATALOG_NAME", "compress");
    }
    let catalog_name =
        std::env::var("VGI_WORKER_CATALOG_NAME").unwrap_or_else(|_| "compress".to_string());

    let mut worker = Worker::new();
    scalar::register(&mut worker);
    worker.set_catalog(catalog_metadata(&catalog_name));
    worker.run();
}
