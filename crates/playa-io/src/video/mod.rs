//! Video metadata / decode (`feature = "ffmpeg"`) vs stub returning [`IoError`].

#[cfg(feature = "ffmpeg")]
mod ffmpeg_imp;

#[cfg(feature = "ffmpeg")]
pub use ffmpeg_imp::{VideoMetadata, decode_frame, get_video_dimensions};

#[cfg(not(feature = "ffmpeg"))]
mod stub;
#[cfg(not(feature = "ffmpeg"))]
#[allow(dead_code)] // FFmpeg path omitted; dispatcher still references the symbol uniformly.
pub(crate) fn init_ffmpeg_logging() {}

#[cfg(not(feature = "ffmpeg"))]
pub use stub::{VideoMetadata, decode_frame, get_video_dimensions};
