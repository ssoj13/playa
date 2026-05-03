//! Media extensions and path helpers (sequences, `@frame` suffix for video URLs).

use std::path::Path;

/// Supported video file extensions (lowercase, no dot).
pub const VIDEO_EXTS: &[&str] = &["mp4", "mov", "avi", "mkv"];

/// All supported extensions (video + raster).
pub const ALL_EXTS: &[&str] = &[
    "exr", "png", "jpg", "jpeg", "tif", "tiff", "tga", "hdr", "mp4", "mov", "avi", "mkv",
];

/// True if path points at a video container (handles `clip.mp4@135` notation).
pub fn is_video(path: &Path) -> bool {
    let path_str = path.to_string_lossy();
    let base_path = if let Some(at_pos) = path_str.rfind('@') {
        if path_str[at_pos + 1..].chars().all(|c| c.is_ascii_digit()) {
            &path_str[..at_pos]
        } else {
            path_str.as_ref()
        }
    } else {
        path_str.as_ref()
    };

    Path::new(base_path)
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| VIDEO_EXTS.contains(&s.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Strip optional `@frame` suffix.
/// `"video.mp4@17"` → `(video.mp4, Some(17))`
pub fn parse_video_path(path: &Path) -> (std::path::PathBuf, Option<usize>) {
    let path_str = path.to_string_lossy();

    if let Some(at_pos) = path_str.rfind('@') {
        let suffix = &path_str[at_pos + 1..];
        if suffix.chars().all(|c| c.is_ascii_digit()) && !suffix.is_empty() {
            let base = &path_str[..at_pos];
            let frame_num = suffix.parse().ok();
            return (std::path::PathBuf::from(base), frame_num);
        }
    }

    (path.to_path_buf(), None)
}
