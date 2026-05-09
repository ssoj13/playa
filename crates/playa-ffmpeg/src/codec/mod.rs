//! Audio, video, and subtitle codec support.
//!
//! This module provides encoding and decoding capabilities for various media codecs.
//! It wraps FFmpeg's `libavcodec` library with safe Rust interfaces.
//!
//! # Main Components
//!
//! - [`decoder`] - Decode compressed media to raw frames
//! - [`encoder`] - Encode raw frames to compressed media
//! - [`Context`] - Codec context managing encoder/decoder state
//! - [`packet::Packet`] - Compressed data packet (encoded media)
//! - [`Parameters`] - Codec parameters (resolution, bitrate, sample rate, etc.)
//! - [`Audio`] / [`Video`] - Type-specific codec information
//!
//! # Usage
//!
//! Decoders are typically created from an input stream's codec parameters,
//! while encoders are configured with desired output parameters before use.
//!
//! # Submodules
//!
//! - `packet` - Compressed media packets
//! - `subtitle` - Subtitle codec support
//! - `capabilities` - Codec capability flags
//! - `threading` - Multi-threaded encoding/decoding
//! - `profile` - Codec profiles (baseline, main, high, etc.)
//! - `compliance` - Standard compliance levels

pub mod flag;
pub use self::flag::Flags;

pub mod id;
pub use self::id::Id;

pub mod packet;

pub mod subtitle;

#[cfg(not(feature = "ffmpeg_5_0"))]
pub mod picture;

pub mod discard;

pub mod context;
pub use self::context::Context;

pub mod capabilities;
pub use self::capabilities::Capabilities;

pub mod codec;

pub mod parameters;
pub use self::parameters::Parameters;

pub mod video;
pub use self::video::Video;

pub mod audio;
pub use self::audio::Audio;

pub mod audio_service;
pub mod field_order;

pub mod compliance;
pub use self::compliance::Compliance;

pub mod debug;
pub use self::debug::Debug;

pub mod profile;
pub use self::profile::Profile;

pub mod threading;

pub mod decoder;
pub mod encoder;
pub mod traits;

use std::{ffi::CStr, str::from_utf8_unchecked};

use crate::ffi::*;

/// Returns the libavcodec version number.
///
/// The version is encoded as `(major << 16) | (minor << 8) | micro`.
/// Use this to check FFmpeg's codec library version at runtime.
pub fn version() -> u32 {
    unsafe { avcodec_version() }
}

/// Returns the libavcodec build configuration string.
///
/// This shows the compile-time configuration options used when building FFmpeg,
/// including enabled codecs, features, and compile flags.
pub fn configuration() -> &'static str {
    unsafe { from_utf8_unchecked(CStr::from_ptr(avcodec_configuration()).to_bytes()) }
}

/// Returns the libavcodec license string.
///
/// Typically "LGPL version 2.1 or later" unless FFmpeg was built with GPL-only
/// components, in which case it returns "GPL version 2 or later".
pub fn license() -> &'static str {
    unsafe { from_utf8_unchecked(CStr::from_ptr(avcodec_license()).to_bytes()) }
}
