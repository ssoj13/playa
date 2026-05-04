//! Browser **WebCodecs** path (Wasm hosts). There is no portable WebCodecs API on desktop
//! natives — use `feature = "ffmpeg"` for H.264/H.265 there.
//!
//! Enable this module with `feature = "webcodecs"` once `wasm-bindgen` + `web_sys` bindings
//! are wired; today it intentionally stays API-free scaffolding.

#![allow(dead_code)]

/// Placeholder: future `VideoDecoder`-backed ingest from `EncodedVideoChunk`.
pub enum WebCodecPath {
    /// Not implemented yet.
    Stub,
}
