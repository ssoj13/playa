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

/// Encoder settings (persistent via AppSettings)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EncoderSettings {
    pub output_path: PathBuf,
    pub container: Container,
    pub codec: VideoCodec,
    pub encoder_impl: EncoderImpl,
    pub quality_mode: QualityMode,
    pub quality_value: u32, // CRF 18-28 or bitrate in kbps
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
}

impl VideoCodec {
    pub fn all() -> &'static [VideoCodec] {
        &[VideoCodec::H264, VideoCodec::H265, VideoCodec::ProRes]
    }
}

impl std::fmt::Display for VideoCodec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VideoCodec::H264 => write!(f, "H.264"),
            VideoCodec::H265 => write!(f, "H.265 (HEVC)"),
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
    pub error: Option<String>,
}

/// Encoding stages
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EncodeStage {
    Validating,   // Checking frame sizes
    Opening,      // Creating encoder
    Encoding,     // Encoding frames
    Flushing,     // Flushing encoder
    Complete,     // Successfully finished
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
        error: None,
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
        error: None,
    });

    // TODO: Implement encoder creation
    // TODO: Implement encoding loop
    // TODO: Implement flushing

    // Stage 3: Complete
    let _ = progress_tx.send(EncodeProgress {
        current_frame: total_frames,
        total_frames,
        stage: EncodeStage::Complete,
        error: None,
    });

    info!("Encoding complete");
    Ok(())
}
