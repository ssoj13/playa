//! Media decoding — **FFmpeg video** (`feature = "ffmpeg"`), **EXR** (`feature = "exr"`),
//! generic images (`image`), and a **WebCodecs** scaffolding module for future Wasm/Web targets.
//!
//! Call [`init_ffmpeg`] once from the desktop binary before decoding video (`feature = "ffmpeg"`).
//! On Wasm/minimal builds it is a no-op.

#![allow(clippy::module_inception)]

pub mod dispatch;
pub mod error;
#[cfg(feature = "exr")]
pub mod exr_layered;
pub mod media;
pub mod pixel;
pub mod source_image;
pub mod video;
pub mod webcodecs;

#[cfg(feature = "ffmpeg")]
pub use ::playa_ffmpeg as ffmpeg;

pub use dispatch::{AttrKv, decode_raster, header_attrs};
pub use error::IoError;
pub use pixel::{DecodedRaster, RawPixelBuffer, RawPixelFormat};
pub use source_image::{SourceImage, pick_display_layer};
pub use video::{VideoMetadata, decode_frame, get_video_dimensions};

/// Initialise FFmpeg runtime (`feature = "ffmpeg"`).
#[cfg(feature = "ffmpeg")]
pub fn init_ffmpeg() -> Result<(), Box<dyn std::error::Error>> {
    playa_ffmpeg::init().map_err(|e| Box::<dyn std::error::Error>::from(e))
}

#[cfg(not(feature = "ffmpeg"))]
pub fn init_ffmpeg() -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}
