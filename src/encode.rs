//! Video encoding module
//!
//! Handles encoding sequences to video files using FFmpeg encoders.
//! Supports hardware acceleration (NVENC/QSV) with CPU fallback.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use log::info;

use crate::cache::Cache;
use playa_ffmpeg as ffmpeg;

/// Encoder settings (persistent via AppSettings)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EncoderSettings {
    pub output_path: PathBuf,
    pub container: Container,
    pub codec: VideoCodec,
    pub encoder_impl: EncoderImpl,
    pub quality_mode: QualityMode,
    pub quality_value: u32, // CRF 18-28 or bitrate in kbps
    pub fps: f32, // Output framerate (frames per second)
}

impl Default for EncoderSettings {
    fn default() -> Self {
        Self {
            output_path: PathBuf::from("output.mp4"),
            container: Container::MP4,
            codec: VideoCodec::H264,
            encoder_impl: EncoderImpl::Auto,
            quality_mode: QualityMode::CRF,
            quality_value: 23, // Default CRF for H.264
            fps: 24.0, // Default framerate
        }
    }
}

/// Container format
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Container {
    MP4,
    MOV,
}

impl Container {
    pub fn extension(&self) -> &'static str {
        match self {
            Container::MP4 => "mp4",
            Container::MOV => "mov",
        }
    }

    pub fn all() -> &'static [Container] {
        &[Container::MP4, Container::MOV]
    }
}

impl std::fmt::Display for Container {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Container::MP4 => write!(f, "MP4"),
            Container::MOV => write!(f, "MOV"),
        }
    }
}

/// Video codec
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VideoCodec {
    H264,
    H265,
    ProRes,
    MPEG4,
}

impl VideoCodec {
    pub fn all() -> &'static [VideoCodec] {
        &[VideoCodec::H264, VideoCodec::H265, VideoCodec::ProRes, VideoCodec::MPEG4]
    }
}

impl std::fmt::Display for VideoCodec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VideoCodec::H264 => write!(f, "H.264"),
            VideoCodec::H265 => write!(f, "H.265 (HEVC)"),
            VideoCodec::ProRes => write!(f, "ProRes"),
            VideoCodec::MPEG4 => write!(f, "MPEG-4"),
        }
    }
}

/// Encoder implementation type
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EncoderImpl {
    Auto,     // Try hardware → fallback software
    Hardware, // NVENC/QSV/AMF only
    Software, // libx264/libx265/prores_ks only
}

impl EncoderImpl {
    pub fn all() -> &'static [EncoderImpl] {
        &[EncoderImpl::Auto, EncoderImpl::Hardware, EncoderImpl::Software]
    }
}

impl std::fmt::Display for EncoderImpl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EncoderImpl::Auto => write!(f, "Auto (HW → CPU)"),
            EncoderImpl::Hardware => write!(f, "Hardware only"),
            EncoderImpl::Software => write!(f, "Software (CPU)"),
        }
    }
}

/// Quality mode for encoding
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum QualityMode {
    CRF,     // Constant Rate Factor (quality-based)
    Bitrate, // Target bitrate in kbps
}

impl QualityMode {
    pub fn all() -> &'static [QualityMode] {
        &[QualityMode::CRF, QualityMode::Bitrate]
    }
}

impl std::fmt::Display for QualityMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QualityMode::CRF => write!(f, "CRF (Quality)"),
            QualityMode::Bitrate => write!(f, "Bitrate (kbps)"),
        }
    }
}

/// Progress updates during encoding
#[derive(Clone, Debug)]
pub struct EncodeProgress {
    pub current_frame: usize,
    pub total_frames: usize,
    pub stage: EncodeStage,
}

/// Encoding stages
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EncodeStage {
    Validating,   // Checking frame sizes
    Opening,      // Creating encoder
    Encoding,     // Encoding frames
    Flushing,     // Flushing encoder
    Complete,     // Successfully finished
    #[allow(dead_code)] // Used in ui_encode.rs pattern matching
    Error(String), // Failed with error
}

/// Encoding errors
#[derive(Debug)]
pub enum EncodeError {
    NoFrames,
    InconsistentFrameSizes {
        expected: (u32, u32),
        found: (u32, u32),
        frame: usize,
    },
    EncoderNotFound,
    HardwareEncoderUnavailable,
    OutputCreateFailed(String),
    EncodeFrameFailed(String),
    Cancelled,
}

impl std::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EncodeError::NoFrames => write!(f, "No frames to encode"),
            EncodeError::InconsistentFrameSizes { expected, found, frame } => {
                write!(
                    f,
                    "Frame {} has different size {}x{} (expected {}x{})",
                    frame, found.0, found.1, expected.0, expected.1
                )
            }
            EncodeError::EncoderNotFound => write!(f, "Encoder not found"),
            EncodeError::HardwareEncoderUnavailable => {
                write!(f, "Hardware encoder not available")
            }
            EncodeError::OutputCreateFailed(msg) => {
                write!(f, "Failed to create output file: {}", msg)
            }
            EncodeError::EncodeFrameFailed(msg) => {
                write!(f, "Frame encoding failed: {}", msg)
            }
            EncodeError::Cancelled => write!(f, "Encoding cancelled by user"),
        }
    }
}

impl std::error::Error for EncodeError {}

/// Get encoder name based on codec and implementation preference
fn get_encoder_name(codec: VideoCodec, encoder_impl: EncoderImpl) -> Result<&'static str, EncodeError> {
    match (codec, encoder_impl) {
        // H.264 encoders
        (VideoCodec::H264, EncoderImpl::Hardware) | (VideoCodec::H264, EncoderImpl::Auto) => {
            // Try NVENC first, then QSV
            if ffmpeg::encoder::find_by_name("h264_nvenc").is_some() {
                Ok("h264_nvenc")
            } else if ffmpeg::encoder::find_by_name("h264_qsv").is_some() {
                Ok("h264_qsv")
            } else if encoder_impl == EncoderImpl::Auto {
                Ok("libx264")  // Fallback to software
            } else {
                Err(EncodeError::HardwareEncoderUnavailable)
            }
        }
        (VideoCodec::H264, EncoderImpl::Software) => Ok("libx264"),

        // H.265 encoders
        (VideoCodec::H265, EncoderImpl::Hardware) | (VideoCodec::H265, EncoderImpl::Auto) => {
            if ffmpeg::encoder::find_by_name("hevc_nvenc").is_some() {
                Ok("hevc_nvenc")
            } else if ffmpeg::encoder::find_by_name("hevc_qsv").is_some() {
                Ok("hevc_qsv")
            } else if encoder_impl == EncoderImpl::Auto {
                Ok("libx265")  // Fallback to software
            } else {
                Err(EncodeError::HardwareEncoderUnavailable)
            }
        }
        (VideoCodec::H265, EncoderImpl::Software) => Ok("libx265"),

        // ProRes (software only)
        (VideoCodec::ProRes, _) => Ok("prores_ks"),

        // MPEG-4 (software only)
        (VideoCodec::MPEG4, _) => Ok("mpeg4"),
    }
}

/// Validate that all sequences have same dimensions
///
/// Uses sequence metadata (xres/yres) without loading frames.
/// Returns (width, height) if valid, error otherwise
fn validate_frame_sizes(
    cache: &Cache,
    _range: (usize, usize),
) -> Result<(u32, u32), EncodeError> {
    let sequences = cache.sequences();

    if sequences.is_empty() {
        return Err(EncodeError::NoFrames);
    }

    // Get dimensions from first sequence
    let first_seq = &sequences[0];
    let width = first_seq.xres();
    let height = first_seq.yres();

    // Verify all sequences have same dimensions
    for (idx, seq) in sequences.iter().enumerate().skip(1) {
        if seq.xres() != width || seq.yres() != height {
            return Err(EncodeError::InconsistentFrameSizes {
                expected: (width as u32, height as u32),
                found: (seq.xres() as u32, seq.yres() as u32),
                frame: idx,
            });
        }
    }

    Ok((width as u32, height as u32))
}

/// Main encoding function
///
/// Encodes sequence from play_range to output file.
/// Runs in separate thread, sends progress updates via channel.
pub fn encode_sequence(
    cache: &mut Cache,
    settings: &EncoderSettings,
    progress_tx: Sender<EncodeProgress>,
    cancel_flag: Arc<AtomicBool>,
) -> Result<(), EncodeError> {
    // Get play range
    let play_range = cache.get_play_range();
    let total_frames = play_range.1 - play_range.0 + 1;

    info!(
        "Starting encode: {} frames ({}..{}) to {:?}",
        total_frames, play_range.0, play_range.1, settings.output_path
    );

    // Stage 1: Validate frame sizes
    let _ = progress_tx.send(EncodeProgress {
        current_frame: 0,
        total_frames,
        stage: EncodeStage::Validating,
    });

    let (width, height) = validate_frame_sizes(cache, play_range)?;
    info!("Frame validation passed: {}x{}", width, height);

    // Check for cancellation
    if cancel_flag.load(Ordering::Relaxed) {
        return Err(EncodeError::Cancelled);
    }

    // Stage 2: Create encoder
    let _ = progress_tx.send(EncodeProgress {
        current_frame: 0,
        total_frames,
        stage: EncodeStage::Opening,
    });

    // Initialize FFmpeg (suppress logging)
    unsafe {
        ffmpeg::ffi::av_log_set_level(ffmpeg::ffi::AV_LOG_QUIET);
    }

    // Create output muxer (MP4 or MOV container)
    let _container_format = match settings.container {
        Container::MP4 => "mp4",
        Container::MOV => "mov",
    };

    let mut octx = ffmpeg::format::output(&settings.output_path)
        .map_err(|e| EncodeError::OutputCreateFailed(e.to_string()))?;

    // Find encoder by name (hardware with fallback or software)
    let encoder_name = get_encoder_name(settings.codec, settings.encoder_impl)?;
    info!("Looking for encoder: {}", encoder_name);

    let codec = ffmpeg::encoder::find_by_name(encoder_name)
        .ok_or_else(|| {
            info!("Encoder '{}' not found", encoder_name);
            EncodeError::EncoderNotFound
        })?;

    info!("Using encoder: {} for codec {:?}", encoder_name, settings.codec);

    // Create encoder context
    let mut encoder = ffmpeg::codec::context::Context::new_with_codec(codec)
        .encoder()
        .video()
        .map_err(|e| EncodeError::OutputCreateFailed(format!("Failed to create encoder: {}", e)))?;

    encoder.set_width(width);
    encoder.set_height(height);

    // Determine pixel format based on encoder
    // Hardware encoders (NVENC, QSV, AMF) and MPEG4 need YUV420P
    // Only libx264/libx265 can accept RGB24 directly
    let needs_yuv = matches!(
        encoder_name,
        "h264_nvenc" | "hevc_nvenc" | "h264_qsv" | "hevc_qsv" | "h264_amf" | "hevc_amf" | "mpeg4"
    );

    let pixel_format = if needs_yuv {
        ffmpeg::format::Pixel::YUV420P
    } else {
        ffmpeg::format::Pixel::RGB24
    };

    encoder.set_format(pixel_format);
    let fps_num = settings.fps as i32;
    encoder.set_frame_rate(Some(ffmpeg::util::rational::Rational::new(fps_num, 1)));
    encoder.set_time_base(ffmpeg::util::rational::Rational::new(1, fps_num));

    // Set quality parameters
    let mut opts = ffmpeg::Dictionary::new();
    match settings.quality_mode {
        QualityMode::CRF => {
            // CRF mode (quality-based)
            if encoder_name == "h264_nvenc" || encoder_name == "hevc_nvenc" {
                // NVENC uses -cq (constant quantizer) instead of -crf
                opts.set("rc", "constqp");  // Rate control mode
                opts.set("cq", &settings.quality_value.to_string());  // Quality (0-51, lower is better)
                opts.set("preset", "p4");  // NVENC preset (p1-p7, p4 = medium)
            } else if encoder_name == "libx264" || encoder_name == "libx265" {
                // Software encoders use standard CRF
                opts.set("crf", &settings.quality_value.to_string());
                opts.set("preset", "medium");
            } else if encoder_name == "h264_qsv" || encoder_name == "hevc_qsv" {
                // QSV uses global_quality
                opts.set("global_quality", &settings.quality_value.to_string());
            } else if encoder_name == "prores_ks" {
                // ProRes profile: 0=Proxy, 1=LT, 2=Standard(422), 3=HQ, 4=4444, 5=4444XQ
                // Map CRF value to profile (lower CRF = higher quality)
                let profile = if settings.quality_value <= 18 {
                    "3"  // HQ for best quality
                } else if settings.quality_value <= 23 {
                    "2"  // Standard (422) for balanced
                } else {
                    "1"  // LT for lower quality
                };
                info!("ProRes encoding with profile {} (CRF {})", profile, settings.quality_value);
                opts.set("profile", profile);
                opts.set("vendor", "apl0");  // Apple vendor ID for compatibility
            }
        }
        QualityMode::Bitrate => {
            // Bitrate mode
            encoder.set_bit_rate(settings.quality_value as usize * 1000);  // Convert kbps to bps

            // MPEG4-specific options
            if encoder_name == "mpeg4" {
                opts.set("q:v", "5");  // Quality scale 1-31 (lower is better)
            }
        }
    }

    // Open encoder with options
    info!("Opening encoder '{}' with pixel_format={:?}, size={}x{}", encoder_name, encoder.format(), width, height);
    let mut encoder = encoder.open_with(opts)
        .map_err(|e| {
            EncodeError::OutputCreateFailed(format!("Failed to open encoder '{}': {}", encoder_name, e))
        })?;

    // Add stream and set parameters from encoder
    let mut ost = octx.add_stream(codec)
        .map_err(|e| EncodeError::OutputCreateFailed(format!("Failed to add stream: {}", e)))?;
    ost.set_parameters(&encoder);

    // Set stream time_base to match encoder (critical for proper timestamps)
    ost.set_time_base(encoder.time_base());

    // Set container options (MP4: move moov atom to start for seekability)
    let mut container_opts = ffmpeg::Dictionary::new();
    if matches!(settings.container, Container::MP4) {
        container_opts.set("movflags", "faststart");
    }

    // Write container header
    octx.set_metadata(octx.metadata().to_owned());
    octx.write_header_with(container_opts)
        .map_err(|e| EncodeError::OutputCreateFailed(format!("Failed to write header: {}", e)))?;

    info!(
        "Encoder initialized: {}x{} @ {} fps, quality mode: {:?}",
        width, height, settings.fps, settings.quality_mode
    );

    // Check for cancellation
    if cancel_flag.load(Ordering::Relaxed) {
        return Err(EncodeError::Cancelled);
    }

    // Stage 3: Encoding loop
    let _ = progress_tx.send(EncodeProgress {
        current_frame: 0,
        total_frames,
        stage: EncodeStage::Encoding,
    });

    let mut pts = 0i64;

    for frame_idx in play_range.0..=play_range.1 {
        // Check for cancellation
        if cancel_flag.load(Ordering::Relaxed) {
            return Err(EncodeError::Cancelled);
        }

        // Get frame from cache
        let frame = cache.get_frame(frame_idx)
            .ok_or_else(|| EncodeError::EncodeFrameFailed(format!("Frame {} not in cache", frame_idx)))?;

        // Ensure frame is loaded (skip Placeholder frames - they're already in memory)
        if frame.status() == crate::frame::FrameStatus::Header {
            frame.load()
                .map_err(|e| EncodeError::EncodeFrameFailed(format!("Failed to load frame {}: {}", frame_idx, e)))?;
        }

        // Get pixel data (must be RGB24/U8 format)
        let pixel_buffer = frame.pixel_buffer();
        let rgb_data = match &*pixel_buffer {
            crate::frame::PixelBuffer::U8(data) => data,
            _ => {
                return Err(EncodeError::EncodeFrameFailed(
                    format!("Frame {} has unsupported format (expected U8/RGBA8)", frame_idx)
                ));
            }
        };

        // Convert RGBA to RGB24 (remove alpha channel)
        let (frame_width, frame_height) = frame.resolution();
        let mut rgb24_data = vec![0u8; frame_width * frame_height * 3];

        for y in 0..frame_height {
            for x in 0..frame_width {
                let src_idx = (y * frame_width + x) * 4;  // RGBA stride
                let dst_idx = (y * frame_width + x) * 3;  // RGB stride

                rgb24_data[dst_idx] = rgb_data[src_idx];         // R
                rgb24_data[dst_idx + 1] = rgb_data[src_idx + 1]; // G
                rgb24_data[dst_idx + 2] = rgb_data[src_idx + 2]; // B
            }
        }

        // Create FFmpeg frame - RGB24 or YUV420P depending on encoder
        let mut ffmpeg_frame = if needs_yuv {
            // Convert RGB24 → YUV420P for hardware encoders
            crate::rgb_cvt::rgb24_to_yuv420p(&rgb24_data, width, height)
                .map_err(|e| EncodeError::EncodeFrameFailed(format!("RGB→YUV conversion failed: {}", e)))?
        } else {
            // Use RGB24 directly for libx264/libx265
            let mut frame = ffmpeg::util::frame::video::Video::new(
                ffmpeg::format::Pixel::RGB24,
                width,
                height,
            );

            // Copy RGB24 data to frame
            let dst_stride = frame.stride(0);
            let src_stride = (width * 3) as usize;

            {
                let dst_data = frame.data_mut(0);
                for y in 0..height as usize {
                    let src_offset = y * src_stride;
                    let dst_offset = y * dst_stride;
                    let row_bytes = src_stride;

                    dst_data[dst_offset..dst_offset + row_bytes]
                        .copy_from_slice(&rgb24_data[src_offset..src_offset + row_bytes]);
                }
            }

            frame
        };

        // Set PTS (presentation timestamp)
        ffmpeg_frame.set_pts(Some(pts));
        pts += 1;

        // Send frame to encoder
        encoder.send_frame(&ffmpeg_frame)
            .map_err(|e| EncodeError::EncodeFrameFailed(format!("Failed to send frame {}: {}", frame_idx, e)))?;

        // Check for cancellation after sending frame
        if cancel_flag.load(Ordering::Relaxed) {
            return Err(EncodeError::Cancelled);
        }

        // Receive encoded packets
        let mut encoded = ffmpeg::Packet::empty();
        while encoder.receive_packet(&mut encoded).is_ok() {
            // Check for cancellation during packet receiving
            if cancel_flag.load(Ordering::Relaxed) {
                return Err(EncodeError::Cancelled);
            }
            encoded.set_stream(0);

            // Set packet duration (1 frame in time_base units)
            encoded.set_duration(1);

            // Set DTS equal to PTS for I-frame sequences (no B-frames)
            if encoded.dts().is_none() {
                encoded.set_dts(encoded.pts());
            }

            encoded.write_interleaved(&mut octx)
                .map_err(|e| EncodeError::EncodeFrameFailed(format!("Failed to write packet: {}", e)))?;
        }

        // Update progress
        let current_frame = frame_idx - play_range.0 + 1;
        let _ = progress_tx.send(EncodeProgress {
            current_frame,
            total_frames,
            stage: EncodeStage::Encoding,
        });

        if current_frame.is_multiple_of(10) {
            info!("Encoded frame {}/{}", current_frame, total_frames);
        }
    }

    // Stage 4: Flush encoder
    let _ = progress_tx.send(EncodeProgress {
        current_frame: total_frames,
        total_frames,
        stage: EncodeStage::Flushing,
    });

    info!("Flushing encoder...");

    // Send flush signal to encoder
    encoder.send_eof()
        .map_err(|e| EncodeError::EncodeFrameFailed(format!("Failed to flush encoder: {}", e)))?;

    // Receive remaining packets
    let mut encoded = ffmpeg::Packet::empty();
    while encoder.receive_packet(&mut encoded).is_ok() {
        // Check for cancellation during flush
        if cancel_flag.load(Ordering::Relaxed) {
            return Err(EncodeError::Cancelled);
        }

        encoded.set_stream(0);

        // Set packet duration (1 frame in time_base units)
        encoded.set_duration(1);

        // Set DTS equal to PTS for I-frame sequences (no B-frames)
        if encoded.dts().is_none() {
            encoded.set_dts(encoded.pts());
        }

        encoded.write_interleaved(&mut octx)
            .map_err(|e| EncodeError::EncodeFrameFailed(format!("Failed to write packet: {}", e)))?;
    }

    // Write container trailer
    octx.write_trailer()
        .map_err(|e| EncodeError::OutputCreateFailed(format!("Failed to write trailer: {}", e)))?;

    // Stage 5: Complete
    let _ = progress_tx.send(EncodeProgress {
        current_frame: total_frames,
        total_frames,
        stage: EncodeStage::Complete,
    });

    info!("Encoding complete: {} frames written to {:?}", total_frames, settings.output_path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::Cache;
    use crate::frame::Frame;
    use crate::sequence::Sequence;

    /// Test encoding with placeholder frames
    #[test]
    fn test_encode_placeholder_frames() {
        // Initialize FFmpeg
        playa_ffmpeg::init().expect("Failed to init FFmpeg");

        // Try to find ANY working video encoder
        println!("Testing available video encoders:");
        let test_encoders = [
            "libx264", "h264_nvenc", "h264_qsv",  // H.264
            "libx265", "hevc_nvenc", "hevc_qsv",  // H.265
            "mpeg4", "libxvid",                    // MPEG-4
            "libvpx", "libvpx-vp9",               // VP8/VP9
            "libaom-av1",                          // AV1
        ];

        let mut found_encoder: Option<&str> = None;
        for name in &test_encoders {
            if ffmpeg::encoder::find_by_name(name).is_some() {
                println!("  ✓ {} FOUND", name);
                if found_encoder.is_none() {
                    found_encoder = Some(name);
                }
            } else {
                println!("  ✗ {} not found", name);
            }
        }

        if found_encoder.is_none() {
            panic!("NO VIDEO ENCODERS FOUND - FFmpeg build has no encoding support! Skipping test.");
        }

        println!("\nUsing encoder: {}", found_encoder.unwrap());

        // Create cache with 100 placeholder frames
        let (mut cache, _ui_rx) = Cache::new(0.1, None);

        // Create sequence with 100 placeholder frames (no files)
        // Placeholders are green RGBA [0,100,0,255] by default
        let frames: Vec<Frame> = (0..100).map(|_| Frame::new(640, 480)).collect();
        let seq = Sequence::from_frames(
            frames,
            "test_placeholder.*.rgb".to_string(),
            640,
            480
        );

        cache.append_seq(seq);

        // Set play range to encode only frames 10-49 (40 frames total)
        cache.set_play_range(10, 49);
        let (play_start, play_end) = cache.get_play_range();
        println!("Play range set: {}..{} ({} frames)", play_start, play_end, play_end - play_start + 1);

        // Determine which codec to use based on available encoder
        let (codec, encoder_impl, encoder_name) = if ffmpeg::encoder::find_by_name("h264_nvenc").is_some() {
            println!("\n🎬 Using NVENC hardware encoder");
            (VideoCodec::H264, EncoderImpl::Hardware, "h264_nvenc")
        } else if ffmpeg::encoder::find_by_name("libx264").is_some() {
            println!("\n🎬 Using libx264 software encoder");
            (VideoCodec::H264, EncoderImpl::Software, "libx264")
        } else if ffmpeg::encoder::find_by_name("mpeg4").is_some() {
            println!("\n🎬 Using mpeg4 encoder");
            (VideoCodec::MPEG4, EncoderImpl::Software, "mpeg4")
        } else {
            println!("\n⚠ No compatible encoder available, skipping encoding test");
            println!("   Available: {}", found_encoder.unwrap());
            println!("   Need: libx264, h264_nvenc, or mpeg4");
            println!("\n✓ Test infrastructure verified:");
            println!("  - Cache with 100 placeholder frames created");
            println!("  - Encoder discovery working");
            return;
        };

        // Setup encoding - create file in current directory
        let output_path = std::path::PathBuf::from("test_encode_output.mp4");
        let _ = std::fs::remove_file(&output_path);

        let settings = EncoderSettings {
            output_path: output_path.clone(),
            container: Container::MP4,
            codec,
            encoder_impl,
            quality_mode: QualityMode::Bitrate,
            quality_value: 2000,  // 2 Mbps
        };

        // Create progress channel
        let (tx, rx) = std::sync::mpsc::channel();
        let cancel_flag = Arc::new(AtomicBool::new(false));

        // Run encoding
        let abs_path = std::fs::canonicalize(&output_path).unwrap_or_else(|_| {
            std::env::current_dir().unwrap().join(&output_path)
        });
        println!("Encoding frames {}..{} to: {}", play_start, play_end, abs_path.display());
        let result = encode_sequence(&mut cache, &settings, tx, cancel_flag);

        // Check progress updates
        let mut last_progress: Option<EncodeProgress> = None;
        while let Ok(progress) = rx.try_recv() {
            last_progress = Some(progress);
        }

        // Verify encoding succeeded
        assert!(result.is_ok(), "Encoding failed: {:?}", result);

        // Verify output file exists and is not empty
        assert!(output_path.exists(), "Output file was not created");
        let metadata = std::fs::metadata(&output_path).expect("Failed to read output file metadata");
        assert!(metadata.len() > 0, "Output file is empty");

        println!("✓ Encoding test passed!");
        println!("  Encoder: {}", encoder_name);
        println!("  Output: {}", abs_path.display());
        println!("  Size: {} bytes ({:.2} KB)", metadata.len(), metadata.len() as f64 / 1024.0);

        // Verify progress reached completion
        if let Some(progress) = last_progress {
            assert_eq!(progress.stage, EncodeStage::Complete, "Encoding did not complete");
            println!("  Frames: {}/{} (play range: {}..{})",
                progress.current_frame, progress.total_frames, play_start, play_end);
            assert_eq!(progress.total_frames, 40, "Should encode exactly 40 frames from play range");
        }

        // Cleanup
        // let _ = std::fs::remove_file(&output_path);
    }
}
