//! Video paths disabled (no FFmpeg / Wasm build).

use std::path::Path;

use crate::error::IoError;
use crate::pixel::{RawPixelBuffer, RawPixelFormat};

pub struct VideoMetadata {
    pub frame_count: usize,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
}

impl VideoMetadata {
    pub fn from_file(_path: &Path) -> Result<Self, IoError> {
        Err(IoError::UnsupportedFormat(
            "Video decode not available (compiled without FFmpeg; use WebCodecs on Wasm)"
                .to_string(),
        ))
    }
}

pub fn get_video_dimensions(_path: &Path) -> Result<(usize, usize), IoError> {
    Err(IoError::UnsupportedFormat(
        "Video decode not available (compiled without FFmpeg; use WebCodecs on Wasm)".to_string(),
    ))
}

pub fn decode_frame(
    _path: &Path,
    _frame_num: usize,
) -> Result<(RawPixelBuffer, RawPixelFormat, usize, usize), IoError> {
    Err(IoError::UnsupportedFormat(
        "Video decode not available (compiled without FFmpeg; use WebCodecs on Wasm)".to_string(),
    ))
}
