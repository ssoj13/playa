//! Media decoding and metadata — FFmpeg video, EXR (`vfx-exr`), generic images (`image`).
//!
//! Call [`init_ffmpeg`] once from the binary before decoding video.

#![allow(clippy::module_inception)]

pub mod dispatch;
pub mod error;
pub mod media;
pub mod pixel;
pub mod source_image;
pub mod video;

pub use dispatch::{decode_raster, header_attrs, AttrKv};
pub use error::IoError;
pub use pixel::{DecodedRaster, RawPixelBuffer, RawPixelFormat};
pub use source_image::{pick_display_layer, SourceImage};
pub use video::{decode_frame, get_video_dimensions, VideoMetadata};

/// Initialise FFmpeg runtime (logging silenced).
pub fn init_ffmpeg() -> Result<(), Box<dyn std::error::Error>> {
    playa_ffmpeg::init().map_err(|e| Box::<dyn std::error::Error>::from(e))
}
