//! Raster / sequence loading — delegated to [`playa_io`] (FFmpeg / EXR / generic).

use std::path::Path;

use playa_io::{AttrKv, RawPixelBuffer, RawPixelFormat, decode_raster, header_attrs};

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
        attrs.set(key, av_from_kv(kv));
    }
    attrs
}

/// Bridge one `playa_io::AttrKv` into the engine's `AttrValue`. Lossless: integer
/// attrs land in `Int64`, arrays in `List`, matrices reshape row-major into
/// `Mat3`/`Mat4`. f64 floats narrow to f32 — EXR authors floats as f32 on disk, so
/// this stays bit-exact against the file.
fn av_from_kv(kv: AttrKv) -> AttrValue {
    match kv {
        AttrKv::Str(s) => AttrValue::Str(s),
        AttrKv::UInt(u) => AttrValue::UInt(u),
        AttrKv::Float(f) => AttrValue::Float(f),
        AttrKv::Int64(i) => AttrValue::Int64(i),
        AttrKv::IntArray(v) => AttrValue::List(v.into_iter().map(AttrValue::Int64).collect()),
        AttrKv::FloatArray(v) => {
            AttrValue::List(v.into_iter().map(|f| AttrValue::Float(f as f32)).collect())
        }
        AttrKv::Matrix3(m) => AttrValue::Mat3([
            [m[0], m[1], m[2]],
            [m[3], m[4], m[5]],
            [m[6], m[7], m[8]],
        ]),
        AttrKv::Matrix4(m) => AttrValue::Mat4([
            [m[0], m[1], m[2], m[3]],
            [m[4], m[5], m[6], m[7]],
            [m[8], m[9], m[10], m[11]],
            [m[12], m[13], m[14], m[15]],
        ]),
    }
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
        header_attrs(path).map(attrs_from_io).map_err(Into::into)
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
