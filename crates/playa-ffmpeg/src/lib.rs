//! # playa-ffmpeg
//!
//! Safe Rust bindings for FFmpeg 8.0 with vcpkg integration for simplified cross-platform builds.
//!
//! This crate provides idiomatic Rust wrappers around FFmpeg's C libraries, enabling multimedia
//! processing including video/audio encoding, decoding, muxing, demuxing, filtering, and transcoding.
//!
//! ## Main Modules
//!
//! - [`codec`] - Audio/video/subtitle codecs (encoders and decoders)
//! - [`mod@format`] - Container formats, streams, input/output contexts
//! - [`util`] - Core utilities (frames, errors, color, channel layouts, dictionaries)
//! - [`filter`] - Audio/video filtering and transformation graphs
//! - [`software`] - Software scaling and resampling
//! - [`device`] - Hardware input/output devices
//!
//! ## Quick Start
//!
//! ```ignore
//! use playa_ffmpeg as ffmpeg;
//!
//! // Initialize FFmpeg (required before use)
//! ffmpeg::init()?;
//!
//! // Open input file
//! let input = ffmpeg::format::input(&"video.mp4")?;
//!
//! // Process streams...
//! ```
//!
//! ## Feature Flags
//!
//! - `codec` (default) - Enable codec support
//! - `format` (default) - Enable format/container support
//! - `filter` (default) - Enable filtering support
//! - `device` (default) - Enable device support
//! - `software-scaling` (default) - Enable software video scaling
//! - `software-resampling` (default) - Enable software audio resampling
//! - `static` - Link FFmpeg statically
//! - `build` - Build FFmpeg from source during compilation
//!
//! See `Cargo.toml` for additional codec-specific and licensing feature flags.

#![allow(non_camel_case_types)]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::module_inception)]
#![allow(clippy::too_many_arguments)]

#[macro_use]
extern crate bitflags;
pub extern crate ffmpeg_sys_next as sys;
#[cfg(feature = "image")]
extern crate image;
extern crate libc;

/// Re-export of the raw FFI bindings from `ffmpeg-sys-next`.
///
/// Use this for direct access to FFmpeg's C API when the safe wrappers don't provide
/// the needed functionality. Most users should prefer the safe wrappers in this crate.
pub use sys as ffi;

#[macro_use]
pub mod util;
pub use crate::util::{
    channel_layout::{self, ChannelLayout},
    chroma, color, dictionary,
    dictionary::{Mut as DictionaryMut, Owned as Dictionary, Ref as DictionaryRef},
    error::{self, Error},
    frame::{self, Frame},
    log,
    mathematics::{self, Rescale, Rounding, rescale},
    media, option, picture,
    rational::{self, Rational},
    time,
};

#[cfg(feature = "format")]
pub mod format;
#[cfg(feature = "format")]
pub use crate::format::chapter::{Chapter, ChapterMut};
#[cfg(feature = "format")]
pub use crate::format::format::Format;
#[cfg(feature = "format")]
pub use crate::format::stream::{Stream, StreamMut};

#[cfg(feature = "codec")]
pub mod codec;
#[cfg(feature = "codec")]
pub use crate::codec::audio_service::AudioService;
#[cfg(feature = "codec")]
pub use crate::codec::codec::Codec;
#[cfg(feature = "codec")]
pub use crate::codec::discard::Discard;
#[cfg(feature = "codec")]
pub use crate::codec::field_order::FieldOrder;
#[cfg(feature = "codec")]
pub use crate::codec::packet::{self, Packet};
#[cfg(all(feature = "codec", not(feature = "ffmpeg_5_0")))]
pub use crate::codec::picture::Picture;
#[cfg(feature = "codec")]
pub use crate::codec::subtitle::{self, Subtitle};
#[cfg(feature = "codec")]
pub use crate::codec::threading;
#[cfg(feature = "codec")]
pub use crate::codec::{decoder, encoder};

#[cfg(feature = "device")]
pub mod device;

#[cfg(feature = "filter")]
pub mod filter;
#[cfg(feature = "filter")]
pub use filter::Filter;

pub mod software;

/// Initializes the error handling subsystem.
///
/// Registers all FFmpeg error codes for proper error translation to Rust Error types.
/// Called automatically by [`init()`].
fn init_error() {
    util::error::register_all();
}

/// Initializes the format/container subsystem (FFmpeg < 5.0).
///
/// Registers all available muxers and demuxers. In FFmpeg 5.0+, this is handled
/// automatically and this function is a no-op.
#[cfg(all(feature = "format", not(feature = "ffmpeg_5_0")))]
fn init_format() {
    format::register_all();
}

#[cfg(not(feature = "format"))]
fn init_format() {}

/// Initializes the device input/output subsystem.
///
/// Registers all available input and output devices (cameras, screen capture, etc.).
/// Only active when the `device` feature is enabled.
#[cfg(feature = "device")]
fn init_device() {
    device::register_all();
}

#[cfg(not(feature = "device"))]
fn init_device() {}

/// Initializes the filter subsystem (FFmpeg < 5.0).
///
/// Registers all available audio/video filters. In FFmpeg 5.0+, this is handled
/// automatically and this function is a no-op.
#[cfg(all(feature = "filter", not(feature = "ffmpeg_5_0")))]
fn init_filter() {
    filter::register_all();
}

#[cfg(not(feature = "filter"))]
fn init_filter() {}

/// Initializes the FFmpeg library.
///
/// This function must be called before using any other FFmpeg functionality. It initializes
/// all subsystems including error handling, formats, devices, and filters.
///
/// # Note
///
/// - In FFmpeg 5.0+, most subsystems auto-register, but this call is still required for
///   error handling and device initialization.
/// - This function is thread-safe and can be called multiple times (subsequent calls are no-ops).
/// - The `ffmpeg4`/`ffmpeg41`/`ffmpeg42`/`ffmpeg43` feature flags are deprecated as version
///   detection is now automatic.
///
/// # Errors
///
/// Currently always returns `Ok(())`, but the return type is kept for future compatibility.
///
/// # Example
///
/// ```ignore
/// use playa_ffmpeg as ffmpeg;
///
/// fn main() -> Result<(), ffmpeg::Error> {
///     ffmpeg::init()?;
///
///     // Now safe to use FFmpeg functionality
///     let input = ffmpeg::format::input(&"video.mp4")?;
///     Ok(())
/// }
/// ```
#[cfg_attr(
    any(feature = "ffmpeg4", feature = "ffmpeg41", feature = "ffmpeg42"),
    deprecated(note = "features ffmpeg4/ffmpeg41/ffmpeg42/ffmpeg43 are now auto-detected \
        and will be removed in a future version")
)]
pub fn init() -> Result<(), Error> {
    init_error();
    #[cfg(not(feature = "ffmpeg_5_0"))]
    init_format();
    init_device();
    #[cfg(not(feature = "ffmpeg_5_0"))]
    init_filter();

    Ok(())
}
