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
                "Compress and decompress a DuckDB BLOB column entirely inside the query engine, \
                 across many codecs — zstd, gzip, zlib, deflate (raw), brotli, lz4 (frame and \
                 block), snappy (framed and raw), xz, lzma, and bzip2. Fills the gap DuckDB core \
                 leaves open: core reads compressed files, but offers no way to decode a COLUMN \
                 of already-compressed payloads at scan time — snappy/lz4/zstd Kafka frames, \
                 gzip'd HTTP bodies, brotli web responses — to transcode a column in place (for \
                 example gzip → zstd), or to audit compressed and decompressed sizes and ratios, \
                 all without leaving the engine. A codec can be named explicitly or sniffed \
                 automatically from magic bytes. Every decode path enforces a decompression-bomb \
                 guard (a per-row output-byte cap, default 256 MiB): a small blob that legally \
                 expands to many gigabytes aborts only that row with a clean per-row error \
                 instead of OOMing the worker, and malformed or truncated input is likewise a \
                 per-row error, never a crash. Pure in-engine local CPU: no network, no state, \
                 zero egress (safe for air-gapped / regulated data). Reach for it whenever \
                 compressed bytes live in a column and you need them decoded, encoded, \
                 re-encoded, identified, or measured in SQL."
                    .to_string(),
            ),
            (
                "vgi.doc_md".to_string(),
                "# compress\n\n**Multi-codec (de)compression over a DuckDB `BLOB` column** — \
                 `zstd`, `gzip`, `zlib`, `deflate`, `brotli`, `lz4` (frame + block), `snappy` \
                 (framed + raw), `xz`, `lzma`, `bzip2` — plus codec auto-detection by magic \
                 bytes, level control, and size/ratio introspection. Fills the one gap DuckDB \
                 core leaves open: core decompresses *files*, but there is no scalar to decode a \
                 *column* of compressed payloads at scan time. Decode \
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
            // VGI152/VGI920: a fixed analyst-task suite the `--ai` agent-check runs.
            // Every function is a pure, deterministic transform, so each
            // reference_sql yields a stable reference result; prompts pin the
            // output column name because grading is strict on names + values.
            (
                "vgi.agent_test_tasks".to_string(),
                meta::agent_test_tasks_json(&[
                    (
                        "roundtrip_gzip",
                        "I have the text 'hello, world'. Compress it with gzip and then \
                         decompress it back to recover the original text. Return a single row \
                         with one column named text.",
                        "SELECT compress.main.decompress(compress.main.compress('hello, \
                         world'::BLOB, 'gzip'), 'gzip')::VARCHAR AS text",
                    ),
                    (
                        "identify_codec",
                        "Some bytes were produced by compressing the text 'payload' with zstd. \
                         Without being told which codec was used, identify it from the bytes \
                         themselves. Return a single row with one column named codec.",
                        "SELECT compress.main.detect_codec(compress.main.compress('payload'::BLOB, \
                         'zstd')) AS codec",
                    ),
                    (
                        "auto_decompress",
                        "I received a blob that is the text 'streaming data' compressed with \
                         gzip, but the sender did not say which codec they used. Recover the \
                         original text automatically, without naming a codec. Return a single \
                         row with one column named text.",
                        "SELECT compress.main.decompress_auto(compress.main.compress('streaming \
                         data'::BLOB, 'gzip'))::VARCHAR AS text",
                    ),
                    (
                        "compressed_size_zstd",
                        "How many bytes does the text 'the quick brown fox jumps over the lazy \
                         dog' occupy after being compressed with zstd at level 19? Do not return \
                         the compressed bytes, just the size. Return a single row with one column \
                         named bytes.",
                        "SELECT compress.main.compressed_size('the quick brown fox jumps over \
                         the lazy dog'::BLOB, 'zstd', 19) AS bytes",
                    ),
                    (
                        "decompressed_size",
                        "A gzip blob was produced from the text 'the quick brown fox jumps over \
                         the lazy dog'. Without printing the text, how many bytes does that blob \
                         decompress to? Return a single row with one column named bytes.",
                        "SELECT compress.main.decompressed_size(compress.main.compress('the quick \
                         brown fox jumps over the lazy dog'::BLOB, 'gzip'), 'gzip') AS bytes",
                    ),
                    (
                        "ratio_zstd",
                        "For the text 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', what is its \
                         compressed-over-original size ratio under zstd at level 19 (a value \
                         below 1.0 means it shrank)? Return a single row with one column named \
                         ratio.",
                        "SELECT compress.main.ratio('aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\
                         '::BLOB, 'zstd', 19) AS ratio",
                    ),
                    (
                        "validate_gzip",
                        "Check whether a gzip-compressed blob of the text 'hi' is a well-formed \
                         gzip stream. Return a single row with one column named ok.",
                        "SELECT compress.main.is_valid(compress.main.compress('hi'::BLOB, 'gzip'), \
                         'gzip') AS ok",
                    ),
                    (
                        "supports_zstd",
                        "Does this build support the zstd codec? Return a single row with one \
                         column named has_zstd.",
                        "SELECT list_contains(compress.main.codecs(), 'zstd') AS has_zstd",
                    ),
                    (
                        "worker_version",
                        "What version of the compress worker is currently running? Return a \
                         single row with one column named version.",
                        "SELECT compress.main.compress_version() AS version",
                    ),
                ]),
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
                    "Multi-codec (de)compression over a DuckDB BLOB column, grouped into one \
                     schema: transforms that encode and decode bytes, automatic codec detection \
                     and identification from magic bytes, size / ratio / validity introspection, \
                     and codec discovery. Codecs span zstd, gzip, zlib, deflate, brotli, lz4 \
                     (frame and block), snappy (framed and raw), xz, lzma, and bzip2. Every \
                     decode is bounded by a decompression-bomb guard (a per-row output-byte cap). \
                     The catalog name matches the ATTACH name, so calls are qualified with the \
                     compress.main prefix."
                        .to_string(),
                ),
                (
                    "vgi.doc_md".to_string(),
                    "## compress.main\n\nThe single schema of the `compress` worker. The catalog \
                     name matches the `ATTACH` name, so calls are qualified with the \
                     `compress.main` prefix.\n\nThe surface is grouped into four navigable \
                     areas:\n\n\
                     - **encode** — turn a `BLOB` into compressed bytes with a chosen codec and \
                     optional level.\n\
                     - **decode** — recover the original bytes, with the codec named explicitly \
                     or auto-detected, under a decompression-bomb guard.\n\
                     - **introspection** — measure compressed and decompressed size, compression \
                     ratio, and stream validity without materializing the output.\n\
                     - **discovery** — identify a blob's codec, list the codecs this build \
                     supports, and report the worker version.\n\n\
                     Every codec — zstd, gzip, zlib, deflate, brotli, lz4, snappy, xz, lzma, and \
                     bzip2 — shares this one uniform surface over a `BLOB` column."
                        .to_string(),
                ),
                // VGI413: the navigation/SEO category registry for this schema.
                // Every function below declares a `vgi.category` naming one of these.
                (
                    "vgi.categories".to_string(),
                    "[{\"name\":\"encode\",\"description\":\"Compress a BLOB with a chosen codec \
                     and optional level.\"},{\"name\":\"decode\",\"description\":\"Decompress a \
                     BLOB back to its original bytes — codec named explicitly or auto-detected — \
                     bounded by a decompression-bomb guard.\"},{\"name\":\"introspection\",\
                     \"description\":\"Measure compressed and decompressed size, compression \
                     ratio, and stream validity without materializing the output.\"},{\"name\":\
                     \"discovery\",\"description\":\"Identify a blob's codec, list the codecs this \
                     build supports, and report the worker version.\"}]"
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
