//! Video decoder - FFmpeg-based video frame decoding
//!
//! Provides video metadata reading and frame-by-frame decoding

use super::frame::{FrameError, PixelBuffer, PixelFormat};
use log::warn;
use playa_ffmpeg as ffmpeg;
use std::path::Path;
use std::sync::Once;

static FFMPEG_LOG_INIT: Once = Once::new();

fn init_ffmpeg_logging() {
    FFMPEG_LOG_INIT.call_once(|| {
        unsafe {
            // Completely suppress all FFmpeg logging
            // AV_LOG_QUIET = -8 (silence all output including stderr)
            ffmpeg::ffi::av_log_set_level(ffmpeg::ffi::AV_LOG_QUIET);
        }
    });
}

pub struct VideoMetadata {
    pub frame_count: usize,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
}

impl VideoMetadata {
    pub fn from_file(path: &Path) -> Result<Self, FrameError> {
        init_ffmpeg_logging();

        let ictx = ffmpeg::format::input(path)
            .map_err(|e| FrameError::LoadError(format!("Failed to open video: {}", e)))?;

        let stream = ictx
            .streams()
            .best(ffmpeg::media::Type::Video)
            .ok_or_else(|| FrameError::LoadError("No video stream found".to_string()))?;

        let duration = stream.duration();
        let fps_rational = stream.avg_frame_rate();
        let time_base = stream.time_base();

        let duration_secs =
            duration as f64 * time_base.numerator() as f64 / time_base.denominator() as f64;
        let fps = fps_rational.numerator() as f64 / fps_rational.denominator() as f64;
        let frame_count = (duration_secs * fps) as usize;

        let codec_params = stream.parameters();
        let decoder_ctx =
            ffmpeg::codec::context::Context::from_parameters(codec_params).map_err(|e| {
                FrameError::LoadError(format!("Failed to create decoder context: {}", e))
            })?;
        let decoder = decoder_ctx
            .decoder()
            .video()
            .map_err(|e| FrameError::LoadError(format!("Failed to create video decoder: {}", e)))?;

        Ok(VideoMetadata {
            frame_count,
            width: decoder.width(),
            height: decoder.height(),
            fps,
        })
    }
}

/// Get video dimensions without decoding frames
pub fn get_video_dimensions(path: &Path) -> Result<(usize, usize), FrameError> {
    let metadata = VideoMetadata::from_file(path)?;
    Ok((metadata.width as usize, metadata.height as usize))
}

pub fn decode_frame(
    path: &Path,
    frame_num: usize,
) -> Result<(PixelBuffer, PixelFormat, usize, usize), FrameError> {
    init_ffmpeg_logging();

    let mut ictx = ffmpeg::format::input(path)
        .map_err(|e| FrameError::LoadError(format!("Failed to open video: {}", e)))?;

    let stream = ictx
        .streams()
        .best(ffmpeg::media::Type::Video)
        .ok_or_else(|| FrameError::LoadError("No video stream found".to_string()))?;
    let stream_idx = stream.index();

    let codec_params = stream.parameters();
    let mut decoder_ctx = ffmpeg::codec::context::Context::from_parameters(codec_params)
        .map_err(|e| FrameError::LoadError(format!("Failed to create decoder context: {}", e)))?;

    // Enable multi-threaded frame decoding (2-4x speedup)
    unsafe {
        (*decoder_ctx.as_mut_ptr()).thread_type = ffmpeg::ffi::FF_THREAD_FRAME;
        (*decoder_ctx.as_mut_ptr()).thread_count = 0; // Auto-detect CPU cores
    }

    let mut decoder = decoder_ctx
        .decoder()
        .video()
        .map_err(|e| FrameError::LoadError(format!("Failed to create video decoder: {}", e)))?;

    let width = decoder.width();
    let height = decoder.height();

    let mut scaler = ffmpeg::software::scaling::Context::get(
        decoder.format(),
        width,
        height,
        ffmpeg::format::Pixel::RGBA,
        width,
        height,
        ffmpeg::software::scaling::Flags::BILINEAR,
    )
    .map_err(|e| FrameError::LoadError(format!("Failed to create scaler: {}", e)))?;

    let fps = stream.avg_frame_rate();
    let fps_num = fps.numerator();
    let fps_den = fps.denominator();
    let time_base = stream.time_base();
    let target_ts = if fps_num > 0 && fps_den > 0 {
        let frame_tb = ffmpeg::ffi::AVRational {
            num: fps_den as i32,
            den: fps_num as i32,
        };
        let stream_tb = ffmpeg::ffi::AVRational {
            num: time_base.numerator() as i32,
            den: time_base.denominator() as i32,
        };
        Some(unsafe { ffmpeg::ffi::av_rescale_q(frame_num as i64, frame_tb, stream_tb) })
    } else {
        None
    };

    if let Some(target_ts) = target_ts {
        let seek_ret = unsafe {
            ffmpeg::ffi::av_seek_frame(
                ictx.as_mut_ptr(),
                stream_idx as i32,
                target_ts,
                ffmpeg::ffi::AVSEEK_FLAG_BACKWARD,
            )
        };
        if seek_ret < 0 {
            warn!("Video seek failed (ret={}), falling back to decode-from-start", seek_ret);
        }
    }

    let mut current_frame = 0;

    for (stream, packet) in ictx.packets() {
        if stream.index() == stream_idx {
            decoder
                .send_packet(&packet)
                .map_err(|e| FrameError::LoadError(format!("Failed to send packet: {}", e)))?;

            let mut decoded = ffmpeg::util::frame::video::Video::empty();
            while decoder.receive_frame(&mut decoded).is_ok() {
                let reached_target = if let Some(target_ts) = target_ts {
                    decoded
                        .pts()
                        .map(|pts| pts >= target_ts)
                        .unwrap_or(current_frame >= frame_num)
                } else {
                    current_frame >= frame_num
                };

                if reached_target {
                    let mut rgba_frame = ffmpeg::util::frame::video::Video::empty();
                    scaler.run(&decoded, &mut rgba_frame).map_err(|e| {
                        FrameError::LoadError(format!("Failed to scale frame: {}", e))
                    })?;

                    let rgba_data = rgba_frame.data(0);
                    let stride = rgba_frame.stride(0) as usize;
                    let row_bytes = (width * 4) as usize;
                    let mut output = vec![0u8; row_bytes * height as usize];
                    for y in 0..height as usize {
                        let src = y * stride;
                        let dst = y * row_bytes;
                        output[dst..dst + row_bytes]
                            .copy_from_slice(&rgba_data[src..src + row_bytes]);
                    }

                    return Ok((
                        PixelBuffer::U8(output),
                        PixelFormat::Rgba8,
                        width as usize,
                        height as usize,
                    ));
                }
                current_frame += 1;
            }
        }
    }

    Err(FrameError::LoadError(format!(
        "Frame {} not found in video",
        frame_num
    )))
}
