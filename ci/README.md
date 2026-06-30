# CI: the vgi-compress worker integration suite

[`.github/workflows/ci.yml`](../.github/workflows/ci.yml) runs fmt/clippy/doc/
build, the Rust unit + proptest tests (the zero-panic decode gate), and this
repo's sqllogictest suite (`test/sql/*.test`) against the vgi-compress VGI
worker through the **real DuckDB `vgi` extension** on every push / PR.

## Transport matrix

The integration suite runs over **every transport the vgi extension supports**.
The exact same `test/sql/*.test` files run three ways; the only thing that
changes is what LOCATION the `.test` files `ATTACH` (set by
[`run-integration.sh`](run-integration.sh) from the `TRANSPORT` env var):

| `TRANSPORT`  | `VGI_COMPRESS_WORKER` (the ATTACH LOCATION) | how the worker is launched |
|--------------|---------------------------------------------|----------------------------|
| `subprocess` | `…/target/release/compress-worker`          | DuckDB spawns the stdio binary (default) |
| `http`       | `http://127.0.0.1:<port>`                   | `compress-worker --http` (auto port; prints `PORT:<n>` on stdout, which the script polls for) |
| `unix`       | `unix:///tmp/compress.<pid>.sock`           | `compress-worker --unix <sock>` (prints `UNIX:<sock>` on stdout + creates the socket; the script waits for both) |

CI runs `transport: [subprocess, http, unix]` × `os: [ubuntu, macos]` as a
matrix. Build the worker once with a plain `cargo build --release` — the
workspace already pins `vgi-rpc = { features = ["macros", "http"] }`, so the one
binary serves all three transports; **no extra cargo feature is needed**.

### The `http` leg needs DuckDB's `httpfs` extension

The vgi extension's **HTTP client** is built on DuckDB's `httpfs`. Over `http://`,
`ATTACH` fails without it, and crucially that error message contains the
substring **`HTTP`**, which DuckDB's sqllogictest runner **silently SKIPs** by
default — a deceptive pass-by-skip. We handle it in two places:

1. [`preprocess-require.awk`](preprocess-require.awk), invoked with
   `-v transport=http`, injects a signed `INSTALL httpfs FROM core; LOAD httpfs;`
   right after each `LOAD vgi;` so the http leg actually loads the client and runs.
2. [`run-integration.sh`](run-integration.sh) fails the job if the runner reports
   *any* skipped tests (a skip is never a pass).

The `unix` (AF_UNIX launcher) leg needs no extra extension.

## How it works (no C++ build)

Rather than building the vgi DuckDB extension from source, the integration job
drives a **prebuilt** standalone `haybarn-unittest` (the DuckDB/Haybarn
sqllogictest runner, published in Haybarn's releases) and installs the
**signed** `vgi` extension from the Haybarn community channel:

1. **Build the worker** — `cargo build --release --bin compress-worker`. The
   compiled `target/release/compress-worker` is a self-contained stdio worker the
   extension spawns (the `.test` files `ATTACH` it via `${VGI_COMPRESS_WORKER}`).
2. **Download the runner** — the matching `haybarn_unittest-*` asset per platform.
3. **Preprocess** — [`preprocess-require.awk`](preprocess-require.awk) rewrites
   each `require <ext>` into an explicit signed `INSTALL … FROM {community,core};
   LOAD …;`. The vgi-compress `.test` files already `LOAD vgi;` explicitly and
   use `require-env VGI_COMPRESS_WORKER`, so the `require`-rewrite is a no-op
   here (the awk still hooks `LOAD vgi;` for the http leg's httpfs injection).
4. **Run** — [`run-integration.sh`](run-integration.sh) brings up the worker for
   the selected `TRANSPORT`, stages the preprocessed tree, warms the extension
   cache once (`INSTALL vgi FROM community;`), then runs the suite. Any failed
   assertion — or any skipped test — fails the job.

## Run it locally

```bash
cargo build --release --bin compress-worker
HAYBARN_UNITTEST=/path/to/haybarn-unittest \
WORKER_BIN="$PWD/target/release/compress-worker" \
TRANSPORT=subprocess \
  ci/run-integration.sh
```

Or use [`run_tests.sh`](../run_tests.sh), which builds the release worker and
runs the suite against a `haybarn-unittest` on `PATH`
(`uv tool install haybarn-unittest`).
