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
        "exr", "png", "jpg", "jpeg", "tif", "tiff", "tga", "hdr", "mp4", "mov", "avi", "mkv",
    ];

    /// Check if file is a video format
    /// Handles video frame paths with @N suffix (e.g., "video.mp4@135")
    pub fn is_video(path: &Path) -> bool {
        // Strip @N suffix if present (video frame indicator)
        let path_str = path.to_string_lossy();
        let base_path = if let Some(at_pos) = path_str.rfind('@') {
            // Check if everything after @ is a number
            if path_str[at_pos + 1..].chars().all(|c| c.is_ascii_digit()) {
                &path_str[..at_pos]
            } else {
                &path_str[..]
            }
        } else {
            &path_str[..]
        };

        Path::new(base_path)
            .extension()
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
