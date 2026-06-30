# CLAUDE.md

Guidance for working in this repository.

## What this is

`vgi-compress` is a **VGI worker** (a standalone binary DuckDB launches and talks
to over Apache Arrow IPC, `ATTACH 'compress' (TYPE vgi, LOCATION '…')`) that
exposes **multi-codec compress / decompress scalar functions over a DuckDB
`BLOB` column** — `zstd`, `gzip`, `zlib`, `deflate`, `brotli`, `lz4` (frame +
block), `snappy` (framed + raw), `xz`, `lzma`, `bzip2` — plus codec
auto-detection by magic bytes, level control, and size/ratio introspection.
Functions live under catalog `compress`, schema `main`.

Built on the published VGI Rust SDK (`vgi = "0.9.5"` from crates.io), arrow 59.
Modeled on `../vgi-cbor` (the MIT BLOB-transform reference) and
`../vgi-fixedformat`. The repo builds standalone — no local SDK checkout, no
`path` dependency on the SDK.

The whole worker is a **pure commodity primitive**: every codec is a mature
library and DuckDB already decompresses files. The value is the missing *scalar*
API — an in-SQL, per-row, uniform multi-codec surface to decode a *column* of
mixed-codec payloads, transcode in place, or audit size/ratio without leaving the
engine. Bundle it under the serialization / streaming / security workers; it is
not a standalone product.

## Layout

```
crates/compress-core/      # pure compute, NO arrow/vgi deps — independently testable
  src/error.rs             #   CodecError taxonomy (UnknownCodec/OutputTooLarge/Corrupt/NoMagic)
  src/guard.rs             #   BoundedWriter — the decompression-bomb guard (counting Write sink)
  src/codec/mod.rs         #   Codec enum, parse(name), level clamp, compress/decompress dispatch
  src/codec/{flate,zstd,brotli,lz4,snappy,xz,bzip2}.rs   # per-codec backends
  src/detect.rs            #   magic-byte table + trial-decode confirmation (detect)
  src/lib.rs               #   high-level API: compress/decompress/decompress_auto/detect_codec/
                           #     compressed_size/decompressed_size/ratio/is_valid/codecs
  tests/vectors.rs         #   golden round-trip per codec + cross-variant + bomb guard
  tests/fuzz.rs            #   proptest zero-panic gate (arbitrary + truncated bytes)
crates/compress-worker/    # arrow + vgi: maps core results onto DuckDB types, serves VGI
  src/main.rs              #   bootstrap + catalog/schema metadata (source_url + tags)
  src/arrow_io.rs          #   blob/codec/int reading + column builders (incl LIST<VARCHAR>)
  src/config.rs            #   max_output_bytes cap resolution (arg > setting > env > 256 MiB)
  src/meta.rs              #   per-object discovery tags (title/doc_llm/doc_md/keywords)
  src/scalar/transform.rs  #   compress, decompress, decompress_auto (BLOB -> BLOB)
  src/scalar/detect.rs     #   detect_codec
  src/scalar/introspect.rs #   compressed_size, decompressed_size, ratio, is_valid
  src/scalar/codecs.rs     #   codecs() -> LIST<VARCHAR>
  src/scalar/version.rs    #   compress_version()
test/sql/compress.test     # haybarn SQLLogic E2E over in-engine fixtures (no external files)
ci/                        # check-version.sh, run-integration.sh, preprocess-require.awk
```

## Design discipline (untrusted input — the load-bearing part)

- **Every decode is bounded.** `guard::BoundedWriter` caps output at
  `max_output_bytes`; a decompression bomb aborts the row with
  `CodecError::OutputTooLarge`, the worker never OOMs. The size-prefixed codecs
  (`lz4_block`, `snappy_raw`) reject before allocating by reading the declared
  size; the streaming codecs drain through the bounded sink.
- **Every decode returns a `Result`, never panics.** Truncated / garbage /
  wrong-codec input is `CodecError::Corrupt`. The worker maps engine errors to
  per-row DuckDB errors (`RpcError::value_error`); the process never crashes.
  Zero panics on arbitrary/truncated bytes is the `tests/fuzz.rs` proptest gate.
- **NULL flows through to NULL; empty round-trips to empty.** `is_valid` and
  `detect_codec` are total (never throw) — bad input is `false` / `'unknown'`.
- A DuckDB scalar's output type is fixed at **bind time** (no data sample), so
  each function returns one fixed Arrow type per call.

## Conventions

- Optional trailing args (`level`, `max_output_bytes`) register a 2-arg and a
  3-arg (resp. 1-arg / 2-arg) **arity overload** — DuckDB binds a signature by
  arity. The `blob` / `codec` / `level` / `max_output_bytes` arguments are read
  **per row** from the process batch, so SQL literals and columns both work.
- Distinct overloads must carry distinct descriptions (VGI120) and bind-able,
  self-contained examples (VGI901). Keep `vgi-lint` green at `--fail-on info`.
- No table functions, no externalized cursor, no secret provider, no network —
  a pure stateless local-CPU transform worker.

## Gates (all green)

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace
cargo build --release --bin compress-worker
./run_tests.sh                                          # haybarn SQLLogic E2E
uvx --from vgi-lint-check==0.37.0 vgi-lint lint \
    target/release/compress-worker --catalog compress --fail-on info   # 100/100, no findings
```

The `--features liblzma` build/test is a separate CI leg (C `xz-utils` backend,
off by default). The E2E runs across the subprocess / unix / http transport
matrix (see `ci/README.md`).

## License

MIT (fleet convention — matches `vgi-cbor`). Every default dependency is
permissive; `zstd` vendors libzstd under its BSD option (not GPL).
