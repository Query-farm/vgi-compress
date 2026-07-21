//! Scalar functions exposed by the compress worker, registered under
//! `compress.main`.

mod codecs;
mod detect;
mod introspect;
mod transform;

use vgi::Worker;

/// Register every scalar function on the worker. The functions with an optional
/// trailing argument (`level` / `max_output_bytes`) register a 2-arg and a
/// 3-arg (resp. 1-arg / 2-arg) arity overload, because DuckDB binds a registered
/// signature by arity.
pub fn register(worker: &mut Worker) {
    // compress(blob, codec [, level])
    worker.register_scalar(transform::Compress { with_level: false });
    worker.register_scalar(transform::Compress { with_level: true });
    // decompress(blob, codec [, max_output_bytes])
    worker.register_scalar(transform::Decompress { with_cap: false });
    worker.register_scalar(transform::Decompress { with_cap: true });
    // decompress_auto(blob [, max_output_bytes])
    worker.register_scalar(transform::DecompressAuto { with_cap: false });
    worker.register_scalar(transform::DecompressAuto { with_cap: true });

    // detect_codec(blob)
    worker.register_scalar(detect::DetectCodec);

    // compressed_size(blob, codec [, level])
    worker.register_scalar(introspect::CompressedSize { with_level: false });
    worker.register_scalar(introspect::CompressedSize { with_level: true });
    // decompressed_size(blob, codec [, max_output_bytes])
    worker.register_scalar(introspect::DecompressedSize { with_cap: false });
    worker.register_scalar(introspect::DecompressedSize { with_cap: true });
    // ratio(blob, codec [, level])
    worker.register_scalar(introspect::Ratio { with_level: false });
    worker.register_scalar(introspect::Ratio { with_level: true });
    // is_valid(blob, codec)
    worker.register_scalar(introspect::IsValid);

    // codecs()
    worker.register_scalar(codecs::Codecs);
}
