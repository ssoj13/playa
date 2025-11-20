//! Image loader with pluggable backends
//!
//! Unified interface for loading image files with metadata extraction.
//! Supports different backends based on feature flags:
//! - Default: `image` crate (uses exrs for EXR)
//! - Feature "openexr": openexr-rs (C++ bindings, full DWAA/DWAB support)

use std::path::Path;
use log::debug;
use half::f16 as F16;

use crate::entities::{Attrs, AttrValue};
use super::frame::{Frame, FrameError, PixelBuffer, PixelFormat};

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
        let ext = path.extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();

        match ext.as_str() {
            "exr" => Self::header_exr(path),
            _ => Self::header_generic(path),
        }
    }

    /// Load complete image file into Frame
    pub fn load(path: &Path) -> Result<Frame, FrameError> {
        let ext = path.extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();

        match ext.as_str() {
            "exr" => Self::load_exr(path),
            _ => Self::load_generic(path),
        }
    }

    // ===== EXR Loading =====

    #[cfg(feature = "openexr")]
    fn header_exr(path: &Path) -> Result<Attrs, FrameError> {
        debug!("Reading EXR header with openexr: {}", path.display());

        use openexr::prelude::*;

        let file = InputFile::new(path, 1)
            .map_err(|e| FrameError::Image(format!("OpenEXR header error: {}", e)))?;

        let header = file.header();
        let data_window = header.data_window();
        let width = (data_window.max.x - data_window.min.x + 1) as usize;
        let height = (data_window.max.y - data_window.min.y + 1) as usize;

        let mut meta = Attrs::new();
        meta.set("width", AttrValue::UInt(width as u32));
        meta.set("height", AttrValue::UInt(height as u32));
        meta.set("format", AttrValue::Str("EXR (OpenEXR)".to_string()));

        // Extract channel count
        let channels = header.channels();
        meta.set("channels", AttrValue::UInt(channels.list.len() as u32));

        Ok(meta)
    }

    #[cfg(not(feature = "openexr"))]
    fn header_exr(path: &Path) -> Result<Attrs, FrameError> {
        debug!("Reading EXR header with image crate: {}", path.display());

        // Use image crate for header reading (it uses exrs internally)
        let reader = image::io::Reader::open(path)
            .map_err(|e| FrameError::Image(format!("Failed to open EXR: {}", e)))?;

        let format = reader.format()
            .ok_or_else(|| FrameError::Image("Failed to detect image format".to_string()))?;

        let img = reader.decode()
            .map_err(|e| {
                let err_str = e.to_string();
                if err_str.contains("DWAA") || err_str.contains("DWAB") {
                    FrameError::UnsupportedFormat(
                        "DWAA/DWAB compression not supported. Build with: cargo xtask build --openexr".to_string()
                    )
                } else {
                    FrameError::Image(format!("EXR decode error: {}", e))
                }
            })?;

        let mut meta = Attrs::new();
        meta.set("width", AttrValue::UInt(img.width()));
        meta.set("height", AttrValue::UInt(img.height()));
        meta.set("format", AttrValue::Str(format!("EXR ({:?})", format)));

        // Determine channel count from color type
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

    #[cfg(feature = "openexr")]
    fn load_exr(path: &Path) -> Result<Frame, FrameError> {
        debug!("Loading EXR with openexr: {}", path.display());

        use openexr::prelude::*;

        let file = InputFile::new(path, 1)
            .map_err(|e| FrameError::Image(format!("OpenEXR error: {}", e)))?;

        let header = file.header();
        let data_window = header.data_window();
        let width = (data_window.max.x - data_window.min.x + 1) as usize;
        let height = (data_window.max.y - data_window.min.y + 1) as usize;

        // Read RGBA channels
        let mut pixel_data = FrameBuffer::new(width, height);
        pixel_data.insert_channels(&["R", "G", "B", "A"]);

        file.read_pixels(&mut pixel_data)
            .map_err(|e| FrameError::Image(format!("OpenEXR read error: {}", e)))?;

        // Convert to F16 buffer
        let mut buffer = vec![F16::ZERO; width * height * 4];

        for y in 0..height {
            for x in 0..width {
                let idx = (y * width + x) * 4;

                if let Some(r) = pixel_data.get_pixel(x, y, "R") {
                    buffer[idx] = F16::from_f32(r);
                }
                if let Some(g) = pixel_data.get_pixel(x, y, "G") {
                    buffer[idx + 1] = F16::from_f32(g);
                }
                if let Some(b) = pixel_data.get_pixel(x, y, "B") {
                    buffer[idx + 2] = F16::from_f32(b);
                }
                if let Some(a) = pixel_data.get_pixel(x, y, "A") {
                    buffer[idx + 3] = F16::from_f32(a);
                } else {
                    buffer[idx + 3] = F16::ONE;
                }
            }
        }

        Ok(Frame::from_buffer(
            PixelBuffer::F16(buffer),
            PixelFormat::RgbaF16,
            width,
            height,
        ))
    }

    #[cfg(not(feature = "openexr"))]
    fn load_exr(path: &Path) -> Result<Frame, FrameError> {
        debug!("Loading EXR with image crate: {}", path.display());

        let img = image::open(path).map_err(|e| {
            let err_str = e.to_string();
            if err_str.contains("DWAA") || err_str.contains("DWAB") {
                return FrameError::UnsupportedFormat(
                    "DWAA/DWAB compression not supported. Build with: cargo xtask build --openexr".to_string()
                );
            }
            FrameError::Image(format!("EXR load error: {}", e))
        })?;

        let width = img.width() as usize;
        let height = img.height() as usize;

        // Convert to Rgba32F
        let rgba_img = img.to_rgba32f();
        let pixels = rgba_img.as_raw();

        // Convert f32 to f16
        let mut buffer = Vec::with_capacity(pixels.len());
        for &pixel in pixels {
            buffer.push(F16::from_f32(pixel));
        }

        Ok(Frame::from_buffer(
            PixelBuffer::F16(buffer),
            PixelFormat::RgbaF16,
            width,
            height,
        ))
    }

    // ===== Generic Image Loading (PNG, JPEG, TIFF, etc.) =====

    fn header_generic(path: &Path) -> Result<Attrs, FrameError> {
        debug!("Reading generic image header: {}", path.display());

        let reader = image::io::Reader::open(path)
            .map_err(|e| FrameError::Image(format!("Failed to open image: {}", e)))?;

        let format = reader.format()
            .ok_or_else(|| FrameError::Image("Failed to detect image format".to_string()))?;

        let img = reader.decode()
            .map_err(|e| FrameError::Image(format!("Image decode error: {}", e)))?;

        let mut meta = Attrs::new();
        meta.set("width", AttrValue::UInt(img.width()));
        meta.set("height", AttrValue::UInt(img.height()));
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
        debug!("Loading generic image: {}", path.display());

        let img = image::open(path)
            .map_err(|e| FrameError::Image(format!("Image load error: {}", e)))?;

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
