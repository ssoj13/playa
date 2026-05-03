//! Raster / sequence loading — delegated to [`playa_io`] (FFmpeg / EXR / generic).

use std::path::Path;

use playa_io::{decode_raster, header_attrs, AttrKv, RawPixelBuffer, RawPixelFormat};

use super::frame::{Frame, FrameError, PixelBuffer, PixelFormat};
use crate::entities::{AttrValue, Attrs};

/// Image loader with metadata support (`playa-io` backends).
pub struct Loader;

impl From<playa_io::IoError> for FrameError {
    fn from(e: playa_io::IoError) -> Self {
        match e {
            playa_io::IoError::Exr(s) => FrameError::Exr(s),
            playa_io::IoError::Image(s) => FrameError::Image(s),
            playa_io::IoError::LoadError(s) => FrameError::LoadError(s),
            playa_io::IoError::UnsupportedFormat(s) => FrameError::UnsupportedFormat(s),
        }
    }
}

fn attrs_from_io(entries: Vec<(String, AttrKv)>) -> Attrs {
    let mut attrs = Attrs::new();
    for (key, kv) in entries {
        match kv {
            AttrKv::Str(s) => attrs.set(key, AttrValue::Str(s)),
            AttrKv::UInt(u) => attrs.set(key, AttrValue::UInt(u)),
            AttrKv::Float(f) => attrs.set(key, AttrValue::Float(f)),
        }
    }
    attrs
}

fn pb_from_raw(b: RawPixelBuffer) -> PixelBuffer {
    match b {
        RawPixelBuffer::U8(v) => PixelBuffer::U8(v),
        RawPixelBuffer::F16(v) => PixelBuffer::F16(v),
        RawPixelBuffer::F32(v) => PixelBuffer::F32(v),
    }
}

fn pf_from_raw(f: RawPixelFormat) -> PixelFormat {
    match f {
        RawPixelFormat::Rgba8 => PixelFormat::Rgba8,
        RawPixelFormat::RgbaF16 => PixelFormat::RgbaF16,
        RawPixelFormat::RgbaF32 => PixelFormat::RgbaF32,
    }
}

impl Loader {
    pub fn header(path: &Path) -> Result<Attrs, FrameError> {
        header_attrs(path)
            .map(attrs_from_io)
            .map_err(Into::into)
    }

    pub fn load(path: &Path) -> Result<Frame, FrameError> {
        let dec = decode_raster(path)?;
        Ok(Frame::from_buffer(
            pb_from_raw(dec.buffer),
            pf_from_raw(dec.format),
            dec.width,
            dec.height,
        ))
    }
}
