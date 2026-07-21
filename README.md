# vgi-compress

**Multi-codec compress / decompress over a DuckDB `BLOB` column** — `zstd`,
`gzip`, `zlib`, `deflate`, `brotli`, `lz4` (frame + block), `snappy` (framed +
raw), `xz`, `lzma`, and `bzip2` — plus **codec auto-detection by magic bytes**,
level control, and size/ratio introspection.

It fills the one gap DuckDB core leaves open: core decompresses *files*
(parquet/CSV/httpfs), but there is **no `decompress(blob, codec)` scalar** to
decode a *column* of compressed payloads — snappy-framed Kafka messages, gzip'd
HTTP bodies in a log table, zstd telemetry frames, brotli web responses — at
scan time, across millions of rows.

It runs as a [VGI worker](https://query.farm): a small standalone binary that
DuckDB launches and talks to over Apache Arrow. You `ATTACH` it and call its
functions like any other. Pure in-engine scalar compute over a `BLOB` column —
**no network, no state, zero egress** (safe for air-gapped / regulated data).

```sql
INSTALL vgi FROM community;
LOAD vgi;
ATTACH 'compress' (TYPE vgi, LOCATION './target/release/compress-worker');
SET search_path = 'compress.main';

-- 1. Decompress a column of gzip'd HTTP bodies in a log table.
SELECT request_id,
       decompress(body_gz, 'gzip')::VARCHAR AS body   -- BLOB -> BLOB, cast to text
FROM http_logs
WHERE detect_codec(body_gz) = 'gzip';

-- 2. Recompress a cold archive gzip -> zstd (transcode in place, no files).
UPDATE archive
SET payload = compress(decompress(payload, 'gzip'), 'zstd', 19),
    codec   = 'zstd';

-- 3. Auto-detect + decompress a mixed-codec Kafka payload column, with a bomb guard.
SELECT topic, partition, "offset",
       detect_codec(value)                 AS codec,
       decompress_auto(value, 67108864)    AS plaintext   -- cap 64 MiB/row
FROM kafka_messages;

-- 4. Compress a query result for export, and report the win.
SELECT compressed_size(blob, 'zstd', 19)   AS bytes_out,
       ratio(blob, 'zstd', 19)             AS ratio,   -- out/in, < 1.0 means it shrank
       compress(blob, 'zstd', 19)          AS packed
FROM (SELECT string_agg(line, '\n')::BLOB AS blob FROM report_lines);
```

## Security — decompression bombs

A 1 KB zstd or bzip2 blob can legally expand to many GB. An unbounded
`decompress` over a hostile or merely fat column would OOM and kill the worker.
**Every decode path therefore enforces a bounded max-output guard**: it aborts
that one row cleanly once output would exceed the cap, surfaces it as a per-row
error (`output exceeds max_output_bytes`), and **never OOMs or crashes the
worker** — the scan keeps running for the next row. Malformed / truncated /
wrong-codec input is likewise a clean per-row error, never a panic. Zero panics
on arbitrary or truncated bytes is a proptest gate (`crates/compress-core/tests/fuzz.rs`).

The cap is, in precedence order: an explicit `max_output_bytes` argument; the
`compress_max_output_bytes` ATTACH option / DuckDB setting; the
`VGI_COMPRESS_MAX_OUTPUT_BYTES` environment variable; or the built-in default of
**256 MiB**.

## Function catalog

All functions are **stateless scalars** over one row (`compress.main.*`). Codec
names are case-insensitive.

| Function | Signature | Returns |
| --- | --- | --- |
| Compress | `compress(input BLOB, codec VARCHAR [, level])` | `BLOB` |
| Decompress | `decompress(input BLOB, codec VARCHAR [, max_output_bytes])` | `BLOB` |
| Decompress (auto) | `decompress_auto(input BLOB [, max_output_bytes])` | `BLOB` |
| Detect | `detect_codec(input BLOB)` | `VARCHAR` (name or `'unknown'`) |
| Compressed size | `compressed_size(input BLOB, codec VARCHAR [, level])` | `UBIGINT` |
| Decompressed size | `decompressed_size(input BLOB, codec VARCHAR [, max_output_bytes])` | `UBIGINT` |
| Ratio | `ratio(input BLOB, codec VARCHAR [, level])` | `DOUBLE` (out/in) |
| Validate | `is_valid(input BLOB, codec VARCHAR)` | `BOOLEAN` |
| Discover | `codecs()` | `LIST<VARCHAR>` |

The running build version is published as the catalog `implementation_version`
(read it via `duckdb_databases()` / `vgi_catalogs()`), not as a scalar function.

`NULL` input flows through to `NULL`. Empty input compresses to the codec's valid
empty stream and round-trips back to empty. An out-of-range `level` clamps to the
codec's range; `level` is ignored on decompress and on the level-less codecs
(`snappy`, `snappy_raw`).

> A DuckDB scalar fixes its output column type at **bind time**, with no data
> sample available — so these functions return a fixed type per call (`BLOB`,
> `VARCHAR`, `UBIGINT`, `DOUBLE`, `BOOLEAN`, or `LIST<VARCHAR>`), never a
> data-dependent shape.

## The codec matrix

Frame-vs-raw variants are **distinct codec names**, because the byte layout
differs and a uniform API must not silently guess between them (auto-detection is
the explicit opt-in for guessing).

| codec | default / range | frame vs raw | magic bytes | backing crate |
| --- | --- | --- | --- | --- |
| `zstd` | 3 / −22–22 | framed | `28 b5 2f fd` | `zstd` (bundled libzstd) |
| `gzip` | 6 / 0–9 | gzip member | `1f 8b` | `flate2` |
| `zlib` | 6 / 0–9 | zlib + Adler-32 | `78 xx` (weak) | `flate2` |
| `deflate` | 6 / 0–9 | **raw** DEFLATE | none | `flate2` |
| `brotli` | 11 / 0–11 | stream | **none** | `brotli` (pure Rust) |
| `lz4` | 0 / 0–16 | **frame** (LZ4F) | `04 22 4d 18` | `lz4_flex` (pure Rust) |
| `lz4_block` | 0 / 0–16 | **raw block** | none | `lz4_flex` |
| `snappy` | n/a | **framed** | `ff 06 00 00 73 4e 61 50 70 59` | `snap` (pure Rust) |
| `snappy_raw` | n/a | **raw block** | none | `snap` |
| `xz` | 6 / 0–9 | xz container | `fd 37 7a 58 5a 00` | `lzma-rs` (pure Rust) |
| `lzma` | 6 / 0–9 | legacy `.lzma` | `5d 00 00` (weak) | `lzma-rs` |
| `bzip2` | 9 / 1–9 | bzip2 stream | `42 5a 68` ("BZh") | `bzip2` (`libbz2-rs-sys`, pure Rust) |

**Headerless codecs cannot be auto-detected.** Brotli, raw `deflate`,
`lz4_block`, and `snappy_raw` carry no magic bytes, so `detect_codec` returns
`'unknown'` and `decompress_auto` returns a `cannot auto-detect codec (no magic
bytes)` per-row error for them — use the explicit `decompress(blob, codec)` form.
This is documented, not a bug: auto-detection is magic-byte-only and best-effort.
Weak magics (`zlib`, `lzma`) are confirmed with a bounded trial decode so a
chance `78 xx` in arbitrary bytes is rejected rather than mis-reported.

## xz / lzma backend

The default xz/lzma backend is the **pure-Rust `lzma-rs`** (MIT, no C, smaller
trust surface). An optional `--features liblzma` routes xz through the C
`xz-utils` `liblzma` FFI for throughput (operators who vet their C toolchain);
`codecs()` reflects the build. Note that the pure-Rust `lzma-rs` *encoder* is a
basic implementation: it produces fully valid, round-trippable xz/lzma streams
but compresses poorly. Decode is always correct and bomb-guarded. Use
`--features liblzma`, or a strong codec like `zstd`/`bzip2`, when xz *ratio* or
*throughput* matters.

## Non-goals

Archive *formats* (zip/tar/7z directory structures — see DuckDB's `zipfs`/`tarfs`
extensions); dictionary-trained zstd; streaming/chunked cross-row compression
(DuckDB scalars are per-row buffered, out of scope by construction); encryption
(compose with `vgi-mask` / `vgi-crypto`, never bundle crypto into a codec
worker).

## Build & test

```bash
cargo build --release --bin compress-worker     # the worker binary
cargo test --workspace --all-features           # unit + golden vectors + proptest zero-panic gate
./run_tests.sh                                  # haybarn SQLLogic E2E (needs haybarn-unittest on PATH)
```

The repository builds standalone against the published VGI Rust SDK
(`vgi = "0.9.5"` from crates.io) and arrow 59 — no local SDK checkout, no `path`
dependency on the SDK. See [`ci/README.md`](ci/README.md) for the transport
matrix (subprocess / unix / http) the E2E suite runs across.

## License

MIT — see [LICENSE](LICENSE). Every default dependency is permissive (MIT /
Apache-2.0 / BSD-3-Clause); the `zstd` crate vendors libzstd under its BSD
option, so the binary is not GPL.

Part of the [Query.Farm](https://query.farm) VGI ecosystem of DuckDB workers.
