///! EXR file loading with pluggable backends
///!
///! **Default**: exrs (pure Rust, no external dependencies)
///! **Feature "openexr"**: openexr-rs (C++ bindings, full DWAA/DWAB support)
///!
///! # Architecture
///!
///! - `ExrLoader` trait: Common API for both implementations
///! - `Exr`: exrs-based implementation (default)
///! - `OpenExr`: openexr-rs-based implementation (feature gated)
///! - `ExrImpl`: Type alias that selects implementation at compile time

use std::path::Path;
use log::debug;
use crate::frame::{PixelBuffer, PixelFormat, FrameError};
use half::f16 as F16;

/// Common trait for EXR loading backends
pub trait ExrLoader {
    /// Load EXR file and return pixel data
    ///
    /// # Returns
    /// (buffer, pixel_format, width, height)
    fn load(path: &Path) -> Result<(PixelBuffer, PixelFormat, usize, usize), FrameError>;

    /// Read EXR header to get dimensions
    ///
    /// # Returns
    /// (width, height)
    fn header(path: &Path) -> Result<(usize, usize), FrameError>;
}

// ============================================================================
// EXRS IMPLEMENTATION (default, pure Rust)
// ============================================================================

#[cfg_attr(feature = "openexr", allow(dead_code))]
pub struct Exr;

impl ExrLoader for Exr {
    fn load(path: &Path) -> Result<(PixelBuffer, PixelFormat, usize, usize), FrameError> {
        debug!("Loading EXR with exrs: {}", path.display());

        // Open and decode EXR using image crate (which uses exrs internally)
        let img = image::open(path)
            .map_err(|e| {
                let err_str = e.to_string();
                // Check for unsupported compression
                if err_str.contains("DWAA") || err_str.contains("DWAB") {
                    return FrameError::UnsupportedFormat(
                        "DWAA/DWAB compression not supported in default build. \
                         Build with full support: cargo xtask build --openexr --release".into()
                    );
                }
                FrameError::Image(err_str)
            })?;

        let width = img.width() as usize;
        let height = img.height() as usize;

        // Detect color type and convert to appropriate format
        use image::DynamicImage;
        let (buffer, pixel_format) = match img {
            DynamicImage::ImageRgb8(_) | DynamicImage::ImageRgba8(_) => {
                // 8-bit LDR image
                let rgba8 = img.to_rgba8();
                (PixelBuffer::U8(rgba8.into_raw()), PixelFormat::Rgba8)
            }
            DynamicImage::ImageRgb16(_) | DynamicImage::ImageRgba16(_) => {
                // 16-bit image - convert to f16 for HDR workflow
                let rgb16 = img.to_rgba16();
                let rgb16_data = rgb16.as_raw();

                // Convert u16 to f16 (normalize 0-65535 to 0.0-1.0)
                let mut buffer_f16 = Vec::with_capacity(width * height * 4);
                for &val in rgb16_data {
                    buffer_f16.push(F16::from_f32(val as f32 / 65535.0));
                }

                (PixelBuffer::F16(buffer_f16), PixelFormat::RgbaF16)
            }
            DynamicImage::ImageRgb32F(_) | DynamicImage::ImageRgba32F(_) => {
                // 32-bit float HDR image
                let rgba32f = img.to_rgba32f();
                (PixelBuffer::F32(rgba32f.into_raw()), PixelFormat::RgbaF32)
            }
            _ => {
                // Fallback: convert to rgba8
                let rgba8 = img.to_rgba8();
                (PixelBuffer::U8(rgba8.into_raw()), PixelFormat::Rgba8)
            }
        };

        debug!("Loaded EXR with exrs: {}x{} ({:?})", width, height, pixel_format);
        Ok((buffer, pixel_format, width, height))
    }

    fn header(path: &Path) -> Result<(usize, usize), FrameError> {
        // Use image::ImageReader to get dimensions without loading pixels
        let reader = image::ImageReader::open(path)
            .map_err(|e| FrameError::Image(e.to_string()))?;

        let (width, height) = reader.into_dimensions()
            .map_err(|e| {
                let err_str = e.to_string();
                // Check for unsupported compression
                if err_str.contains("DWAA") || err_str.contains("DWAB") {
                    return FrameError::UnsupportedFormat(
                        "DWAA/DWAB compression not supported in default build. \
                         Build with full support: cargo xtask build --openexr --release".into()
                    );
                }
                FrameError::Image(err_str)
            })?;

        Ok((width as usize, height as usize))
    }
}

// ============================================================================
// OPENEXR IMPLEMENTATION (feature = "openexr", C++ bindings)
// ============================================================================

#[cfg(feature = "openexr")]
pub struct OpenExr;

#[cfg(feature = "openexr")]
impl ExrLoader for OpenExr {
    fn load(path: &Path) -> Result<(PixelBuffer, PixelFormat, usize, usize), FrameError> {
        use openexr::prelude::*;

        debug!("Loading EXR with openexr-rs: {}", path.display());

        // Open file to read header and detect pixel type
        let file = RgbaInputFile::new(path, 1)
            .map_err(|e| FrameError::Exr(e.to_string()))?;

        let header = file.header();
        let data_window = header.data_window::<[i32; 4]>();
        let width = (data_window[2] - data_window[0] + 1) as usize;
        let height = (data_window[3] - data_window[1] + 1) as usize;

        // Detect pixel type from channels (check R channel)
        let channels = header.channels();
        let pixel_type = channels
            .iter()
            .find(|(name, _)| *name == "R")
            .map(|(_, ch)| ch.type_)
            .unwrap_or(PixelType::Half.into());

        drop(header);
        drop(file);

        // Load based on detected pixel type
        if pixel_type == PixelType::Half.into() {
            Self::load_half(path, width, height)
        } else if pixel_type == PixelType::Float.into() {
            Self::load_float(path, width, height)
        } else {
            // UINT pixels - load as f16 for memory efficiency
            debug!("EXR UINT pixels detected, loading as f16");
            Self::load_half(path, width, height)
        }
    }

    fn header(path: &Path) -> Result<(usize, usize), FrameError> {
        use openexr::prelude::*;

        let file = RgbaInputFile::new(path, 1)
            .map_err(|e| FrameError::Exr(e.to_string()))?;

        let header = file.header();
        let data_window = header.data_window::<[i32; 4]>();
        let width = (data_window[2] - data_window[0] + 1) as usize;
        let height = (data_window[3] - data_window[1] + 1) as usize;

        Ok((width, height))
    }
}

#[cfg(feature = "openexr")]
impl OpenExr {
    /// Load EXR with HALF pixels (native f16)
    fn load_half(path: &Path, width: usize, height: usize) -> Result<(PixelBuffer, PixelFormat, usize, usize), FrameError> {
        use openexr::prelude::*;

        let mut file = RgbaInputFile::new(path, 1)
            .map_err(|e| FrameError::Exr(e.to_string()))?;

        let header = file.header();
        let data_window = header.data_window::<[i32; 4]>();
        let y_min = data_window[1];
        let y_max = data_window[3];
        drop(header);

        // Read as Rgba (which uses half::f16 internally)
        let mut pixels_rgba = vec![Rgba::from_f32(0.0, 0.0, 0.0, 0.0); width * height];
        file.set_frame_buffer(&mut pixels_rgba, 1, width)
            .map_err(|e| FrameError::Exr(e.to_string()))?;

        unsafe {
            file.read_pixels(y_min, y_max)
                .map_err(|e| FrameError::Exr(e.to_string()))?;
        }

        // Extract f16 values from Rgba into flat RGBA buffer
        let mut buffer_f16 = Vec::with_capacity(width * height * 4);
        for pixel in pixels_rgba.iter() {
            buffer_f16.push(pixel.r);  // half::f16
            buffer_f16.push(pixel.g);
            buffer_f16.push(pixel.b);
            buffer_f16.push(pixel.a);
        }

        debug!("Loaded EXR HALF with openexr-rs: {}x{} (f16)", width, height);
        Ok((PixelBuffer::F16(buffer_f16), PixelFormat::RgbaF16, width, height))
    }

    /// Load EXR with FLOAT pixels (native f32, true precision)
    fn load_float(path: &Path, width: usize, height: usize) -> Result<(PixelBuffer, PixelFormat, usize, usize), FrameError> {
        use openexr::prelude::*;

        // Use InputFile + Frame API for true f32 precision (no f16 conversion)
        let file = InputFile::new(path, 1)
            .map_err(|e| FrameError::Exr(e.to_string()))?;

        let header = file.header();
        let data_window = *header.data_window::<[i32; 4]>();
        let y_min = data_window[1];
        let y_max = data_window[3];
        drop(header);

        // Create Frame with f32 pixel type for RGBA channels
        let frame_rgba = Frame::new::<f32, _, _>(&["R", "G", "B", "A"], data_window)
            .map_err(|e| FrameError::Exr(e.to_string()))?;

        // Read pixels into frame (f32, no f16 conversion)
        let (_file, mut frames) = file
            .into_reader(vec![frame_rgba])
            .map_err(|e| FrameError::Exr(e.to_string()))?
            .read_pixels(y_min, y_max)
            .map_err(|e| FrameError::Exr(e.to_string()))?;

        // Extract flat RGBA f32 buffer
        let buffer_f32: Vec<f32> = frames.remove(0).into_vec();

        debug!("Loaded EXR FLOAT with openexr-rs: {}x{} (f32, native precision)", width, height);
        Ok((PixelBuffer::F32(buffer_f32), PixelFormat::RgbaF32, width, height))
    }
}

// ============================================================================
// TYPE ALIAS - Compile-time backend selection
// ============================================================================

/// ExrImpl: Type alias that selects backend at compile time
///
/// - Without feature "openexr": uses Exr (exrs, pure Rust)
/// - With feature "openexr": uses OpenExr (openexr-rs, C++ bindings)
#[cfg(not(feature = "openexr"))]
pub type ExrImpl = Exr;

#[cfg(feature = "openexr")]
pub type ExrImpl = OpenExr;
