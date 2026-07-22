//! The `compress` VGI worker (native binary entrypoint).
//!
//! A standalone binary DuckDB launches and talks to over Apache Arrow IPC
//! (`ATTACH 'compress' (TYPE vgi, LOCATION '…')`). All function registration and
//! catalog metadata live in the library crate ([`compress_worker::build_worker`])
//! so the browser (wasm) build shares them verbatim; this binary only wires up
//! logging and the native transport.

fn main() {
    // Logs MUST go to stderr — stdout is the Arrow-IPC channel.
    let _ = env_logger::Builder::from_env(env_logger::Env::default().filter_or("VGI_LOG", "info"))
        .format_timestamp_millis()
        .try_init();

    compress_worker::build_worker().run();
}
