//! Image loader with pluggable backends
//!
//! Unified interface for loading image files with metadata extraction.
//! - EXR: `vfx-exr` (pure Rust, all compressions including DWAA/DWAB/HTJ2K)
//! - Video: `playa-ffmpeg`
//! - Other (PNG, JPEG, TIFF, TGA, HDR): `image` crate

use log::trace;
use std::path::Path;

use super::frame::{Frame, FrameError, PixelBuffer, PixelFormat};
use crate::entities::loader_video;
use crate::entities::{AttrValue, Attrs};
use crate::utils::media;
use super::keys::{A_FPS, A_HEIGHT, A_WIDTH};

/// Classified file type used to dispatch to the correct loader/header backend
enum FileKind {
    Video,
    Exr,
    Generic,
}

/// Classify a lowercased extension string into a FileKind
fn classify_ext(ext: &str) -> FileKind {
    if media::VIDEO_EXTS.contains(&ext) {
        FileKind::Video
    } else if ext == "exr" {
        FileKind::Exr
    } else {
        FileKind::Generic
    }
}

/// Extract the lowercased extension from a path
fn path_ext(path: &Path) -> String {
    path.extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase()
}

/// Image loader with metadata support
pub struct Loader;

impl Loader {
    /// Read image file header and extract metadata
    ///
    /// Returns Attrs with metadata like:
    /// - "width", "height" (UInt) - image dimensions
    /// - "channels" (UInt) - number of channels
    /// - "format" (Str) - pixel format description
    /// - Additional format-specific metadata
    pub fn header(path: &Path) -> Result<Attrs, FrameError> {
        match classify_ext(&path_ext(path)) {
            FileKind::Video => Self::header_video(path),
            FileKind::Exr => Self::header_exr(path),
            FileKind::Generic => Self::header_generic(path),
        }
    }

    /// Load complete image file into Frame
    pub fn load(path: &Path) -> Result<Frame, FrameError> {
        match classify_ext(&path_ext(path)) {
            FileKind::Video => Self::load_video(path),
            FileKind::Exr => Self::load_exr(path),
            FileKind::Generic => Self::load_generic(path),
        }
    }

    // ===== Video Loading =====

    /// Read video metadata into Attrs (width, height, fps, frames)
    fn header_video(path: &Path) -> Result<Attrs, FrameError> {
        let (actual_path, _) = media::parse_video_path(path);
        let meta = loader_video::VideoMetadata::from_file(&actual_path)?;

        let mut meta_attrs = Attrs::new();
        meta_attrs.set(A_WIDTH, AttrValue::UInt(meta.width));
        meta_attrs.set(A_HEIGHT, AttrValue::UInt(meta.height));
        meta_attrs.set(
            "format",
            AttrValue::Str(format!("Video ({})", actual_path.display())),
        );
        meta_attrs.set("channels", AttrValue::UInt(3));
        meta_attrs.set("frames", AttrValue::UInt(meta.frame_count as u32));
        meta_attrs.set(A_FPS, AttrValue::Float(meta.fps as f32));

        Ok(meta_attrs)
    }

    /// Load a single video frame into Frame (defaults to frame 0 if not specified)
    fn load_video(path: &Path) -> Result<Frame, FrameError> {
        let (actual_path, frame_idx) = media::parse_video_path(path);
        let frame_num = frame_idx.unwrap_or(0);
        let (buffer, pixel_format, width, height) =
            loader_video::decode_frame(&actual_path, frame_num)?;

        Ok(Frame::from_buffer(buffer, pixel_format, width, height))
    }

    // ===== EXR Loading =====

    /// Read EXR header metadata via vfx-exr.
    ///
    /// Reports OIIO-aligned info: dimensions, channel count, channel names of
    /// the first layer, total layer count (multi-layer EXR), and compression
    /// of the first layer as an OIIO-style string (`"piz"`, `"dwaa:45"`,
    /// `"htj2k:32"`, …) via [`vfx_io::exr::compression_str`].
    fn header_exr(path: &Path) -> Result<Attrs, FrameError> {
        trace!("Reading EXR header with vfx-exr: {}", path.display());
        use std::io::Cursor;
        use vfx_exr::meta::MetaData;

        // MetaData::read_from_buffered parses ALL headers in one shot — one
        // syscall + one parse, no pixel decode. Multi-layer files yield
        // `headers: SmallVec<[Header; 1]>` where each entry is a full layer.
        let bytes = std::fs::read(path)
            .map_err(|e| FrameError::Image(format!("EXR open error: {}", e)))?;
        let meta = MetaData::read_from_buffered(Cursor::new(&bytes), false)
            .map_err(|e| FrameError::Image(format!("EXR header error: {}", e)))?;

        let first = meta
            .headers
            .first()
            .ok_or_else(|| FrameError::Image("EXR has no layers".to_string()))?;

        let width = first.layer_size.x() as u32;
        let height = first.layer_size.y() as u32;
        let channel_count = first.channels.list.len() as u32;
        let layer_count = meta.headers.len() as u32;

        // OIIO-style compression string via the vfx-io codec.
        let compression_str = vfx_io::exr::compression_str::format(&first.compression);

        // Channel names of the first layer (comma-joined for the UI line).
        let channel_names: String = first
            .channels
            .list
            .iter()
            .map(|c| c.name.to_string())
            .collect::<Vec<_>>()
            .join(",");

        // Layer names (only meaningful when there's more than one).
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

        let mut meta_attrs = Attrs::new();
        meta_attrs.set(A_WIDTH, AttrValue::UInt(width));
        meta_attrs.set(A_HEIGHT, AttrValue::UInt(height));
        meta_attrs.set(
            "format",
            AttrValue::Str(format!("EXR ({})", compression_str)),
        );
        meta_attrs.set("compression", AttrValue::Str(compression_str));
        meta_attrs.set("channels", AttrValue::UInt(channel_count));
        meta_attrs.set("channel_names", AttrValue::Str(channel_names));
        meta_attrs.set("layers", AttrValue::UInt(layer_count));
        if layer_count > 1 {
            meta_attrs.set("layer_names", AttrValue::Str(layer_names));
        }

        Ok(meta_attrs)
    }

    /// Load EXR via vfx-exr — reads RGBA as f16 (native for HALF, converted for FLOAT)
    fn load_exr(path: &Path) -> Result<Frame, FrameError> {
        trace!("Loading EXR with vfx-exr: {}", path.display());
        use half::f16;
        use vfx_exr::prelude::*;

        // Detect sample type from header to pick optimal pixel format
        let file = InputFile::open(path)
            .map_err(|e| FrameError::Image(format!("EXR open error: {}", e)))?;
        let header = file.header();
        let is_float = header.channels.list.iter()
            .any(|ch| ch.name == *"R" && ch.sample_type == SampleType::F32);
        drop(file);

        if is_float {
            // FLOAT channels: read as f32 to preserve full precision
            let image = read_first_rgba_layer_from_file(
                path,
                |resolution, _channels| {
                    Vec::<f32>::with_capacity(resolution.x() * resolution.y() * 4)
                },
                |buffer, _pos, (r, g, b, a): (f32, f32, f32, f32)| {
                    buffer.push(r);
                    buffer.push(g);
                    buffer.push(b);
                    buffer.push(a);
                },
            )
            .map_err(|e| FrameError::Image(format!("EXR decode error: {}", e)))?;

            let layer = &image.layer_data;
            let width = layer.size.x();
            let height = layer.size.y();
            let buffer = &layer.channel_data.pixels;

            trace!("Loaded EXR FLOAT: {}x{} (f32)", width, height);
            Ok(Frame::from_buffer(
                PixelBuffer::F32(buffer.clone()),
                PixelFormat::RgbaF32,
                width,
                height,
            ))
        } else {
            // HALF / UINT: read as f16 (memory-efficient)
            let image = read_first_rgba_layer_from_file(
                path,
                |resolution, _channels| {
                    Vec::<f16>::with_capacity(resolution.x() * resolution.y() * 4)
                },
                |buffer, _pos, (r, g, b, a): (f16, f16, f16, f16)| {
                    buffer.push(r);
                    buffer.push(g);
                    buffer.push(b);
                    buffer.push(a);
                },
            )
            .map_err(|e| FrameError::Image(format!("EXR decode error: {}", e)))?;

            let layer = &image.layer_data;
            let width = layer.size.x();
            let height = layer.size.y();
            let buffer = &layer.channel_data.pixels;

            trace!("Loaded EXR HALF: {}x{} (f16)", width, height);
            Ok(Frame::from_buffer(
                PixelBuffer::F16(buffer.clone()),
                PixelFormat::RgbaF16,
                width,
                height,
            ))
        }
    }

    // ===== Generic Image Loading (PNG, JPEG, TIFF, etc.) =====

    fn header_generic(path: &Path) -> Result<Attrs, FrameError> {
        trace!("Reading generic image header: {}", path.display());

        let reader = image::ImageReader::open(path)
            .map_err(|e| FrameError::Image(format!("Failed to open image: {}", e)))?;

        let format = reader
            .format()
            .ok_or_else(|| FrameError::Image("Failed to detect image format".to_string()))?;

        let img = reader
            .decode()
            .map_err(|e| FrameError::Image(format!("Image decode error: {}", e)))?;

        let mut meta = Attrs::new();
        meta.set(A_WIDTH, AttrValue::UInt(img.width()));
        meta.set(A_HEIGHT, AttrValue::UInt(img.height()));
        meta.set("format", AttrValue::Str(format!("{:?}", format)));

        let channels = match img.color() {
            image::ColorType::L8 | image::ColorType::L16 => 1,
            image::ColorType::La8 | image::ColorType::La16 => 2,
            image::ColorType::Rgb8 | image::ColorType::Rgb16 | image::ColorType::Rgb32F => 3,
            image::ColorType::Rgba8 | image::ColorType::Rgba16 | image::ColorType::Rgba32F => 4,
            _ => 4,
        };
        meta.set("channels", AttrValue::UInt(channels));

        Ok(meta)
    }

    fn load_generic(path: &Path) -> Result<Frame, FrameError> {
        trace!("Loading generic image: {}", path.display());

        let img =
            image::open(path).map_err(|e| FrameError::Image(format!("Image load error: {}", e)))?;

        let width = img.width() as usize;
        let height = img.height() as usize;

        // Convert to Rgba8
        let rgba_img = img.to_rgba8();
        let pixels = rgba_img.into_raw();

        Ok(Frame::from_buffer(
            PixelBuffer::U8(pixels),
            PixelFormat::Rgba8,
            width,
            height,
        ))
    }
}

