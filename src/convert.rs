//! Frame format conversion utilities
//!
//! Provides efficient FFmpeg swscale-based conversion between pixel formats.
//! Reuses swscale contexts to avoid expensive recreations.

use playa_ffmpeg as ffmpeg;

/// Reusable swscale context for efficient format conversions
pub struct SwsContext {
    ctx: Option<ffmpeg::software::scaling::Context>,
    src_format: ffmpeg::format::Pixel,
    dst_format: ffmpeg::format::Pixel,
    width: u32,
    height: u32,
}

impl SwsContext {
    /// Create new swscale context with custom formats
    pub fn new(
        src_format: ffmpeg::format::Pixel,
        dst_format: ffmpeg::format::Pixel,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        let ctx = ffmpeg::software::scaling::Context::get(
            src_format,
            width,
            height,
            dst_format,
            width,
            height,
            ffmpeg::software::scaling::Flags::BILINEAR,
        )
        .map_err(|e| format!("Failed to create swscale context: {}", e))?;

        Ok(Self {
            ctx: Some(ctx),
            src_format,
            dst_format,
            width,
            height,
        })
    }

    /// Convert RGB24 data to destination format (YUV420P, YUV422P10, etc.)
    ///
    /// Uses the destination format specified during SwsContext creation.
    /// Reuses internal swscale context. Recreates if dimensions change.
    ///
    /// # Arguments
    /// * `rgb24_data` - RGB24 pixel data (width * height * 3 bytes)
    /// * `width` - Frame width
    /// * `height` - Frame height
    ///
    /// # Returns
    /// FFmpeg video frame in destination format ready for encoding
    pub fn convert(
        &mut self,
        rgb24_data: &[u8],
        width: u32,
        height: u32,
    ) -> Result<ffmpeg::util::frame::video::Video, String> {
        // Validate input size
        let expected_size = (width * height * 3) as usize;
        if rgb24_data.len() != expected_size {
            return Err(format!(
                "Invalid RGB24 data size: expected {} bytes, got {}",
                expected_size,
                rgb24_data.len()
            ));
        }

        // Recreate context if dimensions changed
        if self.width != width || self.height != height {
            self.recreate(width, height)?;
        }

        // Create source RGB24 frame
        let mut src_frame = ffmpeg::util::frame::video::Video::new(
            self.src_format,
            width,
            height,
        );

        // Copy RGB24 data to source frame
        let src_stride = src_frame.stride(0);
        let row_bytes = (width * 3) as usize;

        {
            let dst_data = src_frame.data_mut(0);
            for y in 0..height as usize {
                let src_offset = y * row_bytes;
                let dst_offset = y * src_stride;
                dst_data[dst_offset..dst_offset + row_bytes]
                    .copy_from_slice(&rgb24_data[src_offset..src_offset + row_bytes]);
            }
        }

        // Create destination frame with configured format
        let mut dst_frame = ffmpeg::util::frame::video::Video::new(
            self.dst_format,
            width,
            height,
        );

        // Convert using swscale context
        self.ctx
            .as_mut()
            .unwrap()
            .run(&src_frame, &mut dst_frame)
            .map_err(|e| format!("swscale conversion failed: {}", e))?;

        Ok(dst_frame)
    }

    /// Convert RGB48LE data (u16 per channel) to destination format (YUV420P10LE, YUV422P10LE)
    ///
    /// Used for 10-bit encoding pipeline. Handles 16-bit RGB data and converts to 10-bit YUV.
    /// Reuses internal swscale context. Recreates if dimensions change.
    ///
    /// # Arguments
    /// * `rgb48_data` - RGB48LE pixel data (width * height * 3 u16 values, little-endian)
    /// * `width` - Frame width
    /// * `height` - Frame height
    ///
    /// # Returns
    /// FFmpeg video frame in destination format (10-bit YUV) ready for encoding
    pub fn convert_rgb48(
        &mut self,
        rgb48_data: &[u16],
        width: u32,
        height: u32,
    ) -> Result<ffmpeg::util::frame::video::Video, String> {
        // Validate input size (3 u16 values per pixel = RGB)
        let expected_size = (width * height * 3) as usize;
        if rgb48_data.len() != expected_size {
            return Err(format!(
                "Invalid RGB48 data size: expected {} u16 values, got {}",
                expected_size,
                rgb48_data.len()
            ));
        }

        // Recreate context if dimensions changed
        if self.width != width || self.height != height {
            self.recreate(width, height)?;
        }

        // Create source RGB48LE frame (48-bit RGB, little-endian)
        let mut src_frame = ffmpeg::util::frame::video::Video::new(
            ffmpeg::format::Pixel::RGB48LE,
            width,
            height,
        );

        // Copy RGB48 data to source frame (u16 → bytes, little-endian)
        let src_stride = src_frame.stride(0);
        let row_pixels = width as usize;

        {
            let dst_data = src_frame.data_mut(0);
            for y in 0..height as usize {
                for x in 0..row_pixels {
                    let pixel_idx = (y * row_pixels + x) * 3; // 3 u16 per pixel
                    let dst_offset = y * src_stride + x * 6; // 6 bytes per pixel (3 * u16)

                    // Write R, G, B as little-endian u16
                    let r = rgb48_data[pixel_idx];
                    let g = rgb48_data[pixel_idx + 1];
                    let b = rgb48_data[pixel_idx + 2];

                    dst_data[dst_offset..dst_offset + 2].copy_from_slice(&r.to_le_bytes());
                    dst_data[dst_offset + 2..dst_offset + 4].copy_from_slice(&g.to_le_bytes());
                    dst_data[dst_offset + 4..dst_offset + 6].copy_from_slice(&b.to_le_bytes());
                }
            }
        }

        // Create destination frame with configured format (YUV420P10LE / YUV422P10LE)
        let mut dst_frame = ffmpeg::util::frame::video::Video::new(
            self.dst_format,
            width,
            height,
        );

        // Convert RGB48LE → YUV10 using swscale context
        self.ctx
            .as_mut()
            .unwrap()
            .run(&src_frame, &mut dst_frame)
            .map_err(|e| format!("RGB48→YUV10 swscale conversion failed: {}", e))?;

        Ok(dst_frame)
    }

    /// Recreate swscale context with new dimensions
    fn recreate(&mut self, width: u32, height: u32) -> Result<(), String> {
        self.ctx = Some(
            ffmpeg::software::scaling::Context::get(
                self.src_format,
                width,
                height,
                self.dst_format,
                width,
                height,
                ffmpeg::software::scaling::Flags::BILINEAR,
            )
            .map_err(|e| format!("Failed to recreate swscale context: {}", e))?,
        );
        self.width = width;
        self.height = height;
        Ok(())
    }
}
