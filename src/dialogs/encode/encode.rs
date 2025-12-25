//! Video encoding module
//!
//! Handles encoding sequences to video files using FFmpeg encoders.
//! Supports hardware acceleration (NVENC/QSV) with CPU fallback.
//! Entry points are called from encoding dialogs and tests; the code pulls frames
//! from `entities::Comp` (via `Project`) and streams them through FFmpeg. Data flow:
//! timeline/project -> `encode_comp` -> frame retrieval (Comp.get_frame) -> pixel
//! format conversion -> muxed video on disk.

#![allow(clippy::items_after_test_module)] // SwsContext etc. placed after tests for readability

use log::info;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

use crate::entities::Comp;
use crate::entities::frame::{CropAlign, FrameConversion, PixelFormat, TonemapMode};
use playa_ffmpeg as ffmpeg;

/// Export mode - video or image sequence
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ExportMode {
    #[default]
    Video,
    Sequence,
}

/// Encode dialog settings (persistent via AppSettings)
/// Contains all codec settings + dialog state
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EncodeDialogSettings {
    // Dialog state
    pub output_path: PathBuf,
    pub container: Container,
    pub fps: f32,
    pub selected_codec: VideoCodec,

    // HDR → LDR conversion settings
    #[serde(default)]
    pub tonemap_mode: TonemapMode,

    // Per-codec settings (all preserved when switching codecs)
    #[serde(default)]
    pub codec_settings: CodecSettings,
    
    // Export mode (Video or Sequence)
    #[serde(default)]
    pub export_mode: ExportMode,
    
    // Image sequence settings
    #[serde(default)]
    pub sequence_settings: SequenceSettings,
}

impl Default for EncodeDialogSettings {
    fn default() -> Self {
        Self {
            output_path: PathBuf::from("output.mp4"),
            container: Container::MP4,
            fps: 24.0,
            selected_codec: VideoCodec::H264,
            tonemap_mode: TonemapMode::default(),
            codec_settings: CodecSettings::default(),
            export_mode: ExportMode::Video,
            sequence_settings: SequenceSettings::default(),
        }
    }
}

/// Encoder input settings (transport DTO for encode_sequence)
///
/// This is a simple flat structure containing settings for ONE selected codec.
/// The UI uses EncodeDialogSettings which stores settings for ALL codecs (H.264/H.265/ProRes/AV1).
/// When starting encoding, build_encoder_settings() converts EncodeDialogSettings → EncoderSettings
/// by extracting only the settings for the currently selected codec.
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
    pub profile: Option<String>, // H.264/H.265 profile (e.g. "high", "main", "main10")
    #[serde(default)]
    pub prores_profile: Option<ProResProfile>, // ProRes profile

    // HDR → LDR conversion settings
    #[serde(default)]
    pub tonemap_mode: TonemapMode, // Tonemapping mode for HDR sources (when encoding 8-bit)
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
            profile: Some("high".to_string()), // H.264: "high", H.265: "main" or "main10"
            prores_profile: Some(ProResProfile::Standard),
            tonemap_mode: TonemapMode::default(), // ACES by default
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
    #[serde(default)]
    pub profile: String, // "main" (8-bit) or "main10" (10-bit)
}

impl Default for H265Settings {
    fn default() -> Self {
        Self {
            encoder_impl: EncoderImpl::Auto,
            quality_mode: QualityMode::CRF,
            quality_value: 28, // H.265 default is higher than H.264
            preset: "medium".to_string(),
            profile: "main".to_string(), // 8-bit by default
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
            preset: "p4".to_string(), // Default preset (p4=medium for NVENC, or use "6" for SVT-AV1)
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

    /// Get preferred container for this codec
    pub fn preferred_container(&self) -> Container {
        match self {
            VideoCodec::H264 => Container::MP4,
            VideoCodec::H265 => Container::MP4,
            VideoCodec::AV1 => Container::MP4,
            VideoCodec::ProRes => Container::MOV, // ProRes typically uses MOV
        }
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

// ============================================================================
// IMAGE SEQUENCE EXPORT
// ============================================================================

/// Image sequence format
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SequenceFormat {
    #[default]
    Exr,
    Png,
    Jpeg,
    Tiff,
    Tga,
}

impl SequenceFormat {
    pub fn all() -> &'static [SequenceFormat] {
        &[
            SequenceFormat::Exr,
            SequenceFormat::Png,
            SequenceFormat::Jpeg,
            SequenceFormat::Tiff,
            SequenceFormat::Tga,
        ]
    }

    pub fn extension(&self) -> &'static str {
        match self {
            SequenceFormat::Exr => "exr",
            SequenceFormat::Png => "png",
            SequenceFormat::Jpeg => "jpg",
            SequenceFormat::Tiff => "tiff",
            SequenceFormat::Tga => "tga",
        }
    }

    /// Whether format supports alpha channel
    pub fn supports_alpha(&self) -> bool {
        match self {
            SequenceFormat::Exr => true,
            SequenceFormat::Png => true,
            SequenceFormat::Jpeg => false,
            SequenceFormat::Tiff => true,
            SequenceFormat::Tga => true,
        }
    }

    /// Whether format supports HDR (no tonemapping needed)
    pub fn is_hdr(&self) -> bool {
        matches!(self, SequenceFormat::Exr)
    }
}

impl std::fmt::Display for SequenceFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SequenceFormat::Exr => write!(f, "EXR"),
            SequenceFormat::Png => write!(f, "PNG"),
            SequenceFormat::Jpeg => write!(f, "JPEG"),
            SequenceFormat::Tiff => write!(f, "TIFF"),
            SequenceFormat::Tga => write!(f, "TGA"),
        }
    }
}

/// Channel mode for export
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ChannelMode {
    Rgb,
    #[default]
    Rgba,
}

impl ChannelMode {
    pub fn all() -> &'static [ChannelMode] {
        &[ChannelMode::Rgb, ChannelMode::Rgba]
    }
}

impl std::fmt::Display for ChannelMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChannelMode::Rgb => write!(f, "RGB"),
            ChannelMode::Rgba => write!(f, "RGBA"),
        }
    }
}

/// Unified output bit depth for all formats
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum OutputBitDepth {
    #[default]
    U8,    // 8-bit unsigned
    U16,   // 16-bit unsigned
    F16,   // 16-bit float (half)
    F32,   // 32-bit float
}

impl OutputBitDepth {
    pub fn all() -> &'static [OutputBitDepth] {
        &[OutputBitDepth::U8, OutputBitDepth::U16, OutputBitDepth::F16, OutputBitDepth::F32]
    }
}

impl std::fmt::Display for OutputBitDepth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OutputBitDepth::U8 => write!(f, "8-bit"),
            OutputBitDepth::U16 => write!(f, "16-bit"),
            OutputBitDepth::F16 => write!(f, "Half (F16)"),
            OutputBitDepth::F32 => write!(f, "Float (F32)"),
        }
    }
}

/// Format capabilities - what each format supports
#[derive(Clone, Debug)]
pub struct FormatCapabilities {
    pub supported_depths: &'static [OutputBitDepth],
    pub supports_alpha: bool,
    pub is_hdr: bool,  // Can store values > 1.0 without tonemapping
}

impl SequenceFormat {
    /// Get capabilities for this format
    pub fn capabilities(&self) -> FormatCapabilities {
        match self {
            SequenceFormat::Exr => FormatCapabilities {
                supported_depths: &[OutputBitDepth::F16, OutputBitDepth::F32],
                supports_alpha: true,
                is_hdr: true,
            },
            SequenceFormat::Png => FormatCapabilities {
                supported_depths: &[OutputBitDepth::U8, OutputBitDepth::U16],
                supports_alpha: true,
                is_hdr: false,
            },
            SequenceFormat::Jpeg => FormatCapabilities {
                supported_depths: &[OutputBitDepth::U8],
                supports_alpha: false,
                is_hdr: false,
            },
            SequenceFormat::Tiff => FormatCapabilities {
                supported_depths: &[OutputBitDepth::U8, OutputBitDepth::U16],
                supports_alpha: true,
                is_hdr: false,
            },
            SequenceFormat::Tga => FormatCapabilities {
                supported_depths: &[OutputBitDepth::U8],
                supports_alpha: true,
                is_hdr: false,
            },
        }
    }
    
    /// Check if bit depth is supported by this format
    pub fn supports_depth(&self, depth: OutputBitDepth) -> bool {
        self.capabilities().supported_depths.contains(&depth)
    }
    
    /// Get default bit depth for this format
    pub fn default_depth(&self) -> OutputBitDepth {
        self.capabilities().supported_depths[0]
    }
    
    /// Validate and fix settings for this format
    pub fn validate_settings(&self, channels: &mut ChannelMode, depth: &mut OutputBitDepth) {
        let caps = self.capabilities();
        
        // Fix channels if alpha not supported
        if !caps.supports_alpha && *channels == ChannelMode::Rgba {
            *channels = ChannelMode::Rgb;
        }
        
        // Fix bit depth if not supported
        if !caps.supported_depths.contains(depth) {
            *depth = caps.supported_depths[0];
        }
    }
}

/// EXR compression mode
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ExrCompression {
    None,
    Rle,
    #[default]
    Zip,
    Piz,
}

impl ExrCompression {
    pub fn all() -> &'static [ExrCompression] {
        &[
            ExrCompression::None,
            ExrCompression::Rle,
            ExrCompression::Zip,
            ExrCompression::Piz,
        ]
    }
}

impl std::fmt::Display for ExrCompression {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExrCompression::None => write!(f, "None"),
            ExrCompression::Rle => write!(f, "RLE"),
            ExrCompression::Zip => write!(f, "ZIP"),
            ExrCompression::Piz => write!(f, "PIZ"),
        }
    }
}

/// EXR bit depth
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ExrBitDepth {
    #[default]
    Half,  // F16
    Float, // F32
}

impl ExrBitDepth {
    pub fn all() -> &'static [ExrBitDepth] {
        &[ExrBitDepth::Half, ExrBitDepth::Float]
    }
}

impl std::fmt::Display for ExrBitDepth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExrBitDepth::Half => write!(f, "Half (16-bit)"),
            ExrBitDepth::Float => write!(f, "Float (32-bit)"),
        }
    }
}

/// EXR sequence settings
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExrSequenceSettings {
    pub bit_depth: ExrBitDepth,
    pub compression: ExrCompression,
}

impl Default for ExrSequenceSettings {
    fn default() -> Self {
        Self {
            bit_depth: ExrBitDepth::Half,
            compression: ExrCompression::Zip,
        }
    }
}

/// PNG compression level (0-9)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PngSequenceSettings {
    pub compression: u8, // 0 (none) to 9 (max)
}

impl Default for PngSequenceSettings {
    fn default() -> Self {
        Self { compression: 6 }
    }
}

/// JPEG quality settings
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JpegSequenceSettings {
    pub quality: u8, // 1-100
}

impl Default for JpegSequenceSettings {
    fn default() -> Self {
        Self { quality: 90 }
    }
}

/// TIFF compression
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TiffCompression {
    None,
    #[default]
    Lzw,
    Zip,
    PackBits,
}

impl TiffCompression {
    pub fn all() -> &'static [TiffCompression] {
        &[
            TiffCompression::None,
            TiffCompression::Lzw,
            TiffCompression::Zip,
            TiffCompression::PackBits,
        ]
    }
}

impl std::fmt::Display for TiffCompression {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TiffCompression::None => write!(f, "None"),
            TiffCompression::Lzw => write!(f, "LZW"),
            TiffCompression::Zip => write!(f, "ZIP"),
            TiffCompression::PackBits => write!(f, "PackBits"),
        }
    }
}

/// TIFF bit depth
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TiffBitDepth {
    #[default]
    Eight,   // 8-bit
    Sixteen, // 16-bit
}

impl TiffBitDepth {
    pub fn all() -> &'static [TiffBitDepth] {
        &[TiffBitDepth::Eight, TiffBitDepth::Sixteen]
    }
}

impl std::fmt::Display for TiffBitDepth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TiffBitDepth::Eight => write!(f, "8-bit"),
            TiffBitDepth::Sixteen => write!(f, "16-bit"),
        }
    }
}

/// TIFF sequence settings
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TiffSequenceSettings {
    pub bit_depth: TiffBitDepth,
    pub compression: TiffCompression,
}

impl Default for TiffSequenceSettings {
    fn default() -> Self {
        Self {
            bit_depth: TiffBitDepth::Eight,
            compression: TiffCompression::Lzw,
        }
    }
}

/// TGA settings
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TgaSequenceSettings {
    pub rle_compression: bool,
}

impl Default for TgaSequenceSettings {
    fn default() -> Self {
        Self {
            rle_compression: true,
        }
    }
}

/// All sequence format settings
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SequenceFormatSettings {
    pub exr: ExrSequenceSettings,
    pub png: PngSequenceSettings,
    pub jpeg: JpegSequenceSettings,
    pub tiff: TiffSequenceSettings,
    pub tga: TgaSequenceSettings,
}

/// Sequence export settings
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SequenceSettings {
    pub format: SequenceFormat,
    pub channels: ChannelMode,
    pub bit_depth: OutputBitDepth,
    pub apply_tonemap: bool,
    pub tonemap_mode: TonemapMode,
    pub format_settings: SequenceFormatSettings,
}

impl Default for SequenceSettings {
    fn default() -> Self {
        Self {
            format: SequenceFormat::Exr,
            channels: ChannelMode::Rgba,
            bit_depth: OutputBitDepth::F16,  // Default for EXR
            apply_tonemap: false,
            tonemap_mode: TonemapMode::default(),
            format_settings: SequenceFormatSettings::default(),
        }
    }
}

impl SequenceSettings {
    /// Validate settings against format capabilities and fix if needed
    pub fn validate(&mut self) {
        self.format.validate_settings(&mut self.channels, &mut self.bit_depth);
    }
}

/// Padding pattern for frame numbering
#[derive(Clone, Debug, PartialEq)]
pub enum PaddingPattern {
    /// Printf-style: %04d -> 4 digits
    Printf { width: usize },
    /// Hash-style: #### -> 4 digits
    Hashes { count: usize },
    /// At-sign: @ -> no padding
    At,
    /// No pattern found
    None,
}

impl PaddingPattern {
    /// Format frame number according to pattern
    pub fn format(&self, frame: i32) -> String {
        match self {
            PaddingPattern::Printf { width } | PaddingPattern::Hashes { count: width } => {
                format!("{:0width$}", frame, width = *width)
            }
            PaddingPattern::At | PaddingPattern::None => {
                format!("{}", frame)
            }
        }
    }
}

/// Parse filename pattern and extract padding info
/// Returns (prefix, pattern, suffix)
/// Example: "render.####.exr" -> ("render.", Hashes{4}, ".exr")
pub fn parse_padding_pattern(filename: &str) -> (String, PaddingPattern, String) {
    // Try printf-style first: %0Nd or %Nd
    if let Some(pos) = filename.find('%') {
        let rest = &filename[pos + 1..];
        let mut chars = rest.chars().peekable();
        
        // Skip leading zero
        let has_zero = chars.peek() == Some(&'0');
        if has_zero {
            chars.next();
        }
        
        // Parse width
        let mut width_str = String::new();
        while let Some(&c) = chars.peek() {
            if c.is_ascii_digit() {
                width_str.push(c);
                chars.next();
            } else {
                break;
            }
        }
        
        // Check for 'd'
        if chars.next() == Some('d') {
            let width = width_str.parse::<usize>().unwrap_or(1);
            let prefix = filename[..pos].to_string();
            let consumed = 1 + if has_zero { 1 } else { 0 } + width_str.len() + 1; // % + 0? + digits + d
            let suffix = filename[pos + consumed..].to_string();
            return (prefix, PaddingPattern::Printf { width }, suffix);
        }
    }
    
    // Try hash-style: ####
    if let Some(start) = filename.find('#') {
        let mut count = 0;
        for c in filename[start..].chars() {
            if c == '#' {
                count += 1;
            } else {
                break;
            }
        }
        if count > 0 {
            let prefix = filename[..start].to_string();
            let suffix = filename[start + count..].to_string();
            return (prefix, PaddingPattern::Hashes { count }, suffix);
        }
    }
    
    // Try @-style
    if let Some(pos) = filename.find('@') {
        let prefix = filename[..pos].to_string();
        let suffix = filename[pos + 1..].to_string();
        return (prefix, PaddingPattern::At, suffix);
    }
    
    // No pattern - insert before extension
    if let Some(dot_pos) = filename.rfind('.') {
        let prefix = format!("{}.", &filename[..dot_pos]);
        let suffix = filename[dot_pos..].to_string();
        (prefix, PaddingPattern::None, suffix)
    } else {
        (format!("{}.", filename), PaddingPattern::None, String::new())
    }
}

/// Build frame path from pattern
pub fn build_frame_path(
    base_dir: &std::path::Path,
    prefix: &str,
    pattern: &PaddingPattern,
    suffix: &str,
    frame: i32,
) -> PathBuf {
    let filename = format!("{}{}{}", prefix, pattern.format(frame), suffix);
    base_dir.join(filename)
}

/// Update filename extension based on format
pub fn update_extension(path: &std::path::Path, format: SequenceFormat) -> PathBuf {
    let mut new_path = path.to_path_buf();
    new_path.set_extension(format.extension());
    new_path
}

// ============================================================================
// VIDEO ENCODING (existing code)
// ============================================================================

/// Progress updates during encoding
#[derive(Clone, Debug)]
pub struct EncodeProgress {
    pub current_frame: i32,
    pub total_frames: i32,
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
    EncoderNotFound,
    HardwareEncoderUnavailable,
    OutputCreateFailed(String),
    EncodeFrameFailed(String),
    Cancelled,
}

impl std::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
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
                info!("H.264: Selected h264_videotoolbox (Apple VideoToolbox)");
                return Ok("h264_videotoolbox");
            }

            if ffmpeg::encoder::find_by_name("h264_nvenc").is_some() {
                info!("H.264: Selected h264_nvenc (NVIDIA NVENC)");
                Ok("h264_nvenc")
            } else if ffmpeg::encoder::find_by_name("h264_qsv").is_some() {
                info!("H.264: Selected h264_qsv (Intel QuickSync)");
                Ok("h264_qsv")
            } else if ffmpeg::encoder::find_by_name("h264_amf").is_some() {
                info!("H.264: Selected h264_amf (AMD AMF)");
                Ok("h264_amf")
            } else if encoder_impl == EncoderImpl::Auto {
                info!("H.264: Selected libx264 (Software, fallback)");
                Ok("libx264") // Fallback to software
            } else {
                Err(EncodeError::HardwareEncoderUnavailable)
            }
        }
        (VideoCodec::H264, EncoderImpl::Software) => {
            info!("H.264: Selected libx264 (Software)");
            Ok("libx264")
        }

        // H.265 encoders
        (VideoCodec::H265, EncoderImpl::Hardware) | (VideoCodec::H265, EncoderImpl::Auto) => {
            // Priority: VideoToolbox (macOS) > NVENC (NVIDIA) > QSV (Intel) > AMF (AMD) > Software
            #[cfg(target_os = "macos")]
            if ffmpeg::encoder::find_by_name("hevc_videotoolbox").is_some() {
                info!("H.265: Selected hevc_videotoolbox (Apple VideoToolbox)");
                return Ok("hevc_videotoolbox");
            }

            if ffmpeg::encoder::find_by_name("hevc_nvenc").is_some() {
                info!("H.265: Selected hevc_nvenc (NVIDIA NVENC)");
                Ok("hevc_nvenc")
            } else if ffmpeg::encoder::find_by_name("hevc_qsv").is_some() {
                info!("H.265: Selected hevc_qsv (Intel QuickSync)");
                Ok("hevc_qsv")
            } else if ffmpeg::encoder::find_by_name("hevc_amf").is_some() {
                info!("H.265: Selected hevc_amf (AMD AMF)");
                Ok("hevc_amf")
            } else if encoder_impl == EncoderImpl::Auto {
                info!("H.265: Selected libx265 (Software, fallback)");
                Ok("libx265") // Fallback to software
            } else {
                Err(EncodeError::HardwareEncoderUnavailable)
            }
        }
        (VideoCodec::H265, EncoderImpl::Software) => {
            info!("H.265: Selected libx265 (Software)");
            Ok("libx265")
        }

        // AV1 encoders
        (VideoCodec::AV1, EncoderImpl::Hardware) | (VideoCodec::AV1, EncoderImpl::Auto) => {
            // Priority: NVENC (RTX 40xx) > QSV (Arc) > AMF (RDNA 3) > SVT-AV1 (software)
            if ffmpeg::encoder::find_by_name("av1_nvenc").is_some() {
                info!("AV1: Selected av1_nvenc (NVIDIA NVENC, RTX 40xx+)");
                Ok("av1_nvenc")
            } else if ffmpeg::encoder::find_by_name("av1_qsv").is_some() {
                info!("AV1: Selected av1_qsv (Intel QuickSync, Arc+)");
                Ok("av1_qsv")
            } else if ffmpeg::encoder::find_by_name("av1_amf").is_some() {
                info!("AV1: Selected av1_amf (AMD AMF, RDNA 3+)");
                Ok("av1_amf")
            } else if encoder_impl == EncoderImpl::Auto {
                // Fallback to software: SVT-AV1 (faster) > libaom (better quality)
                if ffmpeg::encoder::find_by_name("libsvtav1").is_some() {
                    info!("AV1: Selected libsvtav1 (Software, fast)");
                    Ok("libsvtav1")
                } else {
                    info!("AV1: Selected libaom-av1 (Software, high quality)");
                    Ok("libaom-av1")
                }
            } else {
                Err(EncodeError::HardwareEncoderUnavailable)
            }
        }
        (VideoCodec::AV1, EncoderImpl::Software) => {
            // Software fallback: prefer SVT-AV1 for speed
            if ffmpeg::encoder::find_by_name("libsvtav1").is_some() {
                info!("AV1: Selected libsvtav1 (Software)");
                Ok("libsvtav1")
            } else {
                info!("AV1: Selected libaom-av1 (Software)");
                Ok("libaom-av1")
            }
        }

        // ProRes (software only)
        (VideoCodec::ProRes, _) => {
            info!("ProRes: Selected prores_ks (Software, Apple ProRes)");
            Ok("prores_ks")
        }
    }
}

/// Main encoding function (legacy cache-based)
///
/// Encodes sequence from cache play_range to output file.
/// Runs in separate thread, sends progress updates via channel.
pub fn encode_sequence_from_comp(
    comp: &Comp,
    project: &crate::entities::Project,
    settings: &EncoderSettings,
    progress_tx: Sender<EncodeProgress>,
    cancel_flag: Arc<AtomicBool>,
) -> Result<(), EncodeError> {
    let start_time = std::time::Instant::now();
    info!(
        "========== encode_sequence() ENTERED at {:?} ==========",
        start_time
    );

    // Get play range from Comp
    let play_range = comp.play_range(true);
    let total_frames = play_range.1.saturating_sub(play_range.0) + 1;

    info!(
        "Play range: {:?}, total frames: {}",
        play_range, total_frames
    );
    info!(
        "Starting encode: {} frames ({}..{}) to {:?}",
        total_frames, play_range.0, play_range.1, settings.output_path
    );

    // Stage 1: Get target dimensions from first frame
    if progress_tx.send(EncodeProgress {
        current_frame: 0,
        total_frames,
        stage: EncodeStage::Validating,
    }).is_err() {
        return Err(EncodeError::Cancelled); // UI closed
    }

    // Get first frame to determine target dimensions
    let first_frame = comp.get_frame(play_range.0, project, true).ok_or_else(|| {
        EncodeError::EncodeFrameFailed(format!("First frame {} not available", play_range.0))
    })?;

    let (width, height) = first_frame.resolution();
    let (width, height) = (width as u32, height as u32);
    info!(
        "Using first frame dimensions as target: {}x{}",
        width, height
    );

    // Check for cancellation
    if cancel_flag.load(Ordering::Relaxed) {
        return Err(EncodeError::Cancelled);
    }

    // Stage 2: Create encoder
    if progress_tx.send(EncodeProgress {
        current_frame: 0,
        total_frames,
        stage: EncodeStage::Opening,
    }).is_err() {
        return Err(EncodeError::Cancelled);
    }

    // Initialize FFmpeg (suppress logging)
    unsafe {
        ffmpeg::ffi::av_log_set_level(ffmpeg::ffi::AV_LOG_QUIET);
    }

    // Create output muxer (container format inferred from output path extension)
    let mut octx = ffmpeg::format::output(&settings.output_path)
        .map_err(|e| EncodeError::OutputCreateFailed(e.to_string()))?;

    // Find encoder by name (hardware with fallback or software)
    let encoder_name = get_encoder_name(settings.codec, settings.encoder_impl)?;
    info!("Looking for encoder: {}", encoder_name);

    info!(
        "[{:?}] Looking for encoder '{}'...",
        start_time.elapsed(),
        encoder_name
    );
    let codec = ffmpeg::encoder::find_by_name(encoder_name).ok_or_else(|| {
        info!("Encoder '{}' not found", encoder_name);
        EncodeError::EncoderNotFound
    })?;

    info!(
        "[{:?}] Using encoder: {} for codec {:?}",
        start_time.elapsed(),
        encoder_name,
        settings.codec
    );

    // Create encoder context
    info!("[{:?}] Creating encoder context...", start_time.elapsed());
    let mut encoder = ffmpeg::codec::context::Context::new_with_codec(codec)
        .encoder()
        .video()
        .map_err(|e| EncodeError::OutputCreateFailed(format!("Failed to create encoder: {}", e)))?;

    encoder.set_width(width);
    encoder.set_height(height);

    // Determine pixel format based on encoder
    // Hardware encoders (NVENC, QSV, AMF), AV1, and ProRes need YUV
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
            | "prores_ks"
    );

    // Determine pixel format based on encoder and profile
    let pixel_format = if encoder_name == "prores_ks" {
        // ProRes always uses YUV422P10 (10-bit 4:2:2)
        ffmpeg::format::Pixel::YUV422P10LE
    } else if encoder_name == "libx265"
        || encoder_name == "hevc_nvenc"
        || encoder_name == "hevc_qsv"
        || encoder_name == "hevc_amf"
        || encoder_name == "hevc_videotoolbox"
    {
        // HEVC: check profile for 10-bit (main10)
        let hevc_10bit = settings
            .profile
            .as_ref()
            .map(|p| p == "main10")
            .unwrap_or(false);

        if hevc_10bit {
            ffmpeg::format::Pixel::YUV420P10LE // 10-bit 4:2:0
        } else {
            ffmpeg::format::Pixel::YUV420P // 8-bit 4:2:0
        }
    } else if needs_yuv {
        ffmpeg::format::Pixel::YUV420P // 8-bit 4:2:0 for other YUV encoders
    } else {
        ffmpeg::format::Pixel::RGB24 // libx264 can use RGB24 directly
    };

    encoder.set_format(pixel_format);
    let fps_num = settings.fps as i32;
    encoder.set_frame_rate(Some(ffmpeg::util::rational::Rational::new(fps_num, 1)));
    encoder.set_time_base(ffmpeg::util::rational::Rational::new(1, fps_num));

    // Set GOP size (keyframe interval) for seekability
    // GOP = 10 seconds (fps * 10) ensures keyframes for timeline scrubbing
    let gop_size = (fps_num * 10).max(1);
    encoder.set_gop(gop_size as u32);

    // Set quality parameters
    let mut opts = ffmpeg::Dictionary::new();
    match settings.quality_mode {
        QualityMode::CRF => {
            // CRF mode (quality-based)
            if encoder_name == "h264_nvenc" || encoder_name == "hevc_nvenc" {
                // NVENC uses -cq (constant quantizer) instead of -crf
                opts.set("rc", "constqp"); // Rate control mode
                opts.set("cq", &settings.quality_value.to_string()); // Quality (0-51, lower is better)
                if let Some(ref preset) = settings.preset
                    && !preset.is_empty()
                {
                    opts.set("preset", preset); // NVENC preset (p1-p7)
                }
                // Force regular keyframes for seekability
                opts.set("forced-idr", "1"); // Force IDR frames at GOP boundaries
                opts.set("no-scenecut", "1"); // Disable scene change detection (consistent GOP)
            } else if encoder_name == "libx264" {
                // libx264 with customizable preset and profile
                opts.set("crf", &settings.quality_value.to_string());
                if let Some(ref preset) = settings.preset
                    && !preset.is_empty()
                {
                    opts.set("preset", preset);
                }
                if let Some(ref profile) = settings.profile {
                    opts.set("profile", profile);
                }
                // Force keyframes for seekability
                opts.set("keyint", &gop_size.to_string()); // Maximum GOP size
                opts.set("sc_threshold", "0"); // Disable scene change detection
            } else if encoder_name == "libx265" {
                // libx265 with customizable preset
                opts.set("crf", &settings.quality_value.to_string());
                if let Some(ref preset) = settings.preset
                    && !preset.is_empty()
                {
                    opts.set("preset", preset);
                }
                // Force keyframes for seekability
                opts.set("keyint", &gop_size.to_string()); // Maximum GOP size
                opts.set("scenecut", "0"); // Disable scene change detection

                // Set profile (main or main10)
                if let Some(ref profile) = settings.profile
                    && !profile.is_empty()
                {
                    opts.set("profile", profile); // "main" (8-bit) or "main10" (10-bit)
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
                // NVENC AV1: use qp (not cq) for constqp mode
                opts.set("rc", "constqp");
                opts.set("qp", &settings.quality_value.to_string()); // QP 0-255
                if let Some(ref preset) = settings.preset
                    && !preset.is_empty()
                {
                    opts.set("preset", preset); // 0-18 or named presets
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
                if let Some(ref preset) = settings.preset
                    && !preset.is_empty()
                {
                    opts.set("preset", preset); // 0-13
                }
            } else if encoder_name == "libaom-av1" {
                // libaom-av1: CRF 0-63, cpu-used 0-8 (0=slowest, 8=fastest)
                opts.set("crf", &settings.quality_value.to_string());
                if let Some(ref preset) = settings.preset
                    && !preset.is_empty()
                {
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
        "[{:?}] Opening encoder '{}' with pixel_format={:?}, size={}x{}",
        start_time.elapsed(),
        encoder_name,
        encoder.format(),
        width,
        height
    );

    // Log all encoder options for debugging
    info!("Encoder options:");
    for (key, value) in opts.iter() {
        info!("  {} = {}", key, value);
    }

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

    // Get stream time_base AFTER write_header (it may be adjusted by the muxer)
    let stream_tb = octx.stream(0).unwrap().time_base();
    let encoder_tb = encoder.time_base();

    info!(
        "Encoder initialized: {}x{} @ {} fps, quality mode: {:?}, time_base: encoder={:?} stream={:?}",
        width, height, settings.fps, settings.quality_mode, encoder_tb, stream_tb
    );

    // Check for cancellation
    if cancel_flag.load(Ordering::Relaxed) {
        return Err(EncodeError::Cancelled);
    }

    // Stage 3: Encoding loop
    if progress_tx.send(EncodeProgress {
        current_frame: 0,
        total_frames,
        stage: EncodeStage::Encoding,
    }).is_err() {
        return Err(EncodeError::Cancelled);
    }

    info!("Starting encoding loop for {} frames", total_frames);

    // Create reusable swscale context for RGB→YUV conversion
    let needs_10bit = pixel_format == ffmpeg::format::Pixel::YUV422P10LE
        || pixel_format == ffmpeg::format::Pixel::YUV420P10LE;

    let mut sws_ctx = if needs_yuv {
        let src_format = if needs_10bit {
            ffmpeg::format::Pixel::RGB48LE // 10-bit: RGB48LE → YUV10
        } else {
            ffmpeg::format::Pixel::RGB24 // 8-bit: RGB24 → YUV420P
        };
        info!(
            "Creating SwsContext for {:?} → {:?} conversion",
            src_format, pixel_format
        );
        Some(
            SwsContext::new(src_format, pixel_format, width, height).map_err(|e| {
                EncodeError::OutputCreateFailed(format!("Failed to create swscale context: {}", e))
            })?,
        )
    } else {
        info!("Using RGB24 directly (no YUV conversion)");
        None
    };

    let mut pts = 0i64;
    info!("Entering frame encoding loop...");

    #[allow(clippy::explicit_counter_loop)]
    for frame_idx in play_range.0..=play_range.1 {
        // Check for cancellation
        if cancel_flag.load(Ordering::Relaxed) {
            return Err(EncodeError::Cancelled);
        }

        if frame_idx % 10 == 0 {
            info!(
                "Processing frame {}/{}",
                frame_idx - play_range.0,
                total_frames
            );
        }

        // Get composed frame from Comp
        let frame = comp.get_frame(frame_idx, project, true).ok_or_else(|| {
            EncodeError::EncodeFrameFailed(format!("Frame {} not available in comp", frame_idx))
        })?;

        // STEP 1: Crop to target dimensions if needed (handles mixed resolutions)
        let (frame_width, frame_height) = frame.resolution();
        let frame_cropped = if frame_width != width as usize || frame_height != height as usize {
            info!(
                "Cropping frame {} from {}x{} to {}x{}",
                frame_idx, frame_width, frame_height, width, height
            );
            frame.crop_copy(width as usize, height as usize, CropAlign::Center)
        } else {
            frame.clone()
        };

        // Check for cancellation after crop
        if cancel_flag.load(Ordering::Relaxed) {
            return Err(EncodeError::Cancelled);
        }

        // Detect if source is HDR (F16/F32 pixel format)
        let source_is_hdr = matches!(
            frame_cropped.pixel_format(),
            PixelFormat::RgbaF16 | PixelFormat::RgbaF32
        );

        // STEP 2: Tonemap HDR → LDR if encoding 8-bit from HDR source
        let frame_for_encode = if !needs_10bit && source_is_hdr {
            // HDR → 8-bit: apply tonemapping
            info!(
                "Frame {}: Tonemapping {:?} → LDR using {:?}",
                frame_idx,
                frame_cropped.pixel_format(),
                settings.tonemap_mode
            );
            frame_cropped.tonemap(settings.tonemap_mode).map_err(|e| {
                EncodeError::EncodeFrameFailed(format!(
                    "Frame {} tonemapping failed: {}",
                    frame_idx, e
                ))
            })?
        } else {
            // No tonemapping needed (either 10-bit encoding or source is already LDR)
            frame_cropped
        };

        // Check for cancellation after tonemap
        if cancel_flag.load(Ordering::Relaxed) {
            return Err(EncodeError::Cancelled);
        }

        // STEP 3: Convert to RGB24 (8-bit) or RGB48 (10-bit)
        let mut ffmpeg_frame = if needs_10bit {
            // 10-bit path: RGBA → RGB48 (u16) → YUV10
            if frame_idx % 10 == 0 {
                info!("Frame {}: Converting RGBA → RGB48 (10-bit path)", frame_idx);
            }
            let rgb48_data = frame_for_encode.to_rgb48().map_err(|e| {
                EncodeError::EncodeFrameFailed(format!(
                    "Frame {} RGBA→RGB48 conversion failed: {}",
                    frame_idx, e
                ))
            })?;

            if frame_idx % 10 == 0 {
                info!(
                    "Frame {}: RGB48 conversion OK, calling swscale RGB48→YUV10",
                    frame_idx
                );
            }
            sws_ctx
                .as_mut()
                .unwrap()
                .convert_rgb48(&rgb48_data, width, height)
                .map_err(|e| {
                    EncodeError::EncodeFrameFailed(format!("RGB48→YUV10 conversion failed: {}", e))
                })?
        } else if needs_yuv {
            // 8-bit YUV path: RGBA8 → RGB24 → YUV420P
            let rgb24_data = frame_for_encode.to_rgb24().map_err(|e| {
                EncodeError::EncodeFrameFailed(format!(
                    "Frame {} RGBA→RGB24 conversion failed: {}",
                    frame_idx, e
                ))
            })?;

            sws_ctx
                .as_mut()
                .unwrap()
                .convert(&rgb24_data, width, height)
                .map_err(|e| {
                    EncodeError::EncodeFrameFailed(format!("RGB24→YUV conversion failed: {}", e))
                })?
        } else {
            // 8-bit RGB24 direct path (libx264/libx265)
            let rgb24_data = frame_for_encode.to_rgb24().map_err(|e| {
                EncodeError::EncodeFrameFailed(format!(
                    "Frame {} RGBA→RGB24 conversion failed: {}",
                    frame_idx, e
                ))
            })?;

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

            // Rescale packet timestamps from encoder time_base to stream time_base
            // This is CRITICAL for proper MP4 timeline and seeking
            encoded.rescale_ts(encoder_tb, stream_tb);

            // Set packet stream index
            encoded.set_stream(0);

            // Set packet duration (1 frame in time_base units)
            encoded.set_duration(1);

            // Ensure DTS is set (NVENC sometimes doesn't set it)
            let pts_val = encoded.pts();
            let dts_val = encoded.dts();

            if dts_val.is_none()
                && let Some(pts) = pts_val
            {
                encoded.set_dts(Some(pts));
            }

            // Debug: log first few packets
            if frame_idx - play_range.0 < 3 {
                info!(
                    "Packet {}: pts={:?}, dts={:?}, duration={}, keyframe={}, tb={:?}→{:?}",
                    frame_idx - play_range.0,
                    encoded.pts(),
                    encoded.dts(),
                    encoded.duration(),
                    encoded.is_key(),
                    encoder_tb,
                    stream_tb
                );
            }

            encoded.write_interleaved(&mut octx).map_err(|e| {
                EncodeError::EncodeFrameFailed(format!("Failed to write packet: {}", e))
            })?;
        }

        // Update progress
        let current_frame = frame_idx - play_range.0 + 1;
        if progress_tx.send(EncodeProgress {
            current_frame,
            total_frames,
            stage: EncodeStage::Encoding,
        }).is_err() {
            return Err(EncodeError::Cancelled);
        }

        if current_frame % 10 == 0 {
            info!("Encoded frame {}/{}", current_frame, total_frames);
        }
    }

    // Stage 4: Flush encoder
    if progress_tx.send(EncodeProgress {
        current_frame: total_frames,
        total_frames,
        stage: EncodeStage::Flushing,
    }).is_err() {
        return Err(EncodeError::Cancelled);
    }

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

        // Rescale packet timestamps from encoder time_base to stream time_base
        encoded.rescale_ts(encoder_tb, stream_tb);

        // Set packet stream index
        encoded.set_stream(0);

        // Set packet duration (1 frame in time_base units)
        encoded.set_duration(1);

        // Ensure DTS is set
        if encoded.dts().is_none()
            && let Some(pts) = encoded.pts()
        {
            encoded.set_dts(Some(pts));
        }

        encoded.write_interleaved(&mut octx).map_err(|e| {
            EncodeError::EncodeFrameFailed(format!("Failed to write packet: {}", e))
        })?;
    }

    info!("Flushed remaining packets");

    // Write container trailer (CRITICAL: without this, no moov atom = no timeline)
    info!("Writing trailer...");
    octx.write_trailer()
        .map_err(|e| EncodeError::OutputCreateFailed(format!("Failed to write trailer: {}", e)))?;
    info!("Trailer written successfully");

    // Stage 5: Complete (ignore send error - encoding is done anyway)
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

/// High-level encoding entry point: encodes a Comp.
///
/// Comp is the single source of truth for play range and fps.
pub fn encode_comp(
    comp: &Comp,
    project: &crate::entities::Project,
    settings: &EncoderSettings,
    progress_tx: Sender<EncodeProgress>,
    cancel_flag: Arc<AtomicBool>,
) -> Result<(), EncodeError> {
    encode_sequence_from_comp(comp, project, settings, progress_tx, cancel_flag)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::cache_man::CacheManager;

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

        // Note: This panic!() is TEST-ONLY - skips test if FFmpeg lacks encoders.
        // Bug hunt false positive: not production code (Plan2/Plan3).
        if found_encoder.is_none() {
            panic!(
                "NO VIDEO ENCODERS FOUND - FFmpeg build has no encoding support! Skipping test."
            );
        }

        println!("\nUsing encoder: {}", found_encoder.unwrap());

        // Define test play range
        let play_start = 0;
        let play_end = 9;

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
            } else if ffmpeg::encoder::find_by_name("libx265").is_some() {
                println!("\n🎬 Using libx265 encoder");
                (VideoCodec::H265, EncoderImpl::Software, "libx265")
            } else {
                println!("\n⚠ No compatible encoder available, skipping encoding test");
                println!("   Available: {}", found_encoder.unwrap());
                println!("   Need: libx264, h264_nvenc, or libx265");
                println!("\n✓ Test infrastructure verified:");
                println!("  - Placeholder frames created");
                println!("  - Encoder discovery working");
                return;
            };
        let mut encoder_used = encoder_name.to_string();

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
            fps: 24.0,
            preset: None,
            profile: None,
            prores_profile: None,
            tonemap_mode: TonemapMode::default(),
        };

        // Create progress channel
        let (tx, rx) = std::sync::mpsc::channel();
        let cancel_flag = Arc::new(AtomicBool::new(false));

        // Build CompNode for testing (produces placeholder frames)
        let mut comp = crate::entities::CompNode::new(
            "placeholder",
            play_start,
            play_end,
            settings.fps,
        );
        // Keep predictable dimensions for encoded video
        comp.attrs
            .set(crate::entities::keys::A_WIDTH, crate::entities::AttrValue::UInt(64));
        comp.attrs
            .set(crate::entities::keys::A_HEIGHT, crate::entities::AttrValue::UInt(64));
        let manager = Arc::new(CacheManager::new(0.75, 2.0));
        let project = crate::entities::project::Project::new(manager);

        // Run encoding
        println!(
            "Encoding frames {}..{} to: {}",
            play_start,
            play_end,
            output_path.display()
        );
        let mut final_output = output_path.clone();

        let result = encode_comp(&comp, &project, &settings, tx.clone(), cancel_flag.clone());

        // Retry with software fallback if hardware encoder refuses to open (driver/args issues)
        let result = match result {
            Ok(_) => Ok(()),
            Err(EncodeError::OutputCreateFailed(msg))
                if settings.encoder_impl == EncoderImpl::Hardware =>
            {
                println!(
                    "⚠ Hardware encoder failed to open ({}), retrying with alternatives.",
                    msg
                );

                // Try HEVC NVENC if available (keep hardware path first)
                if ffmpeg::encoder::find_by_name("hevc_nvenc").is_some() {
                    let mut hevc = settings.clone();
                    hevc.codec = VideoCodec::H265;
                    hevc.encoder_impl = EncoderImpl::Hardware;
                    hevc.output_path = output_path.with_file_name("test_encode_output_hevc.mp4");
                    let _ = std::fs::remove_file(&hevc.output_path);
                    if let Ok(()) =
                        encode_comp(&comp, &project, &hevc, tx.clone(), cancel_flag.clone())
                    {
                        println!("✓ HEVC NVENC fallback succeeded");
                        final_output = hevc.output_path.clone();
                        encoder_used = "hevc_nvenc".to_string();
                        Ok(())
                    } else {
                        println!(
                            "⚠ HEVC NVENC fallback failed, trying software libx264 if available"
                        );
                        // fall through to software attempt below
                        let mut sw = settings.clone();
                        sw.encoder_impl = EncoderImpl::Software;
                        sw.codec = VideoCodec::H264;
                        sw.output_path = output_path.with_file_name("test_encode_output_sw.mp4");
                        let _ = std::fs::remove_file(&sw.output_path);
                        final_output = sw.output_path.clone();
                        encoder_used = "libx264".to_string();
                        encode_comp(&comp, &project, &sw, tx, cancel_flag)
                    }
                } else if ffmpeg::encoder::find_by_name("libx264").is_some() {
                    let mut sw = settings.clone();
                    sw.encoder_impl = EncoderImpl::Software;
                    sw.codec = VideoCodec::H264;
                    sw.output_path = output_path.with_file_name("test_encode_output_sw.mp4");
                    let _ = std::fs::remove_file(&sw.output_path);
                    final_output = sw.output_path.clone();
                    encoder_used = "libx264".to_string();
                    encode_comp(&comp, &project, &sw, tx, cancel_flag)
                } else {
                    println!(
                        "⚠ No fallback encoder available (libx264 missing). Skipping encode test."
                    );
                    return;
                }
            }
            Err(EncodeError::EncoderNotFound) => {
                println!("⚠ Encoder not found in this environment. Skipping encode test.");
                return;
            }
            Err(e) => Err(e),
        };

        if let Err(EncodeError::EncoderNotFound) = &result {
            println!("⚠ Encoder not available after fallbacks. Skipping encode test.");
            return;
        }

        // Check progress updates
        let mut last_progress: Option<EncodeProgress> = None;
        while let Ok(progress) = rx.try_recv() {
            last_progress = Some(progress);
        }

        // Verify encoding succeeded
        assert!(result.is_ok(), "Encoding failed: {:?}", result);

        // Verify output file exists and is not empty
        assert!(final_output.exists(), "Output file was not created");
        let metadata =
            std::fs::metadata(&final_output).expect("Failed to read output file metadata");
        assert!(metadata.len() > 0, "Output file is empty");

        println!("✓ Encoding test passed!");
        println!("  Encoder: {}", encoder_used);
        let abs_path = std::fs::canonicalize(&final_output)
            .unwrap_or_else(|_| std::env::current_dir().unwrap().join(&final_output));
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
                progress.total_frames, 10,
                "Should encode exactly 10 frames from play range 0..9"
            );
        }

        // Cleanup
        let _ = std::fs::remove_file(&output_path);
    }
}

// ============================================================================
// IMAGE SEQUENCE EXPORT FUNCTIONS
// ============================================================================

use std::fs::File;
use std::io::BufWriter;

/// Write frame to EXR file using image crate (fallback when openexr feature disabled)
#[cfg(not(feature = "openexr"))]
fn write_exr_frame(
    frame: &crate::entities::Frame,
    path: &std::path::Path,
    settings: &ExrSequenceSettings,
    channels: ChannelMode,
    _bit_depth: OutputBitDepth, // Note: image crate EXR always uses F32
) -> Result<(), EncodeError> {
    use crate::entities::frame::PixelBuffer;
    use image::{Rgb, Rgba, ImageBuffer};
    
    let buffer = frame.buffer();
    let (width, height) = frame.resolution();
    
    // Convert to f32 for EXR
    match buffer.as_ref() {
        PixelBuffer::F32(data) => {
            match channels {
                ChannelMode::Rgba => {
                    let img: ImageBuffer<Rgba<f32>, Vec<f32>> = 
                        ImageBuffer::from_raw(width as u32, height as u32, data.clone())
                            .ok_or_else(|| EncodeError::EncodeFrameFailed("Failed to create RGBA32F buffer".into()))?;
                    img.save(path)
                        .map_err(|e| EncodeError::EncodeFrameFailed(format!("EXR save failed: {}", e)))?;
                }
                ChannelMode::Rgb => {
                    // Convert RGBA to RGB
                    let mut rgb_data = Vec::with_capacity(width * height * 3);
                    for chunk in data.chunks_exact(4) {
                        rgb_data.push(chunk[0]);
                        rgb_data.push(chunk[1]);
                        rgb_data.push(chunk[2]);
                    }
                    let img: ImageBuffer<Rgb<f32>, Vec<f32>> = 
                        ImageBuffer::from_raw(width as u32, height as u32, rgb_data)
                            .ok_or_else(|| EncodeError::EncodeFrameFailed("Failed to create RGB32F buffer".into()))?;
                    img.save(path)
                        .map_err(|e| EncodeError::EncodeFrameFailed(format!("EXR save failed: {}", e)))?;
                }
            }
        }
        PixelBuffer::F16(data) => {
            // Convert F16 to F32 for image crate (it doesn't support f16 directly)
            let f32_data: Vec<f32> = data.iter().map(|v| v.to_f32()).collect();
            match channels {
                ChannelMode::Rgba => {
                    let img: ImageBuffer<Rgba<f32>, Vec<f32>> = 
                        ImageBuffer::from_raw(width as u32, height as u32, f32_data)
                            .ok_or_else(|| EncodeError::EncodeFrameFailed("Failed to create RGBA32F buffer".into()))?;
                    img.save(path)
                        .map_err(|e| EncodeError::EncodeFrameFailed(format!("EXR save failed: {}", e)))?;
                }
                ChannelMode::Rgb => {
                    let mut rgb_data = Vec::with_capacity(width * height * 3);
                    for chunk in f32_data.chunks_exact(4) {
                        rgb_data.push(chunk[0]);
                        rgb_data.push(chunk[1]);
                        rgb_data.push(chunk[2]);
                    }
                    let img: ImageBuffer<Rgb<f32>, Vec<f32>> = 
                        ImageBuffer::from_raw(width as u32, height as u32, rgb_data)
                            .ok_or_else(|| EncodeError::EncodeFrameFailed("Failed to create RGB32F buffer".into()))?;
                    img.save(path)
                        .map_err(|e| EncodeError::EncodeFrameFailed(format!("EXR save failed: {}", e)))?;
                }
            }
        }
        PixelBuffer::U8(data) => {
            // Convert U8 to F32 (0-255 -> 0.0-1.0)
            let f32_data: Vec<f32> = data.iter().map(|&v| v as f32 / 255.0).collect();
            match channels {
                ChannelMode::Rgba => {
                    let img: ImageBuffer<Rgba<f32>, Vec<f32>> = 
                        ImageBuffer::from_raw(width as u32, height as u32, f32_data)
                            .ok_or_else(|| EncodeError::EncodeFrameFailed("Failed to create RGBA32F buffer".into()))?;
                    img.save(path)
                        .map_err(|e| EncodeError::EncodeFrameFailed(format!("EXR save failed: {}", e)))?;
                }
                ChannelMode::Rgb => {
                    let mut rgb_data = Vec::with_capacity(width * height * 3);
                    for chunk in f32_data.chunks_exact(4) {
                        rgb_data.push(chunk[0]);
                        rgb_data.push(chunk[1]);
                        rgb_data.push(chunk[2]);
                    }
                    let img: ImageBuffer<Rgb<f32>, Vec<f32>> = 
                        ImageBuffer::from_raw(width as u32, height as u32, rgb_data)
                            .ok_or_else(|| EncodeError::EncodeFrameFailed("Failed to create RGB32F buffer".into()))?;
                    img.save(path)
                        .map_err(|e| EncodeError::EncodeFrameFailed(format!("EXR save failed: {}", e)))?;
                }
            }
        }
    }
    
    let _ = settings; // TODO: Apply compression settings when image crate supports it
    Ok(())
}

/// Write frame to EXR file using openexr crate (when feature enabled)
#[cfg(feature = "openexr")]
fn write_exr_frame(
    frame: &crate::entities::Frame,
    path: &std::path::Path,
    settings: &ExrSequenceSettings,
    channels: ChannelMode,
    bit_depth: OutputBitDepth,
) -> Result<(), EncodeError> {
    use crate::entities::frame::PixelBuffer;
    use openexr::prelude::*;
    
    let buffer = frame.buffer();
    let (width, height) = frame.resolution();
    
    // Build header with dimensions
    let mut header = Header::from_dimensions(width as i32, height as i32);
    
    // Set compression
    let compression = match settings.compression {
        ExrCompression::None => Compression::None,
        ExrCompression::Rle => Compression::Rle,
        ExrCompression::Zip => Compression::Zip,
        ExrCompression::Piz => Compression::Piz,
    };
    header.set_compression(compression);
    
    // Map OutputBitDepth to EXR half/float (EXR only supports F16 and F32)
    let use_half = matches!(bit_depth, OutputBitDepth::F16 | OutputBitDepth::U8 | OutputBitDepth::U16);
    
    match buffer.as_ref() {
        PixelBuffer::F32(data) => {
            write_exr_f32_data(path, &header, data, width, height, channels, use_half)?;
        }
        PixelBuffer::F16(data) => {
            // Convert to f32 first
            let f32_data: Vec<f32> = data.iter().map(|v| v.to_f32()).collect();
            write_exr_f32_data(path, &header, &f32_data, width, height, channels, use_half)?;
        }
        PixelBuffer::U8(data) => {
            // Convert U8 to F32
            let f32_data: Vec<f32> = data.iter().map(|&v| v as f32 / 255.0).collect();
            write_exr_f32_data(path, &header, &f32_data, width, height, channels, use_half)?;
        }
    }
    
    Ok(())
}

#[cfg(feature = "openexr")]
fn write_exr_f32_data(
    path: &std::path::Path,
    header: &Header,
    data: &[f32],
    width: usize,
    height: usize,
    channels: ChannelMode,
    _use_half: bool, // RgbaOutputFile always uses half precision internally
) -> Result<(), EncodeError> {
    use openexr::prelude::*;
    
    // Convert interleaved RGBA f32 data to Vec<Rgba>
    // Rgba::from_f32 handles the f32 -> internal f16 conversion
    let rgba_pixels: Vec<Rgba> = data
        .chunks_exact(4)
        .map(|chunk| Rgba::from_f32(chunk[0], chunk[1], chunk[2], chunk[3]))
        .collect();
    
    // Create output file
    let rgba_channels = if channels == ChannelMode::Rgba {
        RgbaChannels::WriteRgba
    } else {
        RgbaChannels::WriteRgb
    };
    
    let mut file = RgbaOutputFile::new(path, header, rgba_channels, 1)
        .map_err(|e| EncodeError::EncodeFrameFailed(format!("Failed to create EXR file: {}", e)))?;
    
    // Set frame buffer: (data, x_stride=1, y_stride=width)
    file.set_frame_buffer(&rgba_pixels, 1, width)
        .map_err(|e| EncodeError::EncodeFrameFailed(format!("Failed to set EXR frame buffer: {}", e)))?;
    
    // Write pixels (requires unsafe as per openexr-rs API)
    unsafe {
        file.write_pixels(height as i32)
            .map_err(|e| EncodeError::EncodeFrameFailed(format!("Failed to write EXR pixels: {}", e)))?;
    }
    
    Ok(())
}

/// Write frame to PNG file
fn write_png_frame(
    frame: &crate::entities::Frame,
    path: &std::path::Path,
    settings: &PngSequenceSettings,
    channels: ChannelMode,
    bit_depth: OutputBitDepth,
) -> Result<(), EncodeError> {
    use crate::entities::frame::PixelBuffer;
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    use image::ImageEncoder;
    
    let buffer = frame.buffer();
    let (width, height) = frame.resolution();
    
    let file = File::create(path)
        .map_err(|e| EncodeError::OutputCreateFailed(format!("Failed to create PNG file: {}", e)))?;
    let writer = BufWriter::new(file);
    
    let compression = match settings.compression {
        0 => CompressionType::Fast,
        1..=3 => CompressionType::Fast,
        4..=6 => CompressionType::Default,
        _ => CompressionType::Best,
    };
    
    let encoder = PngEncoder::new_with_quality(writer, compression, FilterType::Adaptive);
    
    // PNG supports U8 and U16
    match bit_depth {
        OutputBitDepth::U8 => {
            // Get/convert to U8 data
            let rgba_data: Vec<u8> = match buffer.as_ref() {
                PixelBuffer::U8(data) => data.clone(),
                PixelBuffer::F16(data) => data.iter().map(|v| (v.to_f32().clamp(0.0, 1.0) * 255.0) as u8).collect(),
                PixelBuffer::F32(data) => data.iter().map(|&v| (v.clamp(0.0, 1.0) * 255.0) as u8).collect(),
            };
            
            match channels {
                ChannelMode::Rgba => {
                    encoder.write_image(&rgba_data, width as u32, height as u32, image::ExtendedColorType::Rgba8)
                        .map_err(|e| EncodeError::EncodeFrameFailed(format!("PNG encode failed: {}", e)))?;
                }
                ChannelMode::Rgb => {
                    let mut rgb_data = Vec::with_capacity(width * height * 3);
                    for chunk in rgba_data.chunks_exact(4) {
                        rgb_data.push(chunk[0]);
                        rgb_data.push(chunk[1]);
                        rgb_data.push(chunk[2]);
                    }
                    encoder.write_image(&rgb_data, width as u32, height as u32, image::ExtendedColorType::Rgb8)
                        .map_err(|e| EncodeError::EncodeFrameFailed(format!("PNG encode failed: {}", e)))?;
                }
            }
        }
        OutputBitDepth::U16 | OutputBitDepth::F16 | OutputBitDepth::F32 => {
            // Convert to U16 for PNG16
            let rgba16_data: Vec<u16> = match buffer.as_ref() {
                PixelBuffer::U8(data) => data.iter().map(|&v| (v as u16) * 257).collect(),
                PixelBuffer::F16(data) => data.iter().map(|v| (v.to_f32().clamp(0.0, 1.0) * 65535.0) as u16).collect(),
                PixelBuffer::F32(data) => data.iter().map(|&v| (v.clamp(0.0, 1.0) * 65535.0) as u16).collect(),
            };
            
            match channels {
                ChannelMode::Rgba => {
                    encoder.write_image(bytemuck::cast_slice(&rgba16_data), width as u32, height as u32, image::ExtendedColorType::Rgba16)
                        .map_err(|e| EncodeError::EncodeFrameFailed(format!("PNG16 encode failed: {}", e)))?;
                }
                ChannelMode::Rgb => {
                    let mut rgb_data: Vec<u16> = Vec::with_capacity(width * height * 3);
                    for chunk in rgba16_data.chunks_exact(4) {
                        rgb_data.push(chunk[0]);
                        rgb_data.push(chunk[1]);
                        rgb_data.push(chunk[2]);
                    }
                    encoder.write_image(bytemuck::cast_slice(&rgb_data), width as u32, height as u32, image::ExtendedColorType::Rgb16)
                        .map_err(|e| EncodeError::EncodeFrameFailed(format!("PNG16 encode failed: {}", e)))?;
                }
            }
        }
    }
    
    Ok(())
}

/// Write frame to JPEG file
fn write_jpeg_frame(
    frame: &crate::entities::Frame,
    path: &std::path::Path,
    settings: &JpegSequenceSettings,
) -> Result<(), EncodeError> {
    use crate::entities::frame::PixelBuffer;
    use image::codecs::jpeg::JpegEncoder;
    use image::ImageEncoder;
    
    let buffer = frame.buffer();
    let (width, height) = frame.resolution();
    
    // Get U8 data
    let rgba_data = match buffer.as_ref() {
        PixelBuffer::U8(data) => data.clone(),
        _ => return Err(EncodeError::EncodeFrameFailed(
            "JPEG requires U8 data. Apply tonemapping for HDR sources.".into()
        )),
    };
    
    // Convert RGBA to RGB (JPEG doesn't support alpha)
    let mut rgb_data = Vec::with_capacity(width * height * 3);
    for chunk in rgba_data.chunks_exact(4) {
        rgb_data.push(chunk[0]);
        rgb_data.push(chunk[1]);
        rgb_data.push(chunk[2]);
    }
    
    let file = File::create(path)
        .map_err(|e| EncodeError::OutputCreateFailed(format!("Failed to create JPEG file: {}", e)))?;
    let writer = BufWriter::new(file);
    
    let encoder = JpegEncoder::new_with_quality(writer, settings.quality);
    encoder.write_image(&rgb_data, width as u32, height as u32, image::ExtendedColorType::Rgb8)
        .map_err(|e| EncodeError::EncodeFrameFailed(format!("JPEG encode failed: {}", e)))?;
    
    Ok(())
}

/// Write frame to TIFF file
fn write_tiff_frame(
    frame: &crate::entities::Frame,
    path: &std::path::Path,
    settings: &TiffSequenceSettings,
    channels: ChannelMode,
    bit_depth: OutputBitDepth,
) -> Result<(), EncodeError> {
    use crate::entities::frame::PixelBuffer;
    use image::{ImageBuffer, Rgb, Rgba};
    
    let buffer = frame.buffer();
    let (width, height) = frame.resolution();
    
    // TIFF supports U8 and U16
    match bit_depth {
        OutputBitDepth::U8 => {
            // Get/convert to U8 data
            let rgba_data: Vec<u8> = match buffer.as_ref() {
                PixelBuffer::U8(data) => data.clone(),
                PixelBuffer::F16(data) => data.iter().map(|v| (v.to_f32().clamp(0.0, 1.0) * 255.0) as u8).collect(),
                PixelBuffer::F32(data) => data.iter().map(|&v| (v.clamp(0.0, 1.0) * 255.0) as u8).collect(),
            };
            
            match channels {
                ChannelMode::Rgba => {
                    let img: ImageBuffer<Rgba<u8>, Vec<u8>> = 
                        ImageBuffer::from_raw(width as u32, height as u32, rgba_data)
                            .ok_or_else(|| EncodeError::EncodeFrameFailed("Failed to create TIFF buffer".into()))?;
                    img.save(path)
                        .map_err(|e| EncodeError::EncodeFrameFailed(format!("TIFF save failed: {}", e)))?;
                }
                ChannelMode::Rgb => {
                    let mut rgb_data = Vec::with_capacity(width * height * 3);
                    for chunk in rgba_data.chunks_exact(4) {
                        rgb_data.push(chunk[0]);
                        rgb_data.push(chunk[1]);
                        rgb_data.push(chunk[2]);
                    }
                    let img: ImageBuffer<Rgb<u8>, Vec<u8>> = 
                        ImageBuffer::from_raw(width as u32, height as u32, rgb_data)
                            .ok_or_else(|| EncodeError::EncodeFrameFailed("Failed to create TIFF buffer".into()))?;
                    img.save(path)
                        .map_err(|e| EncodeError::EncodeFrameFailed(format!("TIFF save failed: {}", e)))?;
                }
            }
        }
        OutputBitDepth::U16 | OutputBitDepth::F16 | OutputBitDepth::F32 => {
            // Convert to U16 for TIFF16
            let rgba16_data: Vec<u16> = match buffer.as_ref() {
                PixelBuffer::U8(data) => data.iter().map(|&v| (v as u16) * 257).collect(),
                PixelBuffer::F16(data) => data.iter().map(|v| (v.to_f32().clamp(0.0, 1.0) * 65535.0) as u16).collect(),
                PixelBuffer::F32(data) => data.iter().map(|&v| (v.clamp(0.0, 1.0) * 65535.0) as u16).collect(),
            };
            
            match channels {
                ChannelMode::Rgba => {
                    let img: ImageBuffer<Rgba<u16>, Vec<u16>> = 
                        ImageBuffer::from_raw(width as u32, height as u32, rgba16_data)
                            .ok_or_else(|| EncodeError::EncodeFrameFailed("Failed to create TIFF16 buffer".into()))?;
                    img.save(path)
                        .map_err(|e| EncodeError::EncodeFrameFailed(format!("TIFF16 save failed: {}", e)))?;
                }
                ChannelMode::Rgb => {
                    let mut rgb_data = Vec::with_capacity(width * height * 3);
                    for chunk in rgba16_data.chunks_exact(4) {
                        rgb_data.push(chunk[0]);
                        rgb_data.push(chunk[1]);
                        rgb_data.push(chunk[2]);
                    }
                    let img: ImageBuffer<Rgb<u16>, Vec<u16>> = 
                        ImageBuffer::from_raw(width as u32, height as u32, rgb_data)
                            .ok_or_else(|| EncodeError::EncodeFrameFailed("Failed to create TIFF16 buffer".into()))?;
                    img.save(path)
                        .map_err(|e| EncodeError::EncodeFrameFailed(format!("TIFF16 save failed: {}", e)))?;
                }
            }
        }
    }
    
    let _ = settings.compression; // TODO: image crate doesn't expose TIFF compression settings easily
    Ok(())
}

/// Write frame to TGA file
fn write_tga_frame(
    frame: &crate::entities::Frame,
    path: &std::path::Path,
    _settings: &TgaSequenceSettings,
    channels: ChannelMode,
) -> Result<(), EncodeError> {
    use crate::entities::frame::PixelBuffer;
    use image::{ImageBuffer, Rgb, Rgba};
    
    let buffer = frame.buffer();
    let (width, height) = frame.resolution();
    
    let rgba_data = match buffer.as_ref() {
        PixelBuffer::U8(data) => data.clone(),
        _ => return Err(EncodeError::EncodeFrameFailed(
            "TGA requires U8 data. Apply tonemapping for HDR sources.".into()
        )),
    };
    
    match channels {
        ChannelMode::Rgba => {
            let img: ImageBuffer<Rgba<u8>, Vec<u8>> = 
                ImageBuffer::from_raw(width as u32, height as u32, rgba_data)
                    .ok_or_else(|| EncodeError::EncodeFrameFailed("Failed to create TGA buffer".into()))?;
            img.save(path)
                .map_err(|e| EncodeError::EncodeFrameFailed(format!("TGA save failed: {}", e)))?;
        }
        ChannelMode::Rgb => {
            let mut rgb_data = Vec::with_capacity(width * height * 3);
            for chunk in rgba_data.chunks_exact(4) {
                rgb_data.push(chunk[0]);
                rgb_data.push(chunk[1]);
                rgb_data.push(chunk[2]);
            }
            let img: ImageBuffer<Rgb<u8>, Vec<u8>> = 
                ImageBuffer::from_raw(width as u32, height as u32, rgb_data)
                    .ok_or_else(|| EncodeError::EncodeFrameFailed("Failed to create TGA buffer".into()))?;
            img.save(path)
                .map_err(|e| EncodeError::EncodeFrameFailed(format!("TGA save failed: {}", e)))?;
        }
    }
    
    // TODO: RLE compression when image crate supports it
    Ok(())
}

/// Main function to export image sequence
///
/// Exports frames from comp to individual image files.
/// Supports EXR, PNG, JPEG, TIFF, TGA formats.
pub fn encode_image_sequence(
    comp: &Comp,
    project: &crate::entities::Project,
    output_path: &std::path::Path,
    settings: &SequenceSettings,
    progress_tx: Sender<EncodeProgress>,
    cancel_flag: Arc<AtomicBool>,
) -> Result<(), EncodeError> {
    let start_time = std::time::Instant::now();
    info!(
        "========== encode_image_sequence() ENTERED at {:?} ==========",
        start_time
    );
    
    // Get play range from Comp
    let play_range = comp.play_range(true);
    let total_frames = (play_range.1.saturating_sub(play_range.0) + 1) as i32;
    
    info!(
        "Image sequence export: format={:?}, channels={:?}, frames={}",
        settings.format, settings.channels, total_frames
    );
    
    // Parse output pattern
    let filename = output_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("frame.####.exr");
    let base_dir = output_path.parent().unwrap_or(std::path::Path::new("."));
    
    // Ensure output directory exists
    if !base_dir.exists() {
        std::fs::create_dir_all(base_dir)
            .map_err(|e| EncodeError::OutputCreateFailed(format!("Failed to create output directory: {}", e)))?;
    }
    
    let (prefix, pattern, suffix) = parse_padding_pattern(filename);
    info!("Pattern parsed: prefix='{}', pattern={:?}, suffix='{}'", prefix, pattern, suffix);
    
    // Stage 1: Validating
    if progress_tx.send(EncodeProgress {
        current_frame: 0,
        total_frames,
        stage: EncodeStage::Validating,
    }).is_err() {
        return Err(EncodeError::Cancelled);
    }
    
    // Check for cancellation
    if cancel_flag.load(Ordering::Relaxed) {
        return Err(EncodeError::Cancelled);
    }
    
    // Stage 2: Encoding loop
    if progress_tx.send(EncodeProgress {
        current_frame: 0,
        total_frames,
        stage: EncodeStage::Encoding,
    }).is_err() {
        return Err(EncodeError::Cancelled);
    }
    
    for frame_idx in play_range.0..=play_range.1 {
        // Check for cancellation
        if cancel_flag.load(Ordering::Relaxed) {
            return Err(EncodeError::Cancelled);
        }
        
        let current_frame = (frame_idx - play_range.0 + 1) as i32;
        
        // Get frame from comp
        let frame = comp.get_frame(frame_idx, project, true).ok_or_else(|| {
            EncodeError::EncodeFrameFailed(format!("Frame {} not available", frame_idx))
        })?;
        
        // Apply tonemapping if needed (HDR -> LDR for non-EXR formats)
        let frame_to_write = if settings.apply_tonemap || (!settings.format.is_hdr() && frame.pixel_format() != PixelFormat::Rgba8) {
            frame.tonemap(settings.tonemap_mode).map_err(|e| {
                EncodeError::EncodeFrameFailed(format!("Tonemapping failed: {}", e))
            })?
        } else {
            frame.clone()
        };
        
        // Build output path for this frame
        let frame_path = build_frame_path(base_dir, &prefix, &pattern, &suffix, frame_idx);
        
        if frame_idx % 10 == 0 {
            info!("Writing frame {} -> {}", frame_idx, frame_path.display());
        }
        
        // Write frame based on format
        match settings.format {
            SequenceFormat::Exr => {
                write_exr_frame(&frame_to_write, &frame_path, &settings.format_settings.exr, settings.channels, settings.bit_depth)?;
            }
            SequenceFormat::Png => {
                write_png_frame(&frame_to_write, &frame_path, &settings.format_settings.png, settings.channels, settings.bit_depth)?;
            }
            SequenceFormat::Jpeg => {
                write_jpeg_frame(&frame_to_write, &frame_path, &settings.format_settings.jpeg)?;
            }
            SequenceFormat::Tiff => {
                write_tiff_frame(&frame_to_write, &frame_path, &settings.format_settings.tiff, settings.channels, settings.bit_depth)?;
            }
            SequenceFormat::Tga => {
                write_tga_frame(&frame_to_write, &frame_path, &settings.format_settings.tga, settings.channels)?;
            }
        }
        
        // Update progress
        if progress_tx.send(EncodeProgress {
            current_frame,
            total_frames,
            stage: EncodeStage::Encoding,
        }).is_err() {
            return Err(EncodeError::Cancelled);
        }
    }
    
    // Stage 3: Complete
    let _ = progress_tx.send(EncodeProgress {
        current_frame: total_frames,
        total_frames,
        stage: EncodeStage::Complete,
    });
    
    let elapsed = start_time.elapsed();
    info!(
        "Image sequence export complete: {} frames in {:.2}s ({:.1} fps)",
        total_frames,
        elapsed.as_secs_f64(),
        total_frames as f64 / elapsed.as_secs_f64()
    );
    
    Ok(())
}

// ============================================================================
// Frame format conversion utilities (SwsContext)
// ============================================================================

/// Reusable swscale context for efficient format conversions
///
/// Provides efficient FFmpeg swscale-based conversion between pixel formats.
/// Reuses swscale contexts to avoid expensive recreations.
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
        let mut src_frame = ffmpeg::util::frame::video::Video::new(self.src_format, width, height);

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
        let mut dst_frame = ffmpeg::util::frame::video::Video::new(self.dst_format, width, height);

        // Convert using swscale context
        let ctx = self.ctx.as_mut().ok_or("SwsContext not initialized")?;
        ctx.run(&src_frame, &mut dst_frame)
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
        let mut src_frame =
            ffmpeg::util::frame::video::Video::new(ffmpeg::format::Pixel::RGB48LE, width, height);

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
        let mut dst_frame = ffmpeg::util::frame::video::Video::new(self.dst_format, width, height);

        // Convert RGB48LE → YUV10 using swscale context
        let ctx = self.ctx.as_mut().ok_or("SwsContext not initialized")?;
        ctx.run(&src_frame, &mut dst_frame)
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
