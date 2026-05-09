//! Software scaling and resampling.
//!
//! This module provides CPU-based media processing for format conversion and resizing.
//! It wraps FFmpeg's `libswscale` (video scaling) and `libswresample` (audio resampling).
//!
//! # Scaling (Video)
//!
//! The [`scaling`] module provides video frame scaling (resizing) and pixel format conversion.
//! Common uses include:
//! - Resizing video to different resolutions
//! - Converting between pixel formats (YUV ↔ RGB, different bit depths)
//! - Aspect ratio changes
//!
//! # Resampling (Audio)
//!
//! The [`resampling`] module provides audio sample rate conversion and format changes.
//! Common uses include:
//! - Changing sample rate (e.g., 48kHz → 44.1kHz)
//! - Converting sample formats (s16 → f32, planar ↔ packed)
//! - Channel layout conversion (stereo → 5.1)

#[cfg(feature = "software-scaling")]
pub mod scaling;

/// Creates a video scaler for resizing frames.
///
/// Convenience function for creating a scaling context that changes resolution
/// but preserves pixel format.
///
/// # Parameters
///
/// * `format` - Pixel format (same for input and output)
/// * `flags` - Scaling algorithm flags (quality vs. speed)
/// * `(in_width, in_height)` - Input dimensions
/// * `(out_width, out_height)` - Output dimensions
#[cfg(feature = "software-scaling")]
#[inline]
pub fn scaler(format: crate::format::Pixel, flags: scaling::Flags, (in_width, in_height): (u32, u32), (out_width, out_height): (u32, u32)) -> Result<scaling::Context, crate::Error> {
    scaling::Context::get(format, in_width, in_height, format, out_width, out_height, flags)
}

/// Creates a pixel format converter.
///
/// Convenience function for converting between pixel formats without changing resolution.
/// Uses fast bilinear scaling (though no actual scaling occurs).
///
/// # Parameters
///
/// * `(width, height)` - Frame dimensions (unchanged)
/// * `input` - Input pixel format
/// * `output` - Output pixel format
#[cfg(feature = "software-scaling")]
#[inline]
pub fn converter((width, height): (u32, u32), input: crate::format::Pixel, output: crate::format::Pixel) -> Result<scaling::Context, crate::Error> {
    scaling::Context::get(input, width, height, output, width, height, scaling::flag::Flags::FAST_BILINEAR)
}

#[cfg(feature = "software-resampling")]
pub mod resampling;

/// Creates an audio resampler.
///
/// Convenience function for creating a resampling context that handles sample rate,
/// format, and channel layout conversion.
///
/// # Parameters
///
/// * `(in_format, in_layout, in_rate)` - Input sample format, channel layout, and sample rate
/// * `(out_format, out_layout, out_rate)` - Output sample format, channel layout, and sample rate
#[cfg(feature = "software-resampling")]
#[inline]
pub fn resampler((in_format, in_layout, in_rate): (crate::util::format::Sample, crate::ChannelLayout, u32), (out_format, out_layout, out_rate): (crate::util::format::Sample, crate::ChannelLayout, u32)) -> Result<resampling::Context, crate::Error> {
    resampling::Context::get(in_format, in_layout, in_rate, out_format, out_layout, out_rate)
}
