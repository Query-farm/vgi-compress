//! Composed example queries: realistic SQL that combines two or more of this
//! worker's functions in one query (one function's output feeding another, a
//! scalar applied across the `codecs()` list, …).
//!
//! Each `(description, sql)` pair is defined ONCE here and referenced from both
//! the schema-level `vgi.example_queries` list (`lib.rs`) and the example list
//! of the function that drives the query (`scalar/*.rs`), so the two copies can
//! never drift. Every query is self-contained (inline fixtures, no tables),
//! offline, and deterministic.

/// `codecs()` × `compressed_size` / `ratio`: benchmark every codec on a payload.
pub const CODEC_BENCHMARK: (&str, &str) = (
    "Rank every supported codec by compressed size and ratio on a sample JSON-lines payload, \
     at each codec's default level (smallest output first).",
    "SELECT codec, compress.main.compressed_size(p, codec) AS bytes_out, \
     round(compress.main.ratio(p, codec), 3) AS ratio \
     FROM (SELECT repeat('{\"user\":42,\"event\":\"click\",\"page\":\"/home\"}' || chr(10), 200)::BLOB AS p), \
     unnest(compress.main.codecs()) AS t(codec) ORDER BY bytes_out, codec",
);

/// `detect_codec` feeding `decompressed_size`: profile a mixed-codec column.
pub const MIXED_CODEC_PROFILE: (&str, &str) = (
    "Profile a mixed-codec message column: group messages by the codec sniffed from their magic \
     bytes and total stored vs. decompressed bytes per codec (the headerless brotli message is \
     detected as 'unknown' and left out).",
    "WITH inbox AS (SELECT id, compress.main.compress(repeat(body, 20)::BLOB, codec) AS payload \
     FROM (VALUES (1, 'zstd', 'temp=21.5;'), (2, 'gzip', 'temp=22.0;'), (3, 'zstd', 'temp=20.9;'), \
     (4, 'bzip2', 'temp=23.1;'), (5, 'brotli', 'temp=19.8;')) AS t(id, codec, body)), \
     sniffed AS (SELECT payload, compress.main.detect_codec(payload) AS codec FROM inbox) \
     SELECT codec, count(*) AS messages, sum(octet_length(payload)) AS stored_bytes, \
     sum(compress.main.decompressed_size(payload, codec)) AS raw_bytes \
     FROM sniffed WHERE codec <> 'unknown' GROUP BY codec ORDER BY codec",
);

/// `decompress` feeding `compressed_size`: size a gzip → zstd transcode.
pub const TRANSCODE_AUDIT: (&str, &str) = (
    "Before transcoding a gzip archive to zstd, compare each file's current gzip size with the \
     size its decompressed contents would take as zstd level 19 (nothing is rewritten).",
    "WITH archive AS (SELECT name, compress.main.compress(repeat(line, n)::BLOB, 'gzip') AS gz \
     FROM (VALUES ('access.log', 'GET /index.html 200' || chr(10), 300), \
     ('error.log', 'WARN disk 91% full' || chr(10), 40)) AS t(name, line, n)) \
     SELECT name, octet_length(gz) AS gzip_bytes, \
     compress.main.compressed_size(compress.main.decompress(gz, 'gzip'), 'zstd', 19) AS zstd_bytes \
     FROM archive ORDER BY name",
);

/// `detect_codec` + `is_valid` across `codecs()`: triage headerless payloads.
pub const HEADERLESS_TRIAGE: (&str, &str) = (
    "Triage payloads whose codec cannot be sniffed: for each blob detect_codec reports as \
     'unknown', list every supported codec it validates as a well-formed stream (the gzip blob \
     is detectable, so it is skipped).",
    "WITH uploads AS (SELECT id, compress.main.compress(body::BLOB, codec) AS payload \
     FROM (VALUES (1, '{\"status\":\"ok\",\"items\":[1,2,3]}', 'brotli'), \
     (2, '{\"status\":\"ok\",\"items\":[4,5,6]}', 'deflate'), \
     (3, '{\"status\":\"ok\",\"items\":[7,8,9]}', 'gzip')) AS t(id, body, codec)) \
     SELECT id, codec FROM uploads, unnest(compress.main.codecs()) AS t(codec) \
     WHERE compress.main.detect_codec(payload) = 'unknown' \
     AND compress.main.is_valid(payload, codec) ORDER BY id, codec",
);

/// The rows [`HEADERLESS_TRIAGE`] returns, as the `expected_result` JSON of its
/// `vgi.executable_examples` entry.
pub const HEADERLESS_TRIAGE_EXPECTED: &str =
    "[{\"id\":1,\"codec\":\"brotli\"},{\"id\":2,\"codec\":\"deflate\"}]";

/// Every composed example, in the order the schema-level list publishes them.
pub const ALL: &[(&str, &str)] = &[
    CODEC_BENCHMARK,
    MIXED_CODEC_PROFILE,
    TRANSCODE_AUDIT,
    HEADERLESS_TRIAGE,
];
