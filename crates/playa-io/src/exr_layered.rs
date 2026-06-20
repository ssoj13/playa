//! Layered EXR writers/readers (`vfx-io`) exposed through `playa-io` so exporters
//! avoid direct `vfx-*` Cargo deps.

use std::path::Path;

use crate::IoError;

// `AttrValue` is the type of `ImageSpec.attributes` — it lives in vfx-core, NOT
// vfx-io (vfx-io has a separate, unrelated `AttrValue`). The encode dialog
// inserts per-layer compression into `layer.spec.attributes` through this
// re-export, so it must be exactly `vfx_core::AttrValue`.
pub use vfx_core::AttrValue;
pub use vfx_io::{
    ChannelKind, ChannelSampleType, ChannelSamples, ImageChannel, ImageLayer, LayeredImage,
    Metadata,
};

fn map_layers_err(e: vfx_io::IoError) -> IoError {
    IoError::Exr(e.to_string())
}

pub fn write_exr_layers(path: impl AsRef<Path>, layered: &LayeredImage) -> Result<(), IoError> {
    vfx_io::exr::write_layers(path.as_ref(), layered).map_err(map_layers_err)
}

pub fn read_exr_layers_passthrough(path: impl AsRef<Path>) -> Result<LayeredImage, IoError> {
    vfx_io::exr::read_layers_passthrough(path.as_ref()).map_err(map_layers_err)
}

pub fn write_exr_layers_passthrough(
    path: impl AsRef<Path>,
    layered: &LayeredImage,
) -> Result<(), IoError> {
    vfx_io::exr::write_layers_passthrough(path.as_ref(), layered).map_err(map_layers_err)
}
