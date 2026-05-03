//! Decoding errors (mapped to [`playa_engine::entities::frame::FrameError`] at the boundary).

#[derive(Debug, Clone)]
pub enum IoError {
    Exr(String),
    Image(String),
    LoadError(String),
    UnsupportedFormat(String),
}
