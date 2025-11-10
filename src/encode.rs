//! Video encoding module
//!
//! Handles encoding sequences to video files using FFmpeg encoders.
//! Supports hardware acceleration (NVENC/QSV) with CPU fallback.

use log::info;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

use crate::cache::Cache;
use crate::convert::SwsContext;
use crate::frame::{CropAlign, FrameConversion, FrameStatus};
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
    pub fps: f32,           // Output framerate (frames per second)

    // Per-codec optional settings
    #[serde(default)]
    pub preset: Option<String>, // H.264/H.265 preset (e.g. "medium", "p4")
    #[serde(default)]
    pub profile: Option<String>, // H.264 profile (e.g. "high")
    #[serde(default)]
    pub prores_profile: Option<ProResProfile>, // ProRes profile
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
            fps: 24.0,         // Default framerate
            preset: Some("medium".to_string()),
            profile: Some("high".to_string()),
            prores_profile: Some(ProResProfile::Standard),
        }
    }
}

/// H.264 specific settings
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct H264Settings {
    pub encoder_impl: EncoderImpl,
    pub quality_mode: QualityMode,
    pub quality_value: u32, // CRF 0-51 or bitrate kbps
    pub preset: String,     // ultrafast/fast/medium/slow/veryslow (libx264) or p1-p7 (nvenc)
    pub profile: String,    // baseline/main/high (libx264 only)
}

impl Default for H264Settings {
    fn default() -> Self {
        Self {
            encoder_impl: EncoderImpl::Auto,
            quality_mode: QualityMode::CRF,
            quality_value: 23,
            preset: "medium".to_string(),
            profile: "high".to_string(),
        }
    }
}

/// H.265/HEVC specific settings
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct H265Settings {
    pub encoder_impl: EncoderImpl,
    pub quality_mode: QualityMode,
    pub quality_value: u32, // CRF 0-51 or bitrate kbps
    pub preset: String,     // ultrafast/fast/medium/slow/veryslow (libx265) or p1-p7 (nvenc)
}

impl Default for H265Settings {
    fn default() -> Self {
        Self {
            encoder_impl: EncoderImpl::Auto,
            quality_mode: QualityMode::CRF,
            quality_value: 28, // H.265 default is higher than H.264
            preset: "medium".to_string(),
        }
    }
}

/// ProRes profile variants
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[allow(clippy::upper_case_acronyms)]
pub enum ProResProfile {
    Proxy,              // 0
    LT,                 // 1
    Standard,           // 2 (422)
    HQ,                 // 3
    FourFourFourFour,   // 4 (4444)
    FourFourFourFourXQ, // 5 (4444XQ)
}

impl ProResProfile {
    pub fn all() -> &'static [ProResProfile] {
        &[
            ProResProfile::Proxy,
            ProResProfile::LT,
            ProResProfile::Standard,
            ProResProfile::HQ,
            ProResProfile::FourFourFourFour,
            ProResProfile::FourFourFourFourXQ,
        ]
    }

    pub fn to_ffmpeg_value(self) -> &'static str {
        match self {
            ProResProfile::Proxy => "0",
            ProResProfile::LT => "1",
            ProResProfile::Standard => "2",
            ProResProfile::HQ => "3",
            ProResProfile::FourFourFourFour => "4",
            ProResProfile::FourFourFourFourXQ => "5",
        }
    }
}

impl std::fmt::Display for ProResProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProResProfile::Proxy => write!(f, "Proxy"),
            ProResProfile::LT => write!(f, "LT"),
            ProResProfile::Standard => write!(f, "422 (Standard)"),
            ProResProfile::HQ => write!(f, "422 HQ"),
            ProResProfile::FourFourFourFour => write!(f, "4444"),
            ProResProfile::FourFourFourFourXQ => write!(f, "4444 XQ"),
        }
    }
}

/// ProRes specific settings
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProResSettings {
    pub profile: ProResProfile,
}

impl Default for ProResSettings {
    fn default() -> Self {
        Self {
            profile: ProResProfile::Standard,
        }
    }
}

/// AV1 specific settings
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AV1Settings {
    pub encoder_impl: EncoderImpl,
    pub quality_mode: QualityMode,
    pub quality_value: u32, // CRF 0-63 or bitrate kbps
    pub preset: String,     // 0-13 for libaom/libsvtav1, p1-p7 for nvenc/qsv/amf
}

impl Default for AV1Settings {
    fn default() -> Self {
        Self {
            encoder_impl: EncoderImpl::Auto,
            quality_mode: QualityMode::CRF,
            quality_value: 30, // AV1 default (roughly equivalent to H.264 CRF 23)
            preset: "6".to_string(), // Medium speed for SVT-AV1 (0=slowest, 13=fastest)
        }
    }
}

/// All codec-specific settings
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CodecSettings {
    pub h264: H264Settings,
    pub h265: H265Settings,
    pub prores: ProResSettings,
    pub av1: AV1Settings,
}

/// Container format
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[allow(clippy::upper_case_acronyms)]
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
#[allow(clippy::upper_case_acronyms)]
pub enum VideoCodec {
    H264,
    H265,
    ProRes,
    AV1,
}

impl VideoCodec {
    pub fn all() -> &'static [VideoCodec] {
        &[
            VideoCodec::H264,
            VideoCodec::H265,
            VideoCodec::AV1,
            VideoCodec::ProRes,
        ]
    }

    /// Check if any encoder is available for this codec
    pub fn is_available(&self) -> bool {
        match self {
            VideoCodec::H264 => {
                // Check all H.264 encoders
                #[cfg(target_os = "macos")]
                if ffmpeg::encoder::find_by_name("h264_videotoolbox").is_some() {
                    return true;
                }

                ffmpeg::encoder::find_by_name("h264_nvenc").is_some()
                    || ffmpeg::encoder::find_by_name("h264_qsv").is_some()
                    || ffmpeg::encoder::find_by_name("h264_amf").is_some()
                    || ffmpeg::encoder::find_by_name("libx264").is_some()
            }
            VideoCodec::H265 => {
                // Check all H.265 encoders
                #[cfg(target_os = "macos")]
                if ffmpeg::encoder::find_by_name("hevc_videotoolbox").is_some() {
                    return true;
                }

                ffmpeg::encoder::find_by_name("hevc_nvenc").is_some()
                    || ffmpeg::encoder::find_by_name("hevc_qsv").is_some()
                    || ffmpeg::encoder::find_by_name("hevc_amf").is_some()
                    || ffmpeg::encoder::find_by_name("libx265").is_some()
            }
            VideoCodec::AV1 => {
                // Check all AV1 encoders (hardware first, then software)
                ffmpeg::encoder::find_by_name("av1_nvenc").is_some()
                    || ffmpeg::encoder::find_by_name("av1_qsv").is_some()
                    || ffmpeg::encoder::find_by_name("av1_amf").is_some()
                    || ffmpeg::encoder::find_by_name("libsvtav1").is_some()
                    || ffmpeg::encoder::find_by_name("libaom-av1").is_some()
            }
            VideoCodec::ProRes => ffmpeg::encoder::find_by_name("prores_ks").is_some(),
        }
    }
}

impl std::fmt::Display for VideoCodec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VideoCodec::H264 => write!(f, "H.264"),
            VideoCodec::H265 => write!(f, "H.265 (HEVC)"),
            VideoCodec::AV1 => write!(f, "AV1"),
            VideoCodec::ProRes => write!(f, "ProRes"),
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
        &[
            EncoderImpl::Auto,
            EncoderImpl::Hardware,
            EncoderImpl::Software,
        ]
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
#[allow(clippy::upper_case_acronyms)]
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
    Validating, // Checking frame sizes
    Opening,    // Creating encoder
    Encoding,   // Encoding frames
    Flushing,   // Flushing encoder
    Complete,   // Successfully finished
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
            EncodeError::InconsistentFrameSizes {
                expected,
                found,
                frame,
            } => {
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
fn get_encoder_name(
    codec: VideoCodec,
    encoder_impl: EncoderImpl,
) -> Result<&'static str, EncodeError> {
    match (codec, encoder_impl) {
        // H.264 encoders
        (VideoCodec::H264, EncoderImpl::Hardware) | (VideoCodec::H264, EncoderImpl::Auto) => {
            // Priority: VideoToolbox (macOS) > NVENC (NVIDIA) > QSV (Intel) > AMF (AMD) > Software
            #[cfg(target_os = "macos")]
            if ffmpeg::encoder::find_by_name("h264_videotoolbox").is_some() {
                return Ok("h264_videotoolbox");
            }

            if ffmpeg::encoder::find_by_name("h264_nvenc").is_some() {
                Ok("h264_nvenc")
            } else if ffmpeg::encoder::find_by_name("h264_qsv").is_some() {
                Ok("h264_qsv")
            } else if ffmpeg::encoder::find_by_name("h264_amf").is_some() {
                Ok("h264_amf")
            } else if encoder_impl == EncoderImpl::Auto {
                Ok("libx264") // Fallback to software
            } else {
                Err(EncodeError::HardwareEncoderUnavailable)
            }
        }
        (VideoCodec::H264, EncoderImpl::Software) => Ok("libx264"),

        // H.265 encoders
        (VideoCodec::H265, EncoderImpl::Hardware) | (VideoCodec::H265, EncoderImpl::Auto) => {
            // Priority: VideoToolbox (macOS) > NVENC (NVIDIA) > QSV (Intel) > AMF (AMD) > Software
            #[cfg(target_os = "macos")]
            if ffmpeg::encoder::find_by_name("hevc_videotoolbox").is_some() {
                return Ok("hevc_videotoolbox");
            }

            if ffmpeg::encoder::find_by_name("hevc_nvenc").is_some() {
                Ok("hevc_nvenc")
            } else if ffmpeg::encoder::find_by_name("hevc_qsv").is_some() {
                Ok("hevc_qsv")
            } else if ffmpeg::encoder::find_by_name("hevc_amf").is_some() {
                Ok("hevc_amf")
            } else if encoder_impl == EncoderImpl::Auto {
                Ok("libx265") // Fallback to software
            } else {
                Err(EncodeError::HardwareEncoderUnavailable)
            }
        }
        (VideoCodec::H265, EncoderImpl::Software) => Ok("libx265"),

        // AV1 encoders
        (VideoCodec::AV1, EncoderImpl::Hardware) | (VideoCodec::AV1, EncoderImpl::Auto) => {
            // Priority: NVENC (RTX 40xx) > QSV (Arc) > AMF (RDNA 3) > SVT-AV1 (software)
            if ffmpeg::encoder::find_by_name("av1_nvenc").is_some() {
                Ok("av1_nvenc")
            } else if ffmpeg::encoder::find_by_name("av1_qsv").is_some() {
                Ok("av1_qsv")
            } else if ffmpeg::encoder::find_by_name("av1_amf").is_some() {
                Ok("av1_amf")
            } else if encoder_impl == EncoderImpl::Auto {
                // Fallback to software: SVT-AV1 (faster) > libaom (better quality)
                if ffmpeg::encoder::find_by_name("libsvtav1").is_some() {
                    Ok("libsvtav1")
                } else {
                    Ok("libaom-av1")
                }
            } else {
                Err(EncodeError::HardwareEncoderUnavailable)
            }
        }
        (VideoCodec::AV1, EncoderImpl::Software) => {
            // Software fallback: prefer SVT-AV1 for speed
            if ffmpeg::encoder::find_by_name("libsvtav1").is_some() {
                Ok("libsvtav1")
            } else {
                Ok("libaom-av1")
            }
        }

        // ProRes (software only)
        (VideoCodec::ProRes, _) => Ok("prores_ks"),
    }
}

/// Validate that all sequences have same dimensions
///
/// Uses sequence metadata (xres/yres) without loading frames.
/// Returns (width, height) if valid, error otherwise
fn validate_frame_sizes(cache: &Cache, _range: (usize, usize)) -> Result<(u32, u32), EncodeError> {
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

    let codec = ffmpeg::encoder::find_by_name(encoder_name).ok_or_else(|| {
        info!("Encoder '{}' not found", encoder_name);
        EncodeError::EncoderNotFound
    })?;

    info!(
        "Using encoder: {} for codec {:?}",
        encoder_name, settings.codec
    );

    // Create encoder context
    let mut encoder = ffmpeg::codec::context::Context::new_with_codec(codec)
        .encoder()
        .video()
        .map_err(|e| EncodeError::OutputCreateFailed(format!("Failed to create encoder: {}", e)))?;

    encoder.set_width(width);
    encoder.set_height(height);

    // Determine pixel format based on encoder
    // Hardware encoders (NVENC, QSV, AMF) and AV1 encoders need YUV420P
    // Only libx264/libx265 can accept RGB24 directly
    let needs_yuv = matches!(
        encoder_name,
        "h264_nvenc"
            | "hevc_nvenc"
            | "av1_nvenc"
            | "h264_qsv"
            | "hevc_qsv"
            | "av1_qsv"
            | "h264_amf"
            | "hevc_amf"
            | "av1_amf"
            | "h264_videotoolbox"
            | "hevc_videotoolbox"
            | "libsvtav1"
            | "libaom-av1"
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
                opts.set("rc", "constqp"); // Rate control mode
                opts.set("cq", &settings.quality_value.to_string()); // Quality (0-51, lower is better)
                if let Some(ref preset) = settings.preset {
                    opts.set("preset", preset); // NVENC preset (p1-p7)
                }
            } else if encoder_name == "libx264" {
                // libx264 with customizable preset and profile
                opts.set("crf", &settings.quality_value.to_string());
                if let Some(ref preset) = settings.preset {
                    opts.set("preset", preset);
                }
                if let Some(ref profile) = settings.profile {
                    opts.set("profile", profile);
                }
            } else if encoder_name == "libx265" {
                // libx265 with customizable preset
                opts.set("crf", &settings.quality_value.to_string());
                if let Some(ref preset) = settings.preset {
                    opts.set("preset", preset);
                }
            } else if encoder_name == "h264_qsv" || encoder_name == "hevc_qsv" {
                // QSV uses global_quality
                opts.set("global_quality", &settings.quality_value.to_string());
            } else if encoder_name == "h264_amf" || encoder_name == "hevc_amf" {
                // AMD AMF rate control (CQP mode for quality-based encoding)
                opts.set("rc", "cqp");
                opts.set("qp", &settings.quality_value.to_string());
            } else if encoder_name == "h264_videotoolbox" || encoder_name == "hevc_videotoolbox" {
                // VideoToolbox doesn't support CRF well, map to bitrate
                // CRF 18 ≈ 10Mbps, CRF 23 ≈ 5Mbps, CRF 28 ≈ 2.5Mbps
                let bitrate_kbps = if settings.quality_value <= 18 {
                    10000
                } else if settings.quality_value <= 23 {
                    5000
                } else {
                    2500
                };
                encoder.set_bit_rate(bitrate_kbps * 1000);
            } else if encoder_name == "av1_nvenc" {
                // NVENC AV1 uses -cq (constant quantizer) like H.264/H.265
                opts.set("rc", "constqp");
                opts.set("cq", &settings.quality_value.to_string()); // CQ 0-51
                if let Some(ref preset) = settings.preset {
                    opts.set("preset", preset); // p1-p7
                }
            } else if encoder_name == "av1_qsv" {
                // QSV AV1 uses global_quality
                opts.set("global_quality", &settings.quality_value.to_string());
            } else if encoder_name == "av1_amf" {
                // AMD AMF AV1 rate control (CQP mode)
                opts.set("rc", "cqp");
                opts.set("qp", &settings.quality_value.to_string());
            } else if encoder_name == "libsvtav1" {
                // SVT-AV1: CRF 0-63, preset 0-13 (0=slowest/best, 13=fastest)
                opts.set("crf", &settings.quality_value.to_string());
                if let Some(ref preset) = settings.preset {
                    opts.set("preset", preset); // 0-13
                }
            } else if encoder_name == "libaom-av1" {
                // libaom-av1: CRF 0-63, cpu-used 0-8 (0=slowest, 8=fastest)
                opts.set("crf", &settings.quality_value.to_string());
                if let Some(ref preset) = settings.preset {
                    opts.set("cpu-used", preset); // Map preset to cpu-used
                }
            } else if encoder_name == "prores_ks" {
                // ProRes profile from settings or default to Standard
                let profile = settings
                    .prores_profile
                    .as_ref()
                    .map(|p| p.to_ffmpeg_value())
                    .unwrap_or("2"); // Default to Standard (422)

                info!(
                    "ProRes encoding with profile {} ({:?})",
                    profile, settings.prores_profile
                );
                opts.set("profile", profile);
                opts.set("vendor", "apl0"); // Apple vendor ID for compatibility
            }
        }
        QualityMode::Bitrate => {
            // Bitrate mode
            encoder.set_bit_rate(settings.quality_value as usize * 1000); // Convert kbps to bps
        }
    }

    // Open encoder with options
    info!(
        "Opening encoder '{}' with pixel_format={:?}, size={}x{}",
        encoder_name,
        encoder.format(),
        width,
        height
    );
    let mut encoder = encoder.open_with(opts).map_err(|e| {
        EncodeError::OutputCreateFailed(format!("Failed to open encoder '{}': {}", encoder_name, e))
    })?;

    // Add stream and set parameters from encoder
    let mut ost = octx
        .add_stream(codec)
        .map_err(|e| EncodeError::OutputCreateFailed(format!("Failed to add stream: {}", e)))?;
    ost.set_parameters(&encoder);

    // Set stream time_base to match encoder (critical for proper timestamps)
    ost.set_time_base(encoder.time_base());

    // For HEVC/H.265 in MP4/MOV: set hvc1 tag for Apple compatibility (QuickTime, Safari)
    // Without this tag, HEVC videos may not play on macOS/iOS
    if settings.codec == VideoCodec::H265
        && matches!(settings.container, Container::MP4 | Container::MOV)
    {
        // Set codec tag via stream parameters
        unsafe {
            // FFmpeg codec tag for HEVC: fourcc 'hvc1'
            (*ost.parameters().as_mut_ptr()).codec_tag = u32::from_le_bytes(*b"hvc1");
        }
        info!("Set HEVC codec tag to 'hvc1' for Apple compatibility");
    }

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

    // Create reusable swscale context for RGB→YUV conversion
    let mut sws_ctx = if needs_yuv {
        Some(SwsContext::new_rgb_to_yuv(width, height)
            .map_err(|e| EncodeError::OutputCreateFailed(format!("Failed to create swscale context: {}", e)))?)
    } else {
        None
    };

    let mut pts = 0i64;

    #[allow(clippy::explicit_counter_loop)]
    for frame_idx in play_range.0..=play_range.1 {
        // Check for cancellation
        if cancel_flag.load(Ordering::Relaxed) {
            return Err(EncodeError::Cancelled);
        }

        // Get frame from cache
        let frame = cache.get_frame(frame_idx).ok_or_else(|| {
            EncodeError::EncodeFrameFailed(format!("Frame {} not in cache", frame_idx))
        })?;

        // Ensure frame is loaded (skip Placeholder frames - they're already in memory)
        if frame.status() == FrameStatus::Header {
            frame.load().map_err(|e| {
                EncodeError::EncodeFrameFailed(format!("Failed to load frame {}: {}", frame_idx, e))
            })?;
        }

        // STEP 1: Crop to target dimensions if needed (handles mixed resolutions)
        let (frame_width, frame_height) = frame.resolution();
        if frame_width != width as usize || frame_height != height as usize {
            info!(
                "Cropping frame {} from {}x{} to {}x{}",
                frame_idx, frame_width, frame_height, width, height
            );
            frame.crop(width as usize, height as usize, CropAlign::Center);
        }

        // STEP 2: Convert RGBA8 → RGB24 (using trait method)
        let rgb24_data = frame.to_rgb24().map_err(|e| {
            EncodeError::EncodeFrameFailed(format!("Frame {} RGBA→RGB24 conversion failed: {}", frame_idx, e))
        })?;

        // STEP 3: Convert to FFmpeg frame (RGB24 or YUV420P depending on encoder)
        let mut ffmpeg_frame = if needs_yuv {
            // Convert RGB24 → YUV420P using reusable swscale context
            sws_ctx.as_mut().unwrap()
                .convert_rgb24_to_yuv420p(&rgb24_data, width, height)
                .map_err(|e| {
                    EncodeError::EncodeFrameFailed(format!("RGB→YUV conversion failed: {}", e))
                })?
        } else {
            // Use RGB24 directly for libx264/libx265
            let mut ffmpeg_frame =
                ffmpeg::util::frame::video::Video::new(ffmpeg::format::Pixel::RGB24, width, height);

            // Copy RGB24 data to FFmpeg frame
            let dst_stride = ffmpeg_frame.stride(0);
            let src_stride = (width * 3) as usize;

            {
                let dst_data = ffmpeg_frame.data_mut(0);
                for y in 0..height as usize {
                    let src_offset = y * src_stride;
                    let dst_offset = y * dst_stride;
                    dst_data[dst_offset..dst_offset + src_stride]
                        .copy_from_slice(&rgb24_data[src_offset..src_offset + src_stride]);
                }
            }

            ffmpeg_frame
        };
        // rgb24_data dropped here, memory freed

        // Set PTS (presentation timestamp)
        ffmpeg_frame.set_pts(Some(pts));
        pts += 1;

        // Send frame to encoder
        encoder.send_frame(&ffmpeg_frame).map_err(|e| {
            EncodeError::EncodeFrameFailed(format!("Failed to send frame {}: {}", frame_idx, e))
        })?;

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

            encoded.write_interleaved(&mut octx).map_err(|e| {
                EncodeError::EncodeFrameFailed(format!("Failed to write packet: {}", e))
            })?;
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
    encoder
        .send_eof()
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

        encoded.write_interleaved(&mut octx).map_err(|e| {
            EncodeError::EncodeFrameFailed(format!("Failed to write packet: {}", e))
        })?;
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

    info!(
        "Encoding complete: {} frames written to {:?}",
        total_frames, settings.output_path
    );
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
            "libx264",
            "h264_nvenc",
            "h264_qsv", // H.264
            "libx265",
            "hevc_nvenc",
            "hevc_qsv", // H.265
            "mpeg4",
            "libxvid", // MPEG-4
            "libvpx",
            "libvpx-vp9", // VP8/VP9
            "libaom-av1", // AV1
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
            panic!(
                "NO VIDEO ENCODERS FOUND - FFmpeg build has no encoding support! Skipping test."
            );
        }

        println!("\nUsing encoder: {}", found_encoder.unwrap());

        // Create cache with 100 placeholder frames
        let (mut cache, _ui_rx) = Cache::new(0.1, None);

        // Create sequence with 100 placeholder frames (no files)
        // Placeholders are green RGBA [0,100,0,255] by default
        let frames: Vec<Frame> = (0..100).map(|_| Frame::new(640, 480)).collect();
        let seq = Sequence::from_frames(frames, "test_placeholder.*.rgb".to_string(), 640, 480);

        cache.append_seq(seq);

        // Set play range to encode only frames 10-49 (40 frames total)
        cache.set_play_range(10, 49);
        let (play_start, play_end) = cache.get_play_range();
        println!(
            "Play range set: {}..{} ({} frames)",
            play_start,
            play_end,
            play_end - play_start + 1
        );

        // Determine which codec to use based on available encoder
        let (codec, encoder_impl, encoder_name) =
            if ffmpeg::encoder::find_by_name("h264_nvenc").is_some() {
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
            quality_value: 2000, // 2 Mbps
        };

        // Create progress channel
        let (tx, rx) = std::sync::mpsc::channel();
        let cancel_flag = Arc::new(AtomicBool::new(false));

        // Run encoding
        let abs_path = std::fs::canonicalize(&output_path)
            .unwrap_or_else(|_| std::env::current_dir().unwrap().join(&output_path));
        println!(
            "Encoding frames {}..{} to: {}",
            play_start,
            play_end,
            abs_path.display()
        );
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
        let metadata =
            std::fs::metadata(&output_path).expect("Failed to read output file metadata");
        assert!(metadata.len() > 0, "Output file is empty");

        println!("✓ Encoding test passed!");
        println!("  Encoder: {}", encoder_name);
        println!("  Output: {}", abs_path.display());
        println!(
            "  Size: {} bytes ({:.2} KB)",
            metadata.len(),
            metadata.len() as f64 / 1024.0
        );

        // Verify progress reached completion
        if let Some(progress) = last_progress {
            assert_eq!(
                progress.stage,
                EncodeStage::Complete,
                "Encoding did not complete"
            );
            println!(
                "  Frames: {}/{} (play range: {}..{})",
                progress.current_frame, progress.total_frames, play_start, play_end
            );
            assert_eq!(
                progress.total_frames, 40,
                "Should encode exactly 40 frames from play range"
            );
        }

        // Cleanup
        // let _ = std::fs::remove_file(&output_path);
    }
}
