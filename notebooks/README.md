# Compression boundary examples

[01 · The compression boundary changes the result](01_where_to_put_the_batch_boundary.ipynb)
compares identical synthetic JSON events compressed individually or in batches.
It preserves the original gzip/zstd comparison and two figures, adding a supported-codec
preflight, bounded automatic decode, capacity estimates and distinct empty/missing/
malformed-input controls. All nine public SQL functions contribute to the investigation.

Use the notebook's **Open in Colab** button, run the install cell, then **Run all**.
All payloads are generated in visible SQL; no clone, companion download, data file or
account is required. The base shared package preserves compatible preloaded plotting
and data libraries. Fresh Python 3.12 notebook-only execution with preloaded libraries
is tested separately; an actual hosted Colab session has not been tested.

For local execution, install Python 3.11+ and [uv](https://docs.astral.sh/uv/):

```sh
cd notebooks
uv sync --locked --extra runner
uv run --locked --extra runner vgi-notebooks
```

The optional `runner` extra supplies a local execution kernel. For interactive Jupyter,
register it with `uv run --locked --extra runner python -m ipykernel install --user
--name vgi-compress-notebooks --display-name 'VGI Compress notebooks'`, select that
kernel, then restart and run all after changing parameters.

The visible worker connection is:

```sql
INSTALL vgi FROM community;
LOAD vgi;
ATTACH 'compress' (TYPE vgi,
 LOCATION 'github-auto://Query-farm/vgi-compress@v0.4.0');
```

Haybarn 2.0.0rc11 downloads and verifies the native release asset for the platform.
No Rust compiler, checkout binary or dependency override is required. Initial Python,
extension and worker downloads require network access; all subsequent computation
uses synthetic bytes in memory. Native validation ran on macOS Apple silicon; release
assets determine availability on other platforms. The shared connection uses an empty
temporary SQL-secret directory with persistent secrets disabled and explicit cleanup.

## One-parameter experiment

```sh
uv run --locked --extra runner vgi-notebooks \
  --parameters '{"EVENTS_PER_BATCH":128}' --output-dir validation/batch-128
```

The default selects 32 events per batch. The experiment selects 128, changing each
512-event stream from 16 to 4 independently compressed units. Raw bytes remain 38,912
per stream. The largest measured saving against individual-event compression changes
from 93.3% (repeated payload / gzip) to 97.0% (repeated payload / zstd). These are results
for the stated synthetic inputs, not general codec rankings.

The reference sweep at 1, 8, 32 and 128 is unchanged; the chosen points and selected
comparison change. Every selected decode unit expands from 2,432 to 9,728 bytes.
Both exceed the separate 1,024-byte preview-size budget, while the full decoder's
explicit 1 MiB cap accommodates all generated batches. Fewer stored units do not
imply a smaller unit for retrieval, replacement or decoding.

Accepted values are integers 1, 8, 32 and 128; booleans are rejected. Reset to 32 for
the default. Invalid overrides and failed runs preserve the saved default and last
successful experiment.

## What the measurements mean

Each stream contains 512 JSON events, with the same envelope and 32-character payload.
One payload repeats `ab`; the other uses deterministic MD5 hexadecimal text for variety.
Digest text is not a claim of randomness or incompressibility. The ordered aggregation
includes one newline per event and preserves event order within every batch.

`codecs()` drives an explicit supported-codec join; we intentionally choose gzip and
zstd from the twelve available names. Their default levels (6 and 3) are codec-specific,
not equivalent effort settings. Materialized BLOBs drive the plots. The capacity audit
also uses `compressed_size` and `ratio`, which perform compression work again without
retaining their own output BLOBs. They are not cost-free header inspections.

The worker ratio is compressed / original; its weighted aggregate matches total stored
bytes / total original bytes. `decompressed_size` takes compressed input and returns
NULL when its cap prevents reporting the size. Some codecs can read a size trailer;
size introspection alone does not establish full-stream validity. `is_valid`, explicit
bounded decoding and automatic bounded decoding check different parts of the contract.

The valid Brotli control returns an unknown detector label but decodes with its declared
codec. Empty gzip decodes to zero bytes and has an undefined ratio; malformed gzip
returns false from `is_valid`, while missing input remains NULL. Those states are
preserved rather than collapsed into one successful-looking fallback.

This is a stored-byte investigation, not a speed benchmark. It excludes database row
headers, indexes and transport envelopes. A production choice also needs measurements
of latency, update frequency, retrieval granularity and the actual payload distribution.
Twelve SQL checks plus capability/aggregate assertions validate the demonstrated
contracts; they do not cover every codec, level or hostile stream.

## Shared tools and coverage

The [shared standard and runner](https://github.com/Query-farm/vgi-example-notebooks/blob/v0.3.6/docs/standard.md)
are pinned at **v0.3.6**. There are no copied presentation or execution helpers.
Charts retain native SVG and content-addressed `charts/` assets for GitHub; experiment
copies keep native SVG. Retain prior published assets when updating a notebook.

[Coverage evidence](coverage.json) verifies **9/9** logical SQL surfaces in both
released v0.4.0 and the reviewed current-public interface. `coverage/` contains complete
actual function/table/view exports, source compatibility review, successful execution
hashes and explicit narrative/result interpretation. Overloads count once per
`(schema,name)`. Current-source discovery is separate from the native release used
for the analytical notebook. Reviewed release/current changes clarify documentation,
add composed examples and add tests; codec logic and public signatures are unchanged.
