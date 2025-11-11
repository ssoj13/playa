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
use crate::utils::media;

/// Parse video path with frame suffix
/// "video.mp4@17" -> (PathBuf("video.mp4"), Some(17))
/// "video.mp4" -> (PathBuf("video.mp4"), None)
fn parse_video_path(path: &Path) -> (PathBuf, Option<usize>) {
    let path_str = path.to_string_lossy();

    if let Some(at_pos) = path_str.rfind('@') {
        let base = &path_str[..at_pos];
        let frame_num = &path_str[at_pos + 1..];

        if let Ok(num) = frame_num.parse::<usize>() {
            return (PathBuf::from(base), Some(num));
        }
    }

    (path.to_path_buf(), None)
}

/// Pixel buffer format - stores different precision levels
#[derive(Debug, Clone)]
pub enum PixelBuffer {
    U8(Vec<u8>),   // LDR formats (PNG, JPEG, TGA) - 8-bit per channel
    F16(Vec<F16>), // HDR half-precision (EXR HALF) - 16-bit float per channel
    F32(Vec<f32>), // HDR full-precision (EXR FLOAT, HDR) - 32-bit float per channel
}

/// Pixel format type
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PixelFormat {
    Rgba8,   // 8-bit RGBA (LDR)
    RgbaF16, // 16-bit half-float RGBA (HDR)
    RgbaF32, // 32-bit float RGBA (HDR)
}

/// Crop alignment mode
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub enum CropAlign {
    Center,  // Center-align when cropping or padding
    LeftTop, // Align to top-left corner (reserved for future use)
}

/// Pixel bit depth for Frame construction
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PixelDepth {
    U8,  // 8-bit RGBA (LDR)
    #[allow(dead_code)] // Created automatically by EXR loader (exr.rs), not via Frame::new()
    F16, // 16-bit half-float RGBA (HDR)
    #[allow(dead_code)] // Created automatically by EXR loader (exr.rs), not via Frame::new()
    F32, // 32-bit float RGBA (HDR)
}

/// Tonemapping mode for HDR→LDR conversion
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum TonemapMode {
    Clamp,    // Simple clamp to [0,1] range
    ACES,     // ACES filmic tone mapping curve
    Reinhard, // Reinhard tone mapping (photographic)
}

impl Default for TonemapMode {
    fn default() -> Self {
        TonemapMode::ACES // ACES provides best filmic results for VFX
    }
}

/// Frame loading status
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FrameStatus {
    Placeholder, // No filename, green placeholder
    Header,      // Filename set, header loaded (resolution known), buffer is green placeholder
    Loading,     // Async loading in progress
    Loaded,      // Image data loaded into buffer
    Error,       // Loading failed
}

impl FrameStatus {
    /// Get UI color for this status (for load indicator)
    pub fn color(&self) -> eframe::egui::Color32 {
        use eframe::egui::Color32;
        match self {
            FrameStatus::Placeholder => Color32::from_rgb(40, 40, 45),  // Dark grey
            FrameStatus::Header => Color32::from_rgb(60, 100, 180),      // Blue
            FrameStatus::Loading => Color32::from_rgb(220, 160, 60),     // Orange
            FrameStatus::Loaded => Color32::from_rgb(80, 200, 120),      // Green
            FrameStatus::Error => Color32::from_rgb(200, 60, 60),        // Red
        }
    }
}

/// Internal frame data protected by mutex
#[derive(Debug, Clone)]
struct FrameData {
    buffer: Arc<PixelBuffer>, // Multi-format pixel buffer (Arc for cheap cloning)
    pixel_format: PixelFormat,
    width: usize,
    height: usize,
    status: FrameStatus,
}

/// Single frame with optional file source
#[derive(Debug, Clone)]
pub struct Frame {
    data: Arc<Mutex<FrameData>>, // All mutable data in one mutex
    filename: Option<PathBuf>,   // Immutable after creation
}

/// Frame loading errors
#[derive(Debug)]
pub enum FrameError {
    #[cfg_attr(not(feature = "openexr"), allow(dead_code))]
    Exr(String),
    Image(String),
    LoadError(String),
    UnsupportedFormat(String),
    NoFilename,
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameError::Exr(e) => write!(f, "EXR error: {}", e),
            FrameError::Image(e) => write!(f, "Image error: {}", e),
            FrameError::LoadError(e) => write!(f, "Load error: {}", e),
            FrameError::UnsupportedFormat(e) => write!(f, "Unsupported format: {}", e),
            FrameError::NoFilename => write!(f, "No filename set"),
        }
    }
}

impl std::error::Error for FrameError {}

impl Frame {
    /// Create new frame with green placeholder
    pub fn new(width: usize, height: usize, depth: PixelDepth) -> Self {
        match depth {
            PixelDepth::U8 => {
                // Efficiently create repeated RGBA pattern [0,100,0,255]
                let mut buffer_u8 = Vec::with_capacity(width * height * 4);
                buffer_u8.resize(width * height * 4, 0);
                for px in buffer_u8.chunks_exact_mut(4) {
                    px[1] = 100; // G channel
                    px[3] = 255; // A channel (R,B already 0)
                }

                let data = FrameData {
                    buffer: Arc::new(PixelBuffer::U8(buffer_u8)),
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
            PixelDepth::F16 => {
                // Create F16 green placeholder
                let mut buffer_f16 = vec![F16::ZERO; width * height * 4];
                let green = F16::from_f32(100.0 / 255.0);
                let one = F16::ONE;

                for px in buffer_f16.chunks_exact_mut(4) {
                    px[1] = green; // G channel
                    px[3] = one; // A channel
                }

                let data = FrameData {
                    buffer: Arc::new(PixelBuffer::F16(buffer_f16)),
                    pixel_format: PixelFormat::RgbaF16,
                    width,
                    height,
                    status: FrameStatus::Placeholder,
                };

                Self {
                    data: Arc::new(Mutex::new(data)),
                    filename: None,
                }
            }
            PixelDepth::F32 => {
                // Create F32 green placeholder
                let mut buffer_f32 = vec![0.0f32; width * height * 4];

                for px in buffer_f32.chunks_exact_mut(4) {
                    px[1] = 100.0 / 255.0; // G channel
                    px[3] = 1.0; // A channel
                }

                let data = FrameData {
                    buffer: Arc::new(PixelBuffer::F32(buffer_f32)),
                    pixel_format: PixelFormat::RgbaF32,
                    width,
                    height,
                    status: FrameStatus::Placeholder,
                };

                Self {
                    data: Arc::new(Mutex::new(data)),
                    filename: None,
                }
            }
        }
    }

    /// Convenience method: Create 8-bit U8 frame
    pub fn new_u8(width: usize, height: usize) -> Self {
        Self::new(width, height, PixelDepth::U8)
    }

    /// Convenience method: Create 16-bit half-float F16 frame
    /// Note: Rarely used - EXR loader creates F16 frames directly via PixelBuffer
    #[allow(dead_code)]
    pub fn new_f16(width: usize, height: usize) -> Self {
        Self::new(width, height, PixelDepth::F16)
    }

    /// Convenience method: Create 32-bit float F32 frame
    /// Note: Rarely used - EXR loader creates F32 frames directly via PixelBuffer
    #[allow(dead_code)]
    pub fn new_f32(width: usize, height: usize) -> Self {
        Self::new(width, height, PixelDepth::F32)
    }

    /// Create unloaded frame placeholder with path (for deserialization/caching)
    pub fn new_unloaded(path: PathBuf) -> Self {
        // Create minimal 1x1 green placeholder
        let buffer_u8 = vec![0, 100, 0, 255]; // 1 pixel dark green

        let data = FrameData {
            buffer: Arc::new(PixelBuffer::U8(buffer_u8)),
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
        let _ = self.set_status(FrameStatus::Header);
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
            false // Already loading, loaded, or error
        }
    }

    /// Load only header (width/height) from any image format
    ///
    /// **Why**: Fast metadata loading without decoding pixels
    ///
    /// **Used by**: Initial frame setup, play_range cache management
    ///
    /// Supports: EXR, PNG, JPG, TIFF, HDR, TGA
    pub fn load_header(&self) -> Result<(), FrameError> {
        let path = self
            .filename
            .as_ref()
            .ok_or(FrameError::NoFilename)?
            .clone();

        // Parse video path: "video.mp4@17" -> ("video.mp4", Some(17))
        let (actual_path, _frame_num) = parse_video_path(&path);

        // For video files, get dimensions from video metadata
        if media::is_video(&actual_path) {
            let (width, height) = crate::video::get_video_dimensions(&actual_path)?;

            let mut data = self.data.lock().unwrap();
            data.width = width;
            data.height = height;
            data.status = FrameStatus::Header;

            debug!("Loaded video header: {}x{}", width, height);
            return Ok(());
        }

        // For images, use ExrImpl::header (works for all formats via image crate)
        let ext = actual_path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_lowercase())
            .unwrap_or_default();

        let (width, height) = match ext.as_str() {
            "exr" => ExrImpl::header(&actual_path)?,
            "png" | "jpg" | "jpeg" | "tif" | "tiff" | "tga" | "hdr" => {
                // Use ImageReader for fast header reading without decoding pixels
                let reader = image::ImageReader::open(&actual_path)
                    .map_err(|e| FrameError::Image(e.to_string()))?;
                let (w, h) = reader
                    .into_dimensions()
                    .map_err(|e| FrameError::Image(e.to_string()))?;
                (w as usize, h as usize)
            }
            _ => return Err(FrameError::UnsupportedFormat(format!(".{}", ext))),
        };

        let mut data = self.data.lock().unwrap();
        data.width = width;
        data.height = height;
        data.status = FrameStatus::Header;

        debug!("Loaded header: {}x{}", width, height);
        Ok(())
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
        let path = self
            .filename
            .as_ref()
            .ok_or(FrameError::NoFilename)?
            .clone();

        // Parse video path: "video.mp4@17" -> ("video.mp4", Some(17))
        let (actual_path, frame_num) = parse_video_path(&path);

        // Atomically claim frame for loading (prevents duplicate loads)
        if !self.try_claim_for_loading() {
            // Already loading/loaded/error - just return current status
            return match self.status() {
                FrameStatus::Loaded => Ok(()),
                // Return a clearer error category instead of UnsupportedFormat
                FrameStatus::Error => Err(FrameError::Image("Previously failed".into())),
                _ => Ok(()), // Loading in progress
            };
        }

        // Detect format by extension
        let ext = actual_path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_lowercase())
            .unwrap_or_default();

        let result = if media::is_video(&actual_path) {
            self.load_video(&actual_path, frame_num.unwrap_or(0))
        } else {
            match ext.as_str() {
                "exr" => self.load_exr(&actual_path),
                "hdr" => self.load_hdr(&actual_path),
                "png" | "jpg" | "jpeg" | "tif" | "tiff" | "tga" => self.load_image(&actual_path),
                _ => Err(FrameError::UnsupportedFormat(format!(".{}", ext))),
            }
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
        data.buffer = Arc::new(buffer);
        data.pixel_format = pixel_format;
        data.width = width;
        data.height = height;

        debug!("Loaded EXR: {}x{} ({:?})", width, height, pixel_format);
        Ok(())
    }

    /// Load Radiance HDR format
    fn load_hdr<P: AsRef<Path>>(&self, path: P) -> Result<(), FrameError> {
        debug!("Loading HDR: {}", path.as_ref().display());

        let img = image::open(path.as_ref()).map_err(|e| FrameError::Image(e.to_string()))?;

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
            buffer_f32.push(1.0); // A (opaque)
        }

        // Update frame data atomically - store as native f32 HDR
        let mut data = self.data.lock().unwrap();
        data.buffer = Arc::new(PixelBuffer::F32(buffer_f32));
        data.pixel_format = PixelFormat::RgbaF32;
        data.width = width;
        data.height = height;

        info!("Loaded HDR: {}x{} (HDR f32)", width, height);
        Ok(())
    }

    /// Load standard image formats
    fn load_image<P: AsRef<Path>>(&self, path: P) -> Result<(), FrameError> {
        debug!("Loading image: {}", path.as_ref().display());

        let img = image::open(path.as_ref()).map_err(|e| FrameError::Image(e.to_string()))?;

        let width = img.width() as usize;
        let height = img.height() as usize;
        let rgba = img.to_rgba8();

        // Update frame data atomically
        let mut data = self.data.lock().unwrap();
        data.buffer = Arc::new(PixelBuffer::U8(rgba.into_raw()));
        data.pixel_format = PixelFormat::Rgba8;
        data.width = width;
        data.height = height;

        Ok(())
    }

    /// Load video frame
    fn load_video<P: AsRef<Path>>(&self, path: P, frame_num: usize) -> Result<(), FrameError> {
        debug!(
            "Loading video frame {}: {}",
            frame_num,
            path.as_ref().display()
        );

        let (buffer, pixel_format, width, height) =
            crate::video::decode_frame(path.as_ref(), frame_num)?;

        // Update frame data atomically
        let mut data = self.data.lock().unwrap();
        data.buffer = Arc::new(buffer);
        data.pixel_format = pixel_format;
        data.width = width;
        data.height = height;

        debug!("Loaded video frame {}: {}x{}", frame_num, width, height);
        Ok(())
    }

    /// Memory size in bytes
    pub fn mem(&self) -> usize {
        let data = self.data.lock().unwrap();
        match data.buffer.as_ref() {
            PixelBuffer::U8(vec) => vec.len(),      // 1 byte per u8
            PixelBuffer::F16(vec) => vec.len() * 2, // 2 bytes per f16
            PixelBuffer::F32(vec) => vec.len() * 4, // 4 bytes per f32
        }
    }

    /// Get status
    pub fn status(&self) -> FrameStatus {
        self.data.lock().unwrap().status.clone()
    }

    /// Smart status transition with automatic state management
    ///
    /// Handles state transitions intelligently:
    /// - Loaded → Header: Unloads pixel data, keeps metadata (width/height/filename)
    /// - Placeholder → Header: Loads metadata from file
    /// - Header → Loaded: Loads full pixel data
    /// - Error → Header: Resets to header state for retry
    /// - Error → Loaded: Attempts full reload
    /// - Loading → Header: Cancels load, keeps/loads metadata
    pub fn set_status(&self, new_status: FrameStatus) -> Result<(), FrameError> {
        let current_status = self.status();

        // Same status - no-op
        if current_status == new_status {
            return Ok(());
        }

        match (current_status, new_status) {
            // === Unload: Loaded → Header ===
            // Drop pixel data, keep metadata
            (FrameStatus::Loaded, FrameStatus::Header) => {
                let mut data = self.data.lock().unwrap();

                // Create green placeholder buffer with current dimensions
                let size = data.width * data.height * 4;
                let mut buffer_u8 = Vec::with_capacity(size);
                buffer_u8.resize(size, 0);
                for px in buffer_u8.chunks_exact_mut(4) {
                    px[1] = 100; // G channel
                    px[3] = 255; // A channel
                }

                data.buffer = Arc::new(PixelBuffer::U8(buffer_u8));
                data.pixel_format = PixelFormat::Rgba8;
                data.status = FrameStatus::Header;

                debug!("Unloaded frame to Header: {}x{}", data.width, data.height);
                Ok(())
            }

            // === Load metadata: Placeholder → Header ===
            (FrameStatus::Placeholder, FrameStatus::Header) => self.load_header(),

            // === Load full data: Header/Error → Loaded ===
            (FrameStatus::Header | FrameStatus::Error, FrameStatus::Loaded) => {
                // Call existing load() method
                self.load()
            }

            // === Reset error: Error → Header ===
            (FrameStatus::Error, FrameStatus::Header) => {
                let mut data = self.data.lock().unwrap();

                // Create green placeholder buffer with current dimensions
                let size = data.width * data.height * 4;
                let mut buffer_u8 = Vec::with_capacity(size);
                buffer_u8.resize(size, 0);
                for px in buffer_u8.chunks_exact_mut(4) {
                    px[1] = 100; // G channel
                    px[3] = 255; // A channel
                }

                data.buffer = Arc::new(PixelBuffer::U8(buffer_u8));
                data.pixel_format = PixelFormat::Rgba8;
                data.status = FrameStatus::Header;

                debug!("Reset error to Header: {}x{}", data.width, data.height);
                Ok(())
            }

            // === Cancel loading: Loading → Header ===
            // Can't actually cancel async load, but mark as Header for retry
            (FrameStatus::Loading, FrameStatus::Header) => {
                let mut data = self.data.lock().unwrap();
                data.status = FrameStatus::Header;
                debug!("Cancelled loading, marked as Header");
                Ok(())
            }

            // === Direct status change (other transitions) ===
            _ => {
                self.data.lock().unwrap().status = new_status;
                Ok(())
            }
        }
    }

    /// Get pixel buffer (returns Arc for efficient sharing)
    pub fn pixel_buffer(&self) -> Arc<PixelBuffer> {
        Arc::clone(&self.data.lock().unwrap().buffer)
    }

    /// Get pixel format
    pub fn pixel_format(&self) -> PixelFormat {
        self.data.lock().unwrap().pixel_format
    }

    /// Get pixels as u8 slice (for backward compatibility, only works with Rgba8 format)
    /// Returns error if the pixel format is not U8 (e.g., HDR formats F16/F32)
    pub fn pixels(&self) -> Result<Vec<u8>, FrameError> {
        let data = self.data.lock().unwrap();
        match data.buffer.as_ref() {
            PixelBuffer::U8(vec) => Ok(vec.clone()),
            PixelBuffer::F16(_) => Err(FrameError::UnsupportedFormat(
                "Frame uses F16 format, use pixel_buffer() for HDR data".into(),
            )),
            PixelBuffer::F32(_) => Err(FrameError::UnsupportedFormat(
                "Frame uses F32 format, use pixel_buffer() for HDR data".into(),
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

    /// Create cropped copy of frame without modifying original
    ///
    /// Returns new Frame with target dimensions. Does not mutate cached data.
    ///
    /// - If new size > current: pad with green placeholder color
    /// - If new size < current: crop according to alignment
    /// - If new size == current: returns copy
    ///
    /// # Arguments
    ///
    /// - `new_w`, `new_h`: Target dimensions
    /// - `align`: Alignment mode (Center or LeftTop)
    ///
    /// # Returns
    ///
    /// New Frame with cropped/padded dimensions
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use playa::frame::{Frame, CropAlign};
    /// let frame = Frame::new(1920, 1080);
    /// let cropped = frame.crop_copy(640, 480, CropAlign::Center);
    /// // Original frame unchanged, cropped is 640x480
    /// ```
    pub fn crop_copy(&self, new_w: usize, new_h: usize, align: CropAlign) -> Frame {
        // Create new frame with same data
        let cropped = Frame {
            data: Arc::new(Mutex::new(self.data.lock().unwrap().clone())),
            filename: self.filename.clone(),
        };

        // Crop the copy in-place
        cropped.crop(new_w, new_h, align);

        cropped
    }

    /// Crop or pad frame to new dimensions in-place
    ///
    /// **WARNING**: Mutates frame data. For encoding, use `crop_copy()` to avoid
    /// modifying cached frames.
    ///
    /// - If new size > current: pad with green placeholder color
    /// - If new size < current: crop according to alignment
    /// - If new size == current: no-op
    ///
    /// # Arguments
    ///
    /// - `new_w`, `new_h`: Target dimensions
    /// - `align`: Alignment mode (Center or LeftTop)
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use playa::frame::{Frame, CropAlign};
    /// let mut frame = Frame::new(1920, 1080);
    /// frame.crop(640, 480, CropAlign::Center); // Mutates frame to 640x480
    /// ```
    pub fn crop(&self, new_w: usize, new_h: usize, align: CropAlign) {
        let mut data = self.data.lock().unwrap();

        // Fast path: same size
        if data.width == new_w && data.height == new_h {
            return;
        }

        let old_w = data.width;
        let old_h = data.height;

        // Calculate offsets based on alignment
        let (src_offset_x, src_offset_y, dst_offset_x, dst_offset_y) = match align {
            CropAlign::Center => {
                let src_x = if old_w > new_w {
                    (old_w - new_w) / 2
                } else {
                    0
                };
                let src_y = if old_h > new_h {
                    (old_h - new_h) / 2
                } else {
                    0
                };
                let dst_x = if new_w > old_w {
                    (new_w - old_w) / 2
                } else {
                    0
                };
                let dst_y = if new_h > old_h {
                    (new_h - old_h) / 2
                } else {
                    0
                };
                (src_x, src_y, dst_x, dst_y)
            }
            CropAlign::LeftTop => (0, 0, 0, 0),
        };

        let copy_w = old_w.min(new_w);
        let copy_h = old_h.min(new_h);

        // Process based on pixel format
        match data.buffer.as_ref() {
            PixelBuffer::U8(old_buf) => {
                // Create new buffer with green placeholder
                let mut new_buf = Vec::with_capacity(new_w * new_h * 4);
                new_buf.resize(new_w * new_h * 4, 0);
                for px in new_buf.chunks_exact_mut(4) {
                    px[1] = 100; // G channel (placeholder green)
                    px[3] = 255; // A channel
                }

                // Copy pixel data
                for y in 0..copy_h {
                    let src_y = src_offset_y + y;
                    let dst_y = dst_offset_y + y;

                    for x in 0..copy_w {
                        let src_x = src_offset_x + x;
                        let dst_x = dst_offset_x + x;

                        let src_idx = (src_y * old_w + src_x) * 4;
                        let dst_idx = (dst_y * new_w + dst_x) * 4;

                        // Copy RGBA
                        new_buf[dst_idx..dst_idx + 4]
                            .copy_from_slice(&old_buf[src_idx..src_idx + 4]);
                    }
                }

                data.buffer = Arc::new(PixelBuffer::U8(new_buf));
            }

            PixelBuffer::F16(old_buf) => {
                // Create new buffer with green placeholder
                let mut new_buf = vec![F16::ZERO; new_w * new_h * 4];
                let green = F16::from_f32(100.0 / 255.0);
                let one = F16::ONE;

                for px in new_buf.chunks_exact_mut(4) {
                    px[1] = green; // G channel
                    px[3] = one; // A channel
                }

                // Copy pixel data
                for y in 0..copy_h {
                    let src_y = src_offset_y + y;
                    let dst_y = dst_offset_y + y;

                    for x in 0..copy_w {
                        let src_x = src_offset_x + x;
                        let dst_x = dst_offset_x + x;

                        let src_idx = (src_y * old_w + src_x) * 4;
                        let dst_idx = (dst_y * new_w + dst_x) * 4;

                        // Copy RGBA
                        new_buf[dst_idx..dst_idx + 4]
                            .copy_from_slice(&old_buf[src_idx..src_idx + 4]);
                    }
                }

                data.buffer = Arc::new(PixelBuffer::F16(new_buf));
            }

            PixelBuffer::F32(old_buf) => {
                // Create new buffer with green placeholder
                let mut new_buf = vec![0.0f32; new_w * new_h * 4];

                for px in new_buf.chunks_exact_mut(4) {
                    px[1] = 100.0 / 255.0; // G channel
                    px[3] = 1.0; // A channel
                }

                // Copy pixel data
                for y in 0..copy_h {
                    let src_y = src_offset_y + y;
                    let dst_y = dst_offset_y + y;

                    for x in 0..copy_w {
                        let src_x = src_offset_x + x;
                        let dst_x = dst_offset_x + x;

                        let src_idx = (src_y * old_w + src_x) * 4;
                        let dst_idx = (dst_y * new_w + dst_x) * 4;

                        // Copy RGBA
                        new_buf[dst_idx..dst_idx + 4]
                            .copy_from_slice(&old_buf[src_idx..src_idx + 4]);
                    }
                }

                data.buffer = Arc::new(PixelBuffer::F32(new_buf));
            }
        }

        // Update dimensions
        data.width = new_w;
        data.height = new_h;
    }

    /// Tonemap HDR frame to LDR (returns new U8 frame)
    ///
    /// Converts F16/F32 HDR data to U8 LDR using specified tonemapping curve.
    /// For U8 frames, returns cloned frame (no conversion needed).
    ///
    /// # Arguments
    ///
    /// - `mode`: Tonemapping algorithm (Clamp, ACES, Reinhard)
    ///
    /// # Returns
    ///
    /// New Frame with U8 pixel buffer (0-255 range)
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use playa::frame::{Frame, TonemapMode, PixelDepth};
    /// let hdr_frame = Frame::new_f16(1920, 1080);
    /// let ldr_frame = hdr_frame.tonemap(TonemapMode::ACES).unwrap();
    /// assert_eq!(ldr_frame.pixel_format(), playa::frame::PixelFormat::Rgba8);
    /// ```
    pub fn tonemap(&self, mode: TonemapMode) -> Result<Frame, FrameError> {
        let data = self.data.lock().unwrap();
        let (width, height) = (data.width, data.height);

        match data.buffer.as_ref() {
            PixelBuffer::U8(_) => {
                // Already LDR, just clone
                drop(data); // Release lock before cloning
                Ok(self.clone())
            }
            PixelBuffer::F16(hdr_data) => {
                let mut ldr_buf = Vec::with_capacity(width * height * 4);

                for chunk in hdr_data.chunks_exact(4) {
                    let r = chunk[0].to_f32();
                    let g = chunk[1].to_f32();
                    let b = chunk[2].to_f32();
                    let a = chunk[3].to_f32();

                    let (r_tm, g_tm, b_tm) = match mode {
                        TonemapMode::Clamp => (r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0)),
                        TonemapMode::ACES => {
                            // ACES filmic tone mapping (Narkowicz 2015)
                            let tonemap_aces = |x: f32| {
                                let a = 2.51;
                                let b = 0.03;
                                let c = 2.43;
                                let d = 0.59;
                                let e = 0.14;
                                ((x * (a * x + b)) / (x * (c * x + d) + e)).clamp(0.0, 1.0)
                            };
                            (tonemap_aces(r), tonemap_aces(g), tonemap_aces(b))
                        }
                        TonemapMode::Reinhard => {
                            // Reinhard photographic tone mapping
                            let tonemap_reinhard = |x: f32| (x / (1.0 + x)).clamp(0.0, 1.0);
                            (tonemap_reinhard(r), tonemap_reinhard(g), tonemap_reinhard(b))
                        }
                    };

                    // Convert [0,1] float → [0,255] u8
                    ldr_buf.push((r_tm * 255.0) as u8);
                    ldr_buf.push((g_tm * 255.0) as u8);
                    ldr_buf.push((b_tm * 255.0) as u8);
                    ldr_buf.push((a.clamp(0.0, 1.0) * 255.0) as u8); // Alpha unchanged
                }

                let ldr_data = FrameData {
                    buffer: Arc::new(PixelBuffer::U8(ldr_buf)),
                    pixel_format: PixelFormat::Rgba8,
                    width,
                    height,
                    status: data.status,
                };

                Ok(Frame {
                    data: Arc::new(Mutex::new(ldr_data)),
                    filename: self.filename.clone(),
                })
            }
            PixelBuffer::F32(hdr_data) => {
                let mut ldr_buf = Vec::with_capacity(width * height * 4);

                for chunk in hdr_data.chunks_exact(4) {
                    let r = chunk[0];
                    let g = chunk[1];
                    let b = chunk[2];
                    let a = chunk[3];

                    let (r_tm, g_tm, b_tm) = match mode {
                        TonemapMode::Clamp => (r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0)),
                        TonemapMode::ACES => {
                            // ACES filmic tone mapping (Narkowicz 2015)
                            let tonemap_aces = |x: f32| {
                                let a = 2.51;
                                let b = 0.03;
                                let c = 2.43;
                                let d = 0.59;
                                let e = 0.14;
                                ((x * (a * x + b)) / (x * (c * x + d) + e)).clamp(0.0, 1.0)
                            };
                            (tonemap_aces(r), tonemap_aces(g), tonemap_aces(b))
                        }
                        TonemapMode::Reinhard => {
                            // Reinhard photographic tone mapping
                            let tonemap_reinhard = |x: f32| (x / (1.0 + x)).clamp(0.0, 1.0);
                            (tonemap_reinhard(r), tonemap_reinhard(g), tonemap_reinhard(b))
                        }
                    };

                    // Convert [0,1] float → [0,255] u8
                    ldr_buf.push((r_tm * 255.0) as u8);
                    ldr_buf.push((g_tm * 255.0) as u8);
                    ldr_buf.push((b_tm * 255.0) as u8);
                    ldr_buf.push((a.clamp(0.0, 1.0) * 255.0) as u8); // Alpha unchanged
                }

                let ldr_data = FrameData {
                    buffer: Arc::new(PixelBuffer::U8(ldr_buf)),
                    pixel_format: PixelFormat::Rgba8,
                    width,
                    height,
                    status: data.status,
                };

                Ok(Frame {
                    data: Arc::new(Mutex::new(ldr_data)),
                    filename: self.filename.clone(),
                })
            }
        }
    }
}

/// Frame format conversion trait
///
/// Provides efficient conversion methods using FFmpeg swscale.
/// Methods return new Frames to avoid mutating cached data.
pub trait FrameConversion {
    /// Convert RGBA8 to RGB24 (removes alpha channel)
    ///
    /// Used as intermediate step for YUV conversion or RGB encoding.
    /// Fast operation: ~1-2ms for 1080p frame.
    ///
    /// **Note**: Only works with U8 format. HDR formats (F16/F32) require
    /// tonemapping first or use `to_rgb48()` for 10-bit encoding.
    ///
    /// # Returns
    /// RGB24 data (width * height * 3 bytes)
    fn to_rgb24(&self) -> Result<Vec<u8>, FrameError>;

    /// Convert any pixel format to RGB48 (16-bit per channel, removes alpha)
    ///
    /// Supports all pixel formats (U8/F16/F32):
    /// - U8: Scale 0-255 → 0-65535
    /// - F16/F32: Map 0.0-1.0 → 0-65535 (clamp out-of-range values)
    ///
    /// Used for 10-bit encoding pipeline (RGB48 → YUV422P10/YUV420P10).
    ///
    /// # Returns
    /// RGB48 data (width * height * 3 u16 values, little-endian)
    fn to_rgb48(&self) -> Result<Vec<u16>, FrameError>;
}

impl FrameConversion for Frame {
    fn to_rgb24(&self) -> Result<Vec<u8>, FrameError> {
        let buffer = self.pixel_buffer();
        match &*buffer {
            PixelBuffer::U8(rgba) => {
                let (width, height) = self.resolution();
                let mut rgb24 = Vec::with_capacity(width * height * 3);

                for chunk in rgba.chunks_exact(4) {
                    rgb24.push(chunk[0]); // R
                    rgb24.push(chunk[1]); // G
                    rgb24.push(chunk[2]); // B
                    // Skip alpha (chunk[3])
                }

                Ok(rgb24)
            }
            PixelBuffer::F16(_) => {
                Err(FrameError::UnsupportedFormat(
                    "Internal error: F16 format in RGB24 conversion path. Encoder should tonemap HDR→LDR first.".into()
                ))
            }
            PixelBuffer::F32(_) => {
                Err(FrameError::UnsupportedFormat(
                    "Internal error: F32 format in RGB24 conversion path. Encoder should tonemap HDR→LDR first.".into()
                ))
            }
        }
    }

    fn to_rgb48(&self) -> Result<Vec<u16>, FrameError> {
        let buffer = self.pixel_buffer();
        let (width, height) = self.resolution();

        match &*buffer {
            PixelBuffer::U8(rgba) => {
                // U8: Scale 0-255 → 0-65535
                let mut rgb48 = Vec::with_capacity(width * height * 3);

                for chunk in rgba.chunks_exact(4) {
                    // Scale from 8-bit to 16-bit: value * 257 (65535 / 255)
                    rgb48.push((chunk[0] as u16) * 257); // R
                    rgb48.push((chunk[1] as u16) * 257); // G
                    rgb48.push((chunk[2] as u16) * 257); // B
                    // Skip alpha (chunk[3])
                }

                Ok(rgb48)
            }
            PixelBuffer::F16(rgba) => {
                // F16: Map 0.0-1.0 → 0-65535 (clamp out-of-range)
                let mut rgb48 = Vec::with_capacity(width * height * 3);

                for chunk in rgba.chunks_exact(4) {
                    let r = chunk[0].to_f32().clamp(0.0, 1.0);
                    let g = chunk[1].to_f32().clamp(0.0, 1.0);
                    let b = chunk[2].to_f32().clamp(0.0, 1.0);

                    rgb48.push((r * 65535.0) as u16); // R
                    rgb48.push((g * 65535.0) as u16); // G
                    rgb48.push((b * 65535.0) as u16); // B
                    // Skip alpha
                }

                Ok(rgb48)
            }
            PixelBuffer::F32(rgba) => {
                // F32: Map 0.0-1.0 → 0-65535 (clamp out-of-range)
                let mut rgb48 = Vec::with_capacity(width * height * 3);

                for chunk in rgba.chunks_exact(4) {
                    let r = chunk[0].clamp(0.0, 1.0);
                    let g = chunk[1].clamp(0.0, 1.0);
                    let b = chunk[2].clamp(0.0, 1.0);

                    rgb48.push((r * 65535.0) as u16); // R
                    rgb48.push((g * 65535.0) as u16); // G
                    rgb48.push((b * 65535.0) as u16); // B
                    // Skip alpha
                }

                Ok(rgb48)
            }
        }
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
        let frame = Frame::new_u8(1920, 1080);

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
        let frame = Frame::new_unloaded(PathBuf::from("/nonexistent/path/test.jpg"));

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
        let frame = Frame::new_u8(100, 100);
        assert_eq!(frame.status(), FrameStatus::Placeholder);

        // Set filename → Header
        let frame = Frame::new_unloaded(PathBuf::from("test.png"));
        assert_eq!(frame.status(), FrameStatus::Header);

        // Load will transition to Loading → Error (file doesn't exist)
        let _ = frame.load();
        assert_eq!(frame.status(), FrameStatus::Error);
    }
}
