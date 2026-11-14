# Compression notebooks

[01 · The compression boundary changes the result](01_where_to_put_the_batch_boundary.ipynb)
asks how much space batching saves when the input events and codec stay fixed.
Visible SQL generates two synthetic JSON streams, forms ordered batches, calls
the real VGI gzip/zstd functions and verifies exact bounded decompression. Two
embedded charts compare stored bytes across batch sizes and against individual
event compression. The end experiment changes only the selected batch boundary.

## Run from this directory

Install [uv](https://docs.astral.sh/uv/), then:

```sh
cd notebooks
uv sync --locked
uv run --locked python execute.py
```

In a notebook editor choose this directory's `.venv/bin/python` kernel and run
all cells. The full notebook directory also works when copied outside the worker
repository. Python 3.13–3.14 is supported; `uv` can provision the interpreter.
No Rust compiler, local worker build or root virtual environment is required.

The connection stays visible and uses a literal published release:

```sql
INSTALL vgi FROM community;
LOAD vgi;
ATTACH 'compress' (TYPE vgi,
  LOCATION 'github-auto://Query-farm/vgi-compress@v0.4.0');
```

VGI selects the native platform archive, verifies its SHA256 sidecar and caches
it. This release supplies macOS arm64 and Linux amd64/arm64 assets. The notebook
environment locks Haybarn 2.0.0rc11 and its visualization dependencies. Initial
installation needs network access; the analysis uses local synthetic bytes and
no credentials or data APIs. Change the visible release tag and rerun all checks
when adopting another release.

Parameters, connection SQL, analytical SQL and useful outputs remain visible.
Imports, styling, plotting, prose-generation source and cleanup are folded.

## One-parameter experiment

```sh
uv run --locked python execute.py --parameters '{"EVENTS_PER_BATCH":128}' \
  --output-dir validation/batch-128
```

The accepted sizes are 1, 8, 32 and 128; default 32, experiment 128. Each stream
always contains 512 events. All four reference sizes are calculated each run;
the selected parameter controls the highlighted points and detailed comparison.
Reset to 32 after comparing results.

The runner creates fresh kernels, rejects unknown parameter names and source
overwrites, and replaces saved notebooks only after successful execution. Copies
include branding, helpers, the locked environment and this guide. Logs, extracted
charts and experiments are ignored under `validation/`; the main notebook keeps
its embedded charts and successful outputs.

## Provenance and interpretation

The two synthetic JSON streams share a fixed envelope, sequence strings, event
counts and raw byte sizes. One payload repeats `ab`; the other uses deterministic
MD5 hexadecimal text for variety. This is not a sample of production telemetry,
and the digest stream is not claimed to be random or incompressible. MD5 is not
used as an integrity proof: the invariant checks compare complete decoded bytes.

SQL includes the newline after every event and orders records explicitly inside
each batch. All compression and decompression use the published VGI worker;
Python only configures the notebook and presents its returned data. Compressed
BLOBs are materialized before summary queries and plotting.

Gzip uses its default level 6 and zstd its default level 3. Those numbers are
codec-specific, not equal effort settings. Each batch is an independent stream.
Decoding calls enforce an explicit 1 MiB output cap per batch. Headers and codec
framing count toward stored bytes; database row metadata, indexes, filenames and
transport envelopes do not. One KiB is 1,024 bytes. The figures measure storage,
not encode/decode timing, system throughput or query latency.

Detection succeeds for the two framed codecs tested. The final Brotli control
shows that a valid stream can still have an `unknown` detected format; preserve
codec labels rather than assuming every format is self-identifying. Unknown,
invalid and NULL payloads are distinct states. A decompression-cap failure is an
error, not a successful empty payload.

Larger batches change the unit that must be decoded or replaced. Production
selection also needs access-pattern, update and latency measurements. These
examples establish the storage tradeoff for the specific fixed inputs only.

The official Strata Sun and VGI assets are unchanged Query.Farm media-kit files,
with linked Strata Sun branding at the beginning and final end of the notebook.

References: [worker documentation](https://github.com/Query-farm/vgi-compress),
[pinned release](https://github.com/Query-farm/vgi-compress/releases/tag/v0.4.0).
