//! Unified header + decode dispatcher for supported media extensions.

use log::trace;
use std::path::Path;

use crate::error::IoError;
use crate::media;
use crate::pixel::{DecodedRaster, RawPixelBuffer, RawPixelFormat};
use crate::video;

/// Serialized header field for engine Attrs bridging.
#[derive(Debug, Clone)]
pub enum AttrKv {
    Str(String),
    UInt(u32),
    Float(f32),
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

#[cfg(feature = "exr")]
fn header_exr(path: &Path) -> Result<Vec<(String, AttrKv)>, IoError> {
    trace!("Reading EXR header with vfx-exr: {}", path.display());
    use std::io::Cursor;

    let bytes =
        std::fs::read(path).map_err(|e| IoError::Image(format!("EXR open error: {}", e)))?;
    let meta = vfx_exr::meta::MetaData::read_from_buffered(Cursor::new(&bytes), false)
        .map_err(|e| IoError::Exr(format!("EXR header error: {}", e)))?;

    let first = meta
        .headers
        .first()
        .ok_or_else(|| IoError::Exr("EXR has no layers".to_string()))?;

    let width = first.layer_size.x() as u32;
    let height = first.layer_size.y() as u32;
    let channel_count = first.channels.list.len() as u32;
    let layer_count = meta.headers.len() as u32;
    let compression_str = vfx_io::exr::compression_str::format(&first.compression);

    let channel_names: String = first
        .channels
        .list
        .iter()
        .map(|c| c.name.to_string())
        .collect::<Vec<_>>()
        .join(",");

    let layer_names: String = meta
        .headers
        .iter()
        .enumerate()
        .map(|(i, h)| {
            h.own_attributes
                .layer_name
                .as_ref()
                .map(|n| n.to_string())
                .unwrap_or_else(|| format!("Layer{}", i))
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
    trace!("Loading EXR with vfx-exr: {}", path.display());
    use half::f16;
    use vfx_exr::prelude::*;

    let file = InputFile::open(path).map_err(|e| IoError::Exr(format!("EXR open error: {}", e)))?;
    let header = file.header();
    let is_float = header
        .channels
        .list
        .iter()
        .any(|ch| ch.name == *"R" && ch.sample_type == SampleType::F32);
    drop(file);

    if is_float {
        let image = read_first_rgba_layer_from_file(
            path,
            |resolution, _channels| Vec::<f32>::with_capacity(resolution.x() * resolution.y() * 4),
            |buffer, _pos, (r, g, b, a): (f32, f32, f32, f32)| {
                buffer.push(r);
                buffer.push(g);
                buffer.push(b);
                buffer.push(a);
            },
        )
        .map_err(|e| IoError::Exr(format!("EXR decode error: {}", e)))?;

        let layer = &image.layer_data;
        let width = layer.size.x();
        let height = layer.size.y();
        let buffer = &layer.channel_data.pixels;

        trace!("Loaded EXR FLOAT: {}x{} (f32)", width, height);
        Ok(DecodedRaster {
            buffer: RawPixelBuffer::F32(buffer.clone()),
            format: RawPixelFormat::RgbaF32,
            width,
            height,
        })
    } else {
        let image = read_first_rgba_layer_from_file(
            path,
            |resolution, _channels| Vec::<f16>::with_capacity(resolution.x() * resolution.y() * 4),
            |buffer, _pos, (r, g, b, a): (f16, f16, f16, f16)| {
                buffer.push(r);
                buffer.push(g);
                buffer.push(b);
                buffer.push(a);
            },
        )
        .map_err(|e| IoError::Exr(format!("EXR decode error: {}", e)))?;

        let layer = &image.layer_data;
        let width = layer.size.x();
        let height = layer.size.y();
        let buffer = &layer.channel_data.pixels;

        trace!("Loaded EXR HALF: {}x{} (f16)", width, height);
        Ok(DecodedRaster {
            buffer: RawPixelBuffer::F16(buffer.clone()),
            format: RawPixelFormat::RgbaF16,
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
