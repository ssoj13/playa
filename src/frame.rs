//! Frame loading with multi-format pixel buffers (U8, F16, F32)
//!
//! **Why**: Different formats require different pixel representations:
//! - JPG/PNG: 8-bit RGBA (u8)
//! - EXR HALF: 16-bit float (half::f16)
//! - EXR FLOAT: 32-bit float (f32, native precision)
//!
//! **Used by**: Cache workers (parallel loading), Viewport (pixel data for GPU upload)
//!
//! # Pixel Formats
//!
//! - `PixelBuffer::U8`: LDR images (JPG/PNG), 4 bytes/pixel
//! - `PixelBuffer::F16`: EXR HALF, 8 bytes/pixel, range -65504..65504
//! - `PixelBuffer::F32`: EXR FLOAT, 16 bytes/pixel, full f32 precision
//!
//! # Atomic Loading
//!
//! `try_claim_for_loading()`: Atomic Header → Loading transition.
//! Prevents multiple workers from loading same frame (TOCTOU race).
//!
//! # EXR Precision
//!
//! Uses `InputFile + Frame<f32>` API for native f32 reading (no f16 intermediate).
//! Critical for ACES/linear workflows where precision matters.

use log::{debug, info};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

// Import f16 from half crate
use half::f16 as F16;

// Import EXR loader (exrs or openexr-rs based on features)
use crate::exr::{ExrImpl, ExrLoader};

/// Pixel buffer format - stores different precision levels
#[derive(Debug, Clone)]
pub enum PixelBuffer {
    U8(Vec<u8>),              // LDR formats (PNG, JPEG, TGA) - 8-bit per channel
    F16(Vec<F16>),            // HDR half-precision (EXR HALF) - 16-bit float per channel
    F32(Vec<f32>),            // HDR full-precision (EXR FLOAT, HDR) - 32-bit float per channel
}

/// Pixel format type
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PixelFormat {
    Rgba8,     // 8-bit RGBA (LDR)
    RgbaF16,   // 16-bit half-float RGBA (HDR)
    RgbaF32,   // 32-bit float RGBA (HDR)
}

/// Frame loading status
#[derive(Debug, Clone, PartialEq)]
pub enum FrameStatus {
    Placeholder, // No filename, green placeholder
    Header,      // Filename set, header loaded (resolution known), buffer is green placeholder
    Loading,     // Async loading in progress
    Loaded,      // Image data loaded into buffer
    Error,       // Loading failed
}

/// Internal frame data protected by mutex
#[derive(Debug, Clone)]
struct FrameData {
    buffer: PixelBuffer,   // Multi-format pixel buffer
    pixel_format: PixelFormat,
    width: usize,
    height: usize,
    status: FrameStatus,
}

/// Single frame with optional file source
#[derive(Debug, Clone)]
pub struct Frame {
    data: Arc<Mutex<FrameData>>, // All mutable data in one mutex
    filename: Option<PathBuf>,    // Immutable after creation
}

/// Frame loading errors
#[derive(Debug)]
pub enum FrameError {
    #[cfg_attr(not(feature = "openexr"), allow(dead_code))]
    Exr(String),
    Image(String),
    UnsupportedFormat(String),
    NoFilename,
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameError::Exr(e) => write!(f, "EXR error: {}", e),
            FrameError::Image(e) => write!(f, "Image error: {}", e),
            FrameError::UnsupportedFormat(e) => write!(f, "Unsupported format: {}", e),
            FrameError::NoFilename => write!(f, "No filename set"),
        }
    }
}

impl std::error::Error for FrameError {}

impl Frame {
    /// Create new frame with green placeholder
    pub fn new(width: usize, height: usize) -> Self {
        // Efficiently create repeated RGBA pattern [0,100,0,255]
        let mut buffer_u8 = vec![0u8; width * height * 4];
        for px in buffer_u8.chunks_mut(4) {
            px.copy_from_slice(&[0, 100, 0, 255]); // Dark green RGBA
        }

        let data = FrameData {
            buffer: PixelBuffer::U8(buffer_u8),
            pixel_format: PixelFormat::Rgba8,
            width,
            height,
            status: FrameStatus::Placeholder,
        };

        Self {
            data: Arc::new(Mutex::new(data)),
            filename: None,
        }
    }

    /// Create unloaded frame placeholder with path (for deserialization/caching)
    pub fn new_unloaded(path: PathBuf) -> Self {
        // Create minimal 1x1 green placeholder
        let buffer_u8 = vec![0, 100, 0, 255]; // 1 pixel dark green

        let data = FrameData {
            buffer: PixelBuffer::U8(buffer_u8),
            pixel_format: PixelFormat::Rgba8,
            width: 1,
            height: 1,
            status: FrameStatus::Header, // Path set but not loaded
        };

        Self {
            data: Arc::new(Mutex::new(data)),
            filename: Some(path),
        }
    }

    /// Set filename but don't load yet (sets status to Header)
    pub fn set_file(&mut self, path: PathBuf) {
        self.filename = Some(path);
        self.data.lock().unwrap().status = FrameStatus::Header;
    }

    /// Get filename if set
    pub fn file(&self) -> Option<&PathBuf> {
        self.filename.as_ref()
    }

    /// Atomically claim frame for loading (Header → Loading)
    ///
    /// **Why**: Prevents TOCTOU race - multiple workers checking "is loaded?" then all loading
    ///
    /// **Used by**: Worker threads before starting decode (`load()`)
    ///
    /// # Returns
    ///
    /// - `true`: Successfully claimed, caller MUST load the frame
    /// - `false`: Already claimed/loaded/error, caller MUST skip loading
    ///
    /// # Atomicity
    ///
    /// Check-and-set is atomic under single Mutex lock.
    /// Only one thread can transition Header → Loading.
    fn try_claim_for_loading(&self) -> bool {
        let mut data = self.data.lock().unwrap();
        if data.status == FrameStatus::Header {
            data.status = FrameStatus::Loading;
            true
        } else {
            false  // Already loading, loaded, or error
        }
    }

    /// Load frame from disk (JPG/PNG/EXR) into pixel buffer
    ///
    /// **Why**: Decode image into GPU-ready RGBA buffer for display
    ///
    /// **Used by**: Cache workers (background threads)
    ///
    /// # Pixel Format Selection
    ///
    /// - JPG/PNG: Always `PixelBuffer::U8` (8-bit RGBA)
    /// - EXR HALF: `PixelBuffer::F16` (half::f16 RGBA)
    /// - EXR FLOAT: `PixelBuffer::F32` (native f32 RGBA, no precision loss)
    ///
    /// # Errors
    ///
    /// - `FrameError::NoFilename`: Frame has no associated file path
    /// - `FrameError::Io`: File not found or read error
    /// - `FrameError::UnsupportedFormat`: Unknown file extension
    /// - `FrameError::Exr`: OpenEXR decode failed
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use playa::frame::Frame;
    /// # use std::path::Path;
    /// let frame = Frame::new_with_file(Path::new("render.0001.exr"), 1920, 1080);
    /// match frame.load() {
    ///     Ok(()) => println!("Loaded: {}x{}", frame.width(), frame.height()),
    ///     Err(e) => eprintln!("Load failed: {:?}", e),
    /// }
    /// ```
    pub fn load(&self) -> Result<(), FrameError> {
        let path = self.filename.as_ref().ok_or(FrameError::NoFilename)?.clone();

        // Atomically claim frame for loading (prevents duplicate loads)
        if !self.try_claim_for_loading() {
            // Already loading/loaded/error - just return current status
            return match self.status() {
                FrameStatus::Loaded => Ok(()),
                // Return a clearer error category instead of UnsupportedFormat
                FrameStatus::Error => Err(FrameError::Image("Previously failed".into())),
                _ => Ok(()),  // Loading in progress
            };
        }

        // Detect format by extension
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_lowercase())
            .unwrap_or_default();

        let result = match ext.as_str() {
            "exr" => self.load_exr(&path),
            "hdr" => self.load_hdr(&path),
            "png" | "jpg" | "jpeg" | "tif" | "tiff" | "tga" => self.load_image(&path),
            _ => Err(FrameError::UnsupportedFormat(format!(".{}", ext))),
        };

        match result {
            Ok(()) => {
                self.data.lock().unwrap().status = FrameStatus::Loaded;
                Ok(())
            }
            Err(e) => {
                self.data.lock().unwrap().status = FrameStatus::Error;
                Err(e)
            }
        }
    }

    /// Load EXR file - delegate to ExrImpl (exrs or openexr-rs)
    fn load_exr<P: AsRef<Path>>(&self, path: P) -> Result<(), FrameError> {
        debug!("Loading EXR: {}", path.as_ref().display());

        // Delegate to ExrImpl (compile-time selected backend)
        let (buffer, pixel_format, width, height) = ExrImpl::load(path.as_ref())?;

        // Update frame data
        let mut data = self.data.lock().unwrap();
        data.buffer = buffer;
        data.pixel_format = pixel_format;
        data.width = width;
        data.height = height;

        debug!("Loaded EXR: {}x{} ({:?})", width, height, pixel_format);
        Ok(())
    }

    /// Load Radiance HDR format
    fn load_hdr<P: AsRef<Path>>(&self, path: P) -> Result<(), FrameError> {
        debug!("Loading HDR: {}", path.as_ref().display());

        let img = image::open(path.as_ref())
            .map_err(|e| FrameError::Image(e.to_string()))?;

        let width = img.width() as usize;
        let height = img.height() as usize;

        // Convert to RGB f32 (HDR decoder outputs f32)
        let rgb_f32 = img.to_rgb32f();
        let rgb_data = rgb_f32.as_raw();

        // Convert RGB f32 to RGBA f32 (add alpha channel = 1.0)
        let mut buffer_f32 = Vec::with_capacity(width * height * 4);
        for chunk in rgb_data.chunks(3) {
            buffer_f32.push(chunk[0]); // R
            buffer_f32.push(chunk[1]); // G
            buffer_f32.push(chunk[2]); // B
            buffer_f32.push(1.0);      // A (opaque)
        }

        // Update frame data atomically - store as native f32 HDR
        let mut data = self.data.lock().unwrap();
        data.buffer = PixelBuffer::F32(buffer_f32);
        data.pixel_format = PixelFormat::RgbaF32;
        data.width = width;
        data.height = height;

        info!("Loaded HDR: {}x{} (HDR f32)", width, height);
        Ok(())
    }

    /// Load standard image formats
    fn load_image<P: AsRef<Path>>(&self, path: P) -> Result<(), FrameError> {
        debug!("Loading image: {}", path.as_ref().display());

        let img = image::open(path.as_ref())
            .map_err(|e| FrameError::Image(e.to_string()))?;

        let width = img.width() as usize;
        let height = img.height() as usize;
        let rgba = img.to_rgba8();

        // Update frame data atomically
        let mut data = self.data.lock().unwrap();
        data.buffer = PixelBuffer::U8(rgba.into_raw());
        data.pixel_format = PixelFormat::Rgba8;
        data.width = width;
        data.height = height;

        Ok(())
    }

    /// Memory size in bytes
    pub fn mem(&self) -> usize {
        let data = self.data.lock().unwrap();
        match &data.buffer {
            PixelBuffer::U8(vec) => vec.len(),       // 1 byte per u8
            PixelBuffer::F16(vec) => vec.len() * 2,  // 2 bytes per f16
            PixelBuffer::F32(vec) => vec.len() * 4,  // 4 bytes per f32
        }
    }

    /// Get status
    pub fn status(&self) -> FrameStatus {
        self.data.lock().unwrap().status.clone()
    }

    /// Set status
    pub fn set_status(&self, status: FrameStatus) {
        self.data.lock().unwrap().status = status;
    }

    /// Get pixel buffer (returns cloned buffer)
    pub fn pixel_buffer(&self) -> PixelBuffer {
        self.data.lock().unwrap().buffer.clone()
    }

    /// Get pixel format
    pub fn pixel_format(&self) -> PixelFormat {
        self.data.lock().unwrap().pixel_format
    }

    /// Get pixels as u8 slice (for backward compatibility, only works with Rgba8 format)
    /// Returns error if the pixel format is not U8 (e.g., HDR formats F16/F32)
    pub fn pixels(&self) -> Result<Vec<u8>, FrameError> {
        let data = self.data.lock().unwrap();
        match &data.buffer {
            PixelBuffer::U8(vec) => Ok(vec.clone()),
            PixelBuffer::F16(_) => Err(FrameError::UnsupportedFormat(
                "Frame uses F16 format, use pixel_buffer() for HDR data".into()
            )),
            PixelBuffer::F32(_) => Err(FrameError::UnsupportedFormat(
                "Frame uses F32 format, use pixel_buffer() for HDR data".into()
            )),
        }
    }

    /// Get buffer (deprecated - use pixel_buffer() instead)
    #[allow(dead_code)]
    pub fn buffer(&self) -> Arc<Mutex<Vec<u8>>> {
        // This method is deprecated and will panic if called on non-U8 format
        Arc::new(Mutex::new(self.pixels().unwrap()))
    }

    /// Get dimensions
    pub fn width(&self) -> usize {
        self.data.lock().unwrap().width
    }

    pub fn height(&self) -> usize {
        self.data.lock().unwrap().height
    }

    /// Get resolution as tuple
    pub fn resolution(&self) -> (usize, usize) {
        let data = self.data.lock().unwrap();
        (data.width, data.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// Test: Frame creation with placeholder
    /// Validates: Initial state is correct
    #[test]
    fn test_frame_creation() {
        let frame = Frame::new(1920, 1080);

        assert_eq!(frame.width(), 1920);
        assert_eq!(frame.height(), 1080);
        assert_eq!(frame.status(), FrameStatus::Placeholder);
        assert_eq!(frame.pixel_format(), PixelFormat::Rgba8);
    }

    /// Test: Frame creation with file path
    /// Validates: Status transitions to Header
    #[test]
    fn test_frame_with_file() {
        let frame = Frame::new_unloaded(PathBuf::from("test.exr"));

        assert_eq!(frame.status(), FrameStatus::Header);
        assert!(frame.file().is_some());
        assert_eq!(frame.file().unwrap(), Path::new("test.exr"));
    }

    /// Test: Load missing file returns error
    /// Validates: Error handling for non-existent files
    #[test]
    fn test_load_missing_file() {
        let frame = Frame::new_unloaded(
            PathBuf::from("/nonexistent/path/test.jpg")
        );

        let result = frame.load();
        assert!(result.is_err());

        // After failed load, status should be Error
        assert_eq!(frame.status(), FrameStatus::Error);
    }

    /// Test: PixelBuffer variant sizes
    /// Validates: Different pixel formats have expected memory layout
    #[test]
    fn test_pixel_buffer_types() {
        // U8: 4 bytes per pixel (RGBA)
        let buf_u8 = PixelBuffer::U8(vec![0u8; 1920 * 1080 * 4]);
        match buf_u8 {
            PixelBuffer::U8(v) => assert_eq!(v.len(), 1920 * 1080 * 4),
            _ => panic!("Wrong variant"),
        }

        // F16: 4 half-floats per pixel (RGBA)
        let buf_f16 = PixelBuffer::F16(vec![F16::ZERO; 1920 * 1080 * 4]);
        match buf_f16 {
            PixelBuffer::F16(v) => assert_eq!(v.len(), 1920 * 1080 * 4),
            _ => panic!("Wrong variant"),
        }

        // F32: 4 floats per pixel (RGBA)
        let buf_f32 = PixelBuffer::F32(vec![0.0f32; 1920 * 1080 * 4]);
        match buf_f32 {
            PixelBuffer::F32(v) => assert_eq!(v.len(), 1920 * 1080 * 4),
            _ => panic!("Wrong variant"),
        }
    }

    /// Test: Frame status transitions
    /// Validates: Status lifecycle is correct
    #[test]
    fn test_status_transitions() {
        let frame = Frame::new(100, 100);
        assert_eq!(frame.status(), FrameStatus::Placeholder);

        // Set filename → Header
        let frame = Frame::new_unloaded(PathBuf::from("test.png"));
        assert_eq!(frame.status(), FrameStatus::Header);

        // Load will transition to Loading → Error (file doesn't exist)
        let _ = frame.load();
        assert_eq!(frame.status(), FrameStatus::Error);
    }
}

