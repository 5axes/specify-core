//! The Specify app and WASM engine built into this command (ADR P12; build.rs): each file
//! gzipped, by its `/`-separated path, sorted. Empty when the build had none.

use std::io::Read;

use flate2::read::GzDecoder;

/// Files by path, each gzipped.
pub(super) type Files = &'static [(&'static str, &'static [u8])];

include!(concat!(env!("OUT_DIR"), "/bundle.rs"));

/// The built-in app and engine, when this build has them.
pub(super) fn app() -> Option<Files> {
    (!APP.is_empty()).then_some(APP)
}

pub(super) fn wasm() -> Option<Files> {
    (!WASM.is_empty()).then_some(WASM)
}

/// The file at `path`, decompressed.
pub(super) fn file(files: Files, path: &str) -> Option<Vec<u8>> {
    let index = files.binary_search_by(|(p, _)| (*p).cmp(path)).ok()?;
    let mut bytes = Vec::new();
    GzDecoder::new(files[index].1)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(bytes)
}
