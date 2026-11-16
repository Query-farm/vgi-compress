# ci/ — integration harness

| file | purpose |
| --- | --- |
| `run-integration.sh` | Run `test/sql/*.test` through the published `haybarn-unittest` with the signed community `vgi` extension, over one `TRANSPORT` (`stdio` \| `launch` \| `http`). Sets `VGI_COMPRESS_WORKER` (the ATTACH `LOCATION`) per transport. Adapted from the vgi-starters harness. |
| `check-version.sh` | Assert the `[workspace.package]` version equals a release tag before publishing. |

## Run locally

```bash
make test-sql                       # build, install haybarn-unittest into .tools, run stdio + launch + http
make test-sql TRANSPORTS=launch     # one transport
TEST_PATTERN=test/sql/<file>.test make test-sql TRANSPORTS=stdio   # one file
```

Transports: `stdio` — DuckDB spawns the worker per connection; `launch` —
DuckDB starts the worker once on a unix socket (`LOCATION 'launch:/path'`) and
pools connections; `http` — the worker runs `--http --port 0` and prints
`PORT:<n>`.

The script stages the tests, rewrites `require vgi` into `LOAD vgi` and loads
`httpfs` on the http transport. It runs with a config that turns off the
runner's error-to-skip rule, and fails on any skip not listed in
`EXPECTED_SKIP_REASONS` and when fewer than `MIN_EXECUTED` files ran. A broken
lane therefore cannot pass as all skips.

The Docker image test (`.github/workflows/docker-publish.yml`) reuses the script
with a preset LOCATION (`docker run -i … stdio` with `TRANSPORT=stdio`, or the
container's `http://` URL with `TRANSPORT=http`).
