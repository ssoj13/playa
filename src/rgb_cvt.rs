//! RGB to YUV pixel format conversion using FFmpeg swscale
//!
//! Hardware encoders (NVENC, QSV, AMF) and many software encoders
//! require YUV pixel formats (YUV420P, NV12) instead of RGB.

use playa_ffmpeg as ffmpeg;

/// Convert RGB24 data to YUV420P FFmpeg frame
///
/// Used for hardware encoders (NVENC, QSV) and codecs that don't accept RGB directly.
///
/// # Arguments
/// * `rgb_data` - RGB24 pixel data (width * height * 3 bytes)
/// * `width` - Frame width in pixels
/// * `height` - Frame height in pixels
///
/// # Returns
/// FFmpeg video frame in YUV420P format, ready for encoding
pub fn rgb24_to_yuv420p(
    rgb_data: &[u8],
    width: u32,
    height: u32,
) -> Result<ffmpeg::util::frame::video::Video, String> {
    // Verify input data size
    let expected_size = (width * height * 3) as usize;
    if rgb_data.len() != expected_size {
        return Err(format!(
            "Invalid RGB data size: expected {} bytes, got {}",
            expected_size,
            rgb_data.len()
        ));
    }

    // Create source RGB24 frame
    let mut src_frame = ffmpeg::util::frame::video::Video::new(
        ffmpeg::format::Pixel::RGB24,
        width,
        height,
    );

    // Copy RGB data to source frame
    let src_stride = src_frame.stride(0);
    let row_bytes = (width * 3) as usize;

    {
        let dst_data = src_frame.data_mut(0);
        for y in 0..height as usize {
            let src_offset = y * row_bytes;
            let dst_offset = y * src_stride;
            dst_data[dst_offset..dst_offset + row_bytes]
                .copy_from_slice(&rgb_data[src_offset..src_offset + row_bytes]);
        }
    }

    // Create destination YUV420P frame
    let mut dst_frame = ffmpeg::util::frame::video::Video::new(
        ffmpeg::format::Pixel::YUV420P,
        width,
        height,
    );

    // Create swscale context for conversion
    let mut sws_ctx = ffmpeg::software::scaling::Context::get(
        ffmpeg::format::Pixel::RGB24,
        width,
        height,
        ffmpeg::format::Pixel::YUV420P,
        width,
        height,
        ffmpeg::software::scaling::Flags::BILINEAR,
    )
    .map_err(|e| format!("Failed to create swscale context: {}", e))?;

    // Convert RGB24 → YUV420P
    sws_ctx
        .run(&src_frame, &mut dst_frame)
        .map_err(|e| format!("swscale conversion failed: {}", e))?;

    Ok(dst_frame)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rgb_to_yuv_conversion() {
        playa_ffmpeg::init().expect("FFmpeg init failed");

        // Create simple RGB test pattern (red frame)
        let width = 64;
        let height = 48;
        let mut rgb_data = vec![0u8; (width * height * 3) as usize];

        // Fill with red color
        for pixel in rgb_data.chunks_exact_mut(3) {
            pixel[0] = 255; // R
            pixel[1] = 0;   // G
            pixel[2] = 0;   // B
        }

        // Convert to YUV
        let result = rgb24_to_yuv420p(&rgb_data, width, height);
        assert!(result.is_ok(), "Conversion failed");

        let yuv_frame = result.unwrap();
        assert_eq!(yuv_frame.width(), width);
        assert_eq!(yuv_frame.height(), height);
        assert_eq!(yuv_frame.format(), ffmpeg::format::Pixel::YUV420P);
    }

    #[test]
    fn test_invalid_data_size() {
        playa_ffmpeg::init().expect("FFmpeg init failed");

        let rgb_data = vec![0u8; 100]; // Wrong size
        let result = rgb24_to_yuv420p(&rgb_data, 64, 48);

        assert!(result.is_err());
        let err_msg = result.err().unwrap();
        assert!(err_msg.contains("Invalid RGB data size"));
    }
}
