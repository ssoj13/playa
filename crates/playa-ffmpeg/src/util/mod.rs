//! Core utilities and data structures.
//!
//! This module provides fundamental types and utilities used throughout the library.
//! It wraps FFmpeg's `libavutil` library.
//!
//! # Main Components
//!
//! - [`frame`] - Raw audio/video frames (decoded data)
//! - [`error`] - Error types and error handling
//! - [`dictionary`] - Key-value metadata and options
//! - [`rational`] - Rational number representation for timestamps/framerates
//! - [`channel_layout`] - Audio channel layouts (stereo, 5.1, etc.)
//! - [`color`] - Color space and color primaries
//! - [`mod@format`] - Pixel and sample formats
//! - [`mathematics`] - Mathematical utilities (rescaling, rounding)
//! - [`time`] - Time representation and conversion
//! - [`mod@log`] - Logging configuration and levels

#[macro_use]
pub mod dictionary;
pub mod chroma;
pub mod color;
pub mod error;
pub mod format;
pub mod frame;
pub mod interrupt;
pub mod log;
pub mod mathematics;
pub mod media;
pub mod option;
pub mod picture;
pub mod range;
pub mod rational;
pub mod time;

#[cfg_attr(feature = "ffmpeg_7_0", path = "channel_layout.rs")]
#[cfg_attr(not(feature = "ffmpeg_7_0"), path = "legacy_channel_layout.rs")]
pub mod channel_layout;

use std::{ffi::CStr, str::from_utf8_unchecked};

use crate::ffi::*;

/// Returns the libavutil version number.
///
/// The version is encoded as `(major << 16) | (minor << 8) | micro`.
#[inline(always)]
pub fn version() -> u32 {
    unsafe { avutil_version() }
}

/// Returns the libavutil build configuration string.
///
/// Shows compile-time options used when building FFmpeg's utility library.
#[inline(always)]
pub fn configuration() -> &'static str {
    unsafe { from_utf8_unchecked(CStr::from_ptr(avutil_configuration()).to_bytes()) }
}

/// Returns the libavutil license string.
///
/// Typically "LGPL version 2.1 or later" unless built with GPL components.
#[inline(always)]
pub fn license() -> &'static str {
    unsafe { from_utf8_unchecked(CStr::from_ptr(avutil_license()).to_bytes()) }
}
