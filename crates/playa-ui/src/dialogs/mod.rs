//! Dialogs - modal and non-modal dialog windows
//!
//! Preferences, encoder settings

#[cfg(not(target_arch = "wasm32"))]
pub mod encode;
#[cfg(target_arch = "wasm32")]
#[path = "encode_stub_wasm.rs"]
pub mod encode;
pub mod prefs;
