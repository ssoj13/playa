//! Shared file dialog helpers for widget UI.

/// Create configured file dialog for image/video selection.
pub fn create_media_dialog(title: &str) -> rfd::FileDialog {
    rfd::FileDialog::new()
        .add_filter("All Supported Files", crate::utils::media::ALL_EXTS)
        .set_title(title)
}
