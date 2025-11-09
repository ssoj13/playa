//! Utility functions and constants
//!
//! **Why**: Centralized helpers used across multiple modules
//!
//! **Used by**: cache, sequence, frame, ui modules

/// Media file type detection
pub mod media {
    use std::path::Path;

    /// Supported video file extensions
    pub const VIDEO_EXTS: &[&str] = &["mp4", "mov", "avi", "mkv"];

    /// Supported image file extensions
    #[allow(dead_code)]
    pub const IMAGE_EXTS: &[&str] = &["exr", "png", "jpg", "jpeg", "tif", "tiff", "tga", "hdr"];

    /// All supported file extensions (video + image)
    pub const ALL_EXTS: &[&str] = &[
        "exr", "png", "jpg", "jpeg", "tif", "tiff", "tga", "hdr",
        "mp4", "mov", "avi", "mkv",
    ];

    /// Check if file is a video format
    pub fn is_video(path: &Path) -> bool {
        path.extension()
            .and_then(|s| s.to_str())
            .map(|s| VIDEO_EXTS.contains(&s.to_lowercase().as_str()))
            .unwrap_or(false)
    }

    /// Check if file is an image format
    #[allow(dead_code)]
    pub fn is_image(path: &Path) -> bool {
        path.extension()
            .and_then(|s| s.to_str())
            .map(|s| IMAGE_EXTS.contains(&s.to_lowercase().as_str()))
            .unwrap_or(false)
    }
}
