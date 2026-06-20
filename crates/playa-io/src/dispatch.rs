//! Unified header + decode dispatcher for supported media extensions.

use log::trace;
use std::path::Path;

use crate::error::IoError;
use crate::media;
use crate::pixel::{DecodedRaster, RawPixelBuffer, RawPixelFormat};
use crate::video;

/// Serialized header field for engine `Attrs` bridging.
///
/// Unified carrier for every header/metadata value a backend surfaces. The scalar
/// arms (`Str`/`UInt`/`Float`) serve video + generic headers; the wider arms
/// mirror `vfx_core::AttrValue` 1:1 so the full EXR attribute set (chromaticities,
/// timecode, framesPerSecond, camera matrices, …) round-trips into the engine
/// without loss. `Int64` carries integer attrs whose u32 flag bits overflow i32
/// (EXR `TimeCode`/`KeyCode`).
#[derive(Debug, Clone)]
pub enum AttrKv {
    Str(String),
    UInt(u32),
    Float(f32),
    /// 64-bit integer (`vfx_core::AttrValue::Int`).
    Int64(i64),
    /// Integer array (`vfx_core::AttrValue::IntArray` — e.g. SMPTE timecode).
    IntArray(Vec<i64>),
    /// Float array (`vfx_core::AttrValue::FloatArray` — e.g. chromaticities[8]).
    FloatArray(Vec<f64>),
    /// 3×3 matrix, row-major (`vfx_core::AttrValue::Matrix3`).
    Matrix3([f32; 9]),
    /// 4×4 matrix, row-major (`vfx_core::AttrValue::Matrix4` — e.g. worldToCamera).
    Matrix4([f32; 16]),
}

enum FileKind {
    Video,
    Exr,
    Hdr,
    Generic,
}

fn classify_ext(ext: &str) -> FileKind {
    if media::VIDEO_EXTS.contains(&ext) {
        FileKind::Video
    } else if ext == "exr" {
        FileKind::Exr
    } else if ext == "hdr" {
        FileKind::Hdr
    } else {
        FileKind::Generic
    }
}

fn path_ext(path: &Path) -> String {
    path.extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase()
}

pub fn header_attrs(path: &Path) -> Result<Vec<(String, AttrKv)>, IoError> {
    match classify_ext(&path_ext(path)) {
        FileKind::Video => header_video(path),
        FileKind::Exr => header_exr(path),
        FileKind::Hdr | FileKind::Generic => header_generic(path),
    }
}

pub fn decode_raster(path: &Path) -> Result<DecodedRaster, IoError> {
    match classify_ext(&path_ext(path)) {
        FileKind::Video => decode_video(path),
        FileKind::Exr => decode_exr(path),
        FileKind::Hdr => decode_hdr(path),
        FileKind::Generic => decode_generic(path),
    }
}

fn header_video(path: &Path) -> Result<Vec<(String, AttrKv)>, IoError> {
    let (actual_path, _) = media::parse_video_path(path);
    let meta = video::VideoMetadata::from_file(&actual_path)?;
    Ok(vec![
        ("width".into(), AttrKv::UInt(meta.width)),
        ("height".into(), AttrKv::UInt(meta.height)),
        (
            "format".into(),
            AttrKv::Str(format!("Video ({})", actual_path.display())),
        ),
        ("channels".into(), AttrKv::UInt(3)),
        ("frames".into(), AttrKv::UInt(meta.frame_count as u32)),
        ("fps".into(), AttrKv::Float(meta.fps as f32)),
    ])
}

fn decode_video(path: &Path) -> Result<DecodedRaster, IoError> {
    let (actual_path, frame_idx) = media::parse_video_path(path);
    let frame_num = frame_idx.unwrap_or(0);
    let (buffer, format, width, height) = video::decode_frame(&actual_path, frame_num)?;
    Ok(DecodedRaster {
        buffer,
        format,
        width,
        height,
    })
}

#[cfg(not(feature = "exr"))]
fn header_exr(_path: &Path) -> Result<Vec<(String, AttrKv)>, IoError> {
    Err(IoError::UnsupportedFormat(
        "EXR decoding is disabled for this build (Wasm / stripped I/O)".to_string(),
    ))
}

/// Map a `vfx_core::AttrValue` (the element type of `ImageSpec.attributes`) into
/// the engine-facing [`AttrKv`]. 1:1 and lossless at this layer: `Float` keeps
/// f64 precision until the engine bridge narrows it to f32 (EXR floats are f32 on
/// disk, so the round-trip stays bit-exact). Arrays and matrices pass through.
#[cfg(feature = "exr")]
fn kv_from_core(v: &vfx_core::AttrValue) -> AttrKv {
    use vfx_core::AttrValue as C;
    match v {
        C::Int(i) => AttrKv::Int64(*i),
        C::Float(f) => AttrKv::Float(*f as f32),
        C::String(s) => AttrKv::Str(s.clone()),
        C::IntArray(a) => AttrKv::IntArray(a.clone()),
        C::FloatArray(a) => AttrKv::FloatArray(a.clone()),
        C::Matrix3(m) => AttrKv::Matrix3(*m),
        C::Matrix4(m) => AttrKv::Matrix4(*m),
    }
}

#[cfg(feature = "exr")]
fn header_exr(path: &Path) -> Result<Vec<(String, AttrKv)>, IoError> {
    trace!("Reading EXR header (vfx-io passthrough): {}", path.display());

    // Pass-through read parses every part header but does NOT decompress pixels,
    // so it cheaply exposes dimensions / channels / compression / layer names
    // (`spec` is fully populated; `channels` stays empty — pixels live in chunks).
    let layered = vfx_io::exr::read_layers_passthrough(path)
        .map_err(|e| IoError::Exr(format!("EXR header error: {}", e)))?;

    let first = layered
        .layers
        .first()
        .ok_or_else(|| IoError::Exr("EXR has no layers".to_string()))?;

    let width = first.width;
    let height = first.height;
    let channel_count = first.spec.channel_names.len() as u32;
    let layer_count = layered.layers.len() as u32;
    let compression_str = first
        .spec
        .attributes
        .get("compression")
        .and_then(|v| v.as_str())
        .unwrap_or("zip")
        .to_string();

    let channel_names: String = first.spec.channel_names.join(",");

    let layer_names: String = layered
        .layers
        .iter()
        .enumerate()
        .map(|(i, l)| {
            if l.name.is_empty() {
                format!("Layer{}", i)
            } else {
                l.name.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(",");

    let mut v = vec![
        ("width".into(), AttrKv::UInt(width)),
        ("height".into(), AttrKv::UInt(height)),
        (
            "format".into(),
            AttrKv::Str(format!("EXR ({})", compression_str)),
        ),
        ("compression".into(), AttrKv::Str(compression_str)),
        ("channels".into(), AttrKv::UInt(channel_count)),
        ("channel_names".into(), AttrKv::Str(channel_names)),
        ("layers".into(), AttrKv::UInt(layer_count)),
    ];
    if layer_count > 1 {
        v.push(("layer_names".into(), AttrKv::Str(layer_names)));
    }

    // Absorb the FULL authored attribute set from every part, namespaced under
    // `exr:` (and `exr:<layer>:` for parts beyond the first) so nothing the file
    // carries is dropped: chromaticities, smpte:TimeCode, FramesPerSecond, owner,
    // comments, worldToCamera and any custom attr land here as typed `AttrKv`.
    // `read_layers_passthrough` is the exhaustive TYPED source (vs `read()` which
    // stringifies). Derived convenience keys above stay unprefixed for the UI.
    for (li, layer) in layered.layers.iter().enumerate() {
        let prefix = if li == 0 {
            "exr".to_string()
        } else if layer.name.is_empty() {
            format!("exr:Layer{li}")
        } else {
            format!("exr:{}", layer.name)
        };
        for (name, val) in &layer.spec.attributes {
            v.push((format!("{prefix}:{name}"), kv_from_core(val)));
        }
    }

    Ok(v)
}

#[cfg(not(feature = "exr"))]
fn decode_exr(_path: &Path) -> Result<DecodedRaster, IoError> {
    Err(IoError::UnsupportedFormat(
        "EXR decoding is disabled for this build (Wasm / stripped I/O)".to_string(),
    ))
}

#[cfg(feature = "exr")]
fn decode_exr(path: &Path) -> Result<DecodedRaster, IoError> {
    trace!("Loading EXR (vfx-io): {}", path.display());
    use half::f16;

    // Canonical single-image read: vfx-io decodes the first RGBA layer and
    // assembles interleaved RGBA f32 with the faithful OIIO fallbacks (missing
    // G/B copy R, missing A = 1.0), also handling multi-part and subsampled
    // luminance-chroma files. `img.format` reports the *authored* bit depth so we
    // can keep half frames compact; the pixel buffer is always f32 in memory.
    let img = vfx_io::exr::read(path)
        .map_err(|e| IoError::Exr(format!("EXR decode error: {}", e)))?;

    let width = img.width as usize;
    let height = img.height as usize;
    let authored = img.format;

    let rgba_f32: Vec<f32> = match img.data {
        vfx_io::PixelData::F32(v) => v,
        // `read()` always emits an f32 RGBA buffer; anything else is a contract break.
        _ => {
            return Err(IoError::Exr(
                "EXR decode produced non-f32 pixel storage".to_string(),
            ));
        }
    };

    // Preserve the source precision: half EXRs round-trip f32→f16 losslessly
    // (every f16 value is exactly representable in f32), keeping the cache compact.
    if authored == vfx_io::PixelFormat::F16 {
        let buffer: Vec<f16> = rgba_f32.iter().map(|&v| f16::from_f32(v)).collect();
        trace!("Loaded EXR HALF: {}x{} (f16)", width, height);
        Ok(DecodedRaster {
            buffer: RawPixelBuffer::F16(buffer),
            format: RawPixelFormat::RgbaF16,
            width,
            height,
        })
    } else {
        trace!("Loaded EXR FLOAT: {}x{} (f32)", width, height);
        Ok(DecodedRaster {
            buffer: RawPixelBuffer::F32(rgba_f32),
            format: RawPixelFormat::RgbaF32,
            width,
            height,
        })
    }
}

fn header_generic(path: &Path) -> Result<Vec<(String, AttrKv)>, IoError> {
    trace!("Reading generic image header: {}", path.display());

    let reader = image::ImageReader::open(path)
        .map_err(|e| IoError::Image(format!("Failed to open image: {}", e)))?;

    let format = reader
        .format()
        .ok_or_else(|| IoError::Image("Failed to detect image format".to_string()))?;

    let img = reader
        .decode()
        .map_err(|e| IoError::Image(format!("Image decode error: {}", e)))?;

    let channels = match img.color() {
        image::ColorType::L8 | image::ColorType::L16 => 1,
        image::ColorType::La8 | image::ColorType::La16 => 2,
        image::ColorType::Rgb8 | image::ColorType::Rgb16 | image::ColorType::Rgb32F => 3,
        image::ColorType::Rgba8 | image::ColorType::Rgba16 | image::ColorType::Rgba32F => 4,
        _ => 4,
    };

    Ok(vec![
        ("width".into(), AttrKv::UInt(img.width())),
        ("height".into(), AttrKv::UInt(img.height())),
        ("format".into(), AttrKv::Str(format!("{:?}", format))),
        ("channels".into(), AttrKv::UInt(channels)),
    ])
}

/// Radiance HDR — decode to linear RGBA f32 via `image`.
fn decode_hdr(path: &Path) -> Result<DecodedRaster, IoError> {
    trace!("Loading Radiance HDR: {}", path.display());
    let img = image::open(path).map_err(|e| IoError::Image(format!("HDR open error: {}", e)))?;

    let width = img.width() as usize;
    let height = img.height() as usize;
    let rgb_f32 = img.to_rgb32f();
    let rgb_data = rgb_f32.as_raw();
    let mut buffer_f32 = Vec::with_capacity(width * height * 4);
    for chunk in rgb_data.chunks(3) {
        buffer_f32.push(chunk[0]);
        buffer_f32.push(chunk[1]);
        buffer_f32.push(chunk[2]);
        buffer_f32.push(1.0);
    }

    Ok(DecodedRaster {
        buffer: RawPixelBuffer::F32(buffer_f32),
        format: RawPixelFormat::RgbaF32,
        width,
        height,
    })
}

fn decode_generic(path: &Path) -> Result<DecodedRaster, IoError> {
    trace!("Loading generic image: {}", path.display());

    let img = image::open(path).map_err(|e| IoError::Image(format!("Image load error: {}", e)))?;

    let width = img.width() as usize;
    let height = img.height() as usize;
    let rgba_img = img.to_rgba8();
    let pixels = rgba_img.into_raw();

    Ok(DecodedRaster {
        buffer: RawPixelBuffer::U8(pixels),
        format: RawPixelFormat::Rgba8,
        width,
        height,
    })
}
