//! `SourceImage` is unavailable without `feature = "exr"`.

use std::path::Path;

#[derive(Debug, Clone)]
pub struct SourceImage;

impl SourceImage {
    pub fn open_exr(_path: &Path) -> Result<Self, String> {
        Err("EXR support not enabled for this target".into())
    }
}

/// Returned only when callers have nothing to classify (Wasm / non-EXR builds).
#[inline]
pub fn pick_display_layer<T>(_: &T) -> usize {
    0
}
