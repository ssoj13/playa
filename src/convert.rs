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
