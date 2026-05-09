//! Container format support for muxing and demuxing.
//!
//! This module provides functionality for reading (demuxing) and writing (muxing) multimedia
//! container formats. It wraps FFmpeg's `libavformat` library with safe Rust interfaces.
//!
//! # Main Components
//!
//! - [`Context`] - Format context managing streams and container metadata
//! - [`stream`] - Individual media streams within a container
//! - [`chapter`] - Chapter/bookmark support for seekable formats
//! - [`mod@format`] - Container format information and discovery
//!
//! # Common Operations
//!
//! ## Opening Files for Reading
//!
//! Use [`input()`] to open a media file for reading:
//!
//! ```ignore
//! let mut input = ffmpeg::format::input(&"video.mp4")?;
//!
//! // Find best video stream
//! let stream = input.streams().best(Type::Video).unwrap();
//! let decoder = stream.codec().decoder().video()?;
//! ```
//!
//! ## Opening Files for Writing
//!
//! Use [`output()`] to create a new media file:
//!
//! ```ignore
//! let mut output = ffmpeg::format::output(&"output.mp4")?;
//!
//! // Add video stream
//! let mut stream = output.add_stream(encoder)?;
//! stream.set_parameters(&encoder);
//!
//! output.write_header()?;
//! // ... write packets ...
//! output.write_trailer()?;
//! ```

pub use crate::util::format::{Pixel, Sample, pixel, sample};
use crate::util::interrupt;

pub mod stream;

pub mod chapter;

pub mod context;
pub use self::context::Context;

pub mod format;
#[cfg(not(feature = "ffmpeg_5_0"))]
pub use self::format::list;
pub use self::format::{Flags, Input, Output, flag};

pub mod network;

use std::{
    ffi::{CStr, CString},
    path::Path,
    ptr,
    str::from_utf8_unchecked,
};

use crate::{Dictionary, Error, Format, ffi::*};

/// Registers all muxers and demuxers (FFmpeg < 5.0 only).
///
/// In FFmpeg 5.0+, formats are automatically registered and this is a no-op.
/// Called automatically by [`crate::init()`].
#[cfg(not(feature = "ffmpeg_5_0"))]
pub fn register_all() {
    unsafe {
        av_register_all();
    }
}

/// Registers a specific format (FFmpeg < 5.0 only).
///
/// In FFmpeg 5.0+, this is a no-op. Most users should rely on [`register_all()`]
/// or automatic registration instead of manually registering formats.
#[cfg(not(feature = "ffmpeg_5_0"))]
pub fn register(format: &Format) {
    match *format {
        Format::Input(ref format) => unsafe {
            av_register_input_format(format.as_ptr() as *mut _);
        },

        Format::Output(ref format) => unsafe {
            av_register_output_format(format.as_ptr() as *mut _);
        },
    }
}

/// Returns the libavformat version number.
///
/// The version is encoded as `(major << 16) | (minor << 8) | micro`.
pub fn version() -> u32 {
    unsafe { avformat_version() }
}

/// Returns the libavformat build configuration string.
///
/// Shows compile-time options used when building FFmpeg's format library.
pub fn configuration() -> &'static str {
    unsafe { from_utf8_unchecked(CStr::from_ptr(avformat_configuration()).to_bytes()) }
}

/// Returns the libavformat license string.
///
/// Typically "LGPL version 2.1 or later" unless built with GPL components.
pub fn license() -> &'static str {
    unsafe { from_utf8_unchecked(CStr::from_ptr(avformat_license()).to_bytes()) }
}

/// Converts a path to a C string for FFmpeg API calls.
///
/// # Panics
///
/// Panics if the path contains invalid UTF-8 or null bytes.
// XXX: use to_cstring when stable
fn from_path<P: AsRef<Path> + ?Sized>(path: &P) -> CString {
    CString::new(path.as_ref().as_os_str().to_str().unwrap()).unwrap()
}

/// Opens a file with a specific format (input or output).
///
/// Prefer [`input()`] or [`output()`] unless you need format override.
///
/// # Parameters
///
/// * `path` - File path to open
/// * `format` - Format to use (Input for reading, Output for writing)
///
/// # Errors
///
/// Returns an error if the file cannot be opened or the format is unsupported.
///
/// # Note
///
/// For input contexts, this automatically probes stream information after opening.
// NOTE: this will be better with specialization or anonymous return types
pub fn open<P: AsRef<Path> + ?Sized>(path: &P, format: &Format) -> Result<Context, Error> {
    unsafe {
        let mut ps = ptr::null_mut();
        let path = from_path(path);

        match *format {
            Format::Input(ref format) => match avformat_open_input(&mut ps, path.as_ptr(), format.as_ptr() as *mut _, ptr::null_mut()) {
                0 => match avformat_find_stream_info(ps, ptr::null_mut()) {
                    r if r >= 0 => Ok(Context::Input(context::Input::wrap(ps))),
                    e => Err(Error::from(e)),
                },

                e => Err(Error::from(e)),
            },

            Format::Output(ref format) => match avformat_alloc_output_context2(&mut ps, format.as_ptr() as *mut _, ptr::null(), path.as_ptr()) {
                0 => match avio_open(&mut (*ps).pb, path.as_ptr(), AVIO_FLAG_WRITE) {
                    0 => Ok(Context::Output(context::Output::wrap(ps))),
                    e => Err(Error::from(e)),
                },

                e => Err(Error::from(e)),
            },
        }
    }
}

/// Opens a file with a specific format and options dictionary.
///
/// Like [`open()`] but allows passing codec/format options.
///
/// # Parameters
///
/// * `path` - File path to open
/// * `format` - Format to use
/// * `options` - Dictionary of format-specific options
pub fn open_with<P: AsRef<Path> + ?Sized>(path: &P, format: &Format, options: Dictionary) -> Result<Context, Error> {
    unsafe {
        let mut ps = ptr::null_mut();
        let path = from_path(path);
        let mut opts = options.disown();

        match *format {
            Format::Input(ref format) => {
                let res = avformat_open_input(&mut ps, path.as_ptr(), format.as_ptr() as *mut _, &mut opts);

                Dictionary::own(opts);

                match res {
                    0 => match avformat_find_stream_info(ps, ptr::null_mut()) {
                        r if r >= 0 => Ok(Context::Input(context::Input::wrap(ps))),
                        e => Err(Error::from(e)),
                    },

                    e => Err(Error::from(e)),
                }
            }

            Format::Output(ref format) => match avformat_alloc_output_context2(&mut ps, format.as_ptr() as *mut _, ptr::null(), path.as_ptr()) {
                0 => match avio_open(&mut (*ps).pb, path.as_ptr(), AVIO_FLAG_WRITE) {
                    0 => Ok(Context::Output(context::Output::wrap(ps))),
                    e => Err(Error::from(e)),
                },

                e => Err(Error::from(e)),
            },
        }
    }
}

/// Opens a media file for reading (demuxing).
///
/// This is the primary function for opening input files. It automatically detects the
/// container format and probes all streams to gather codec information.
///
/// # Parameters
///
/// * `path` - Path to the media file (supports various protocols: file://, http://, rtsp://, etc.)
///
/// # Returns
///
/// An [`context::Input`] that can be used to access streams and read packets.
///
/// # Errors
///
/// - File not found or inaccessible
/// - Unsupported or corrupted format
/// - Permission denied
///
/// # Example
///
/// ```ignore
/// let mut input = ffmpeg::format::input(&"video.mp4")?;
///
/// // Find the best video stream
/// let stream = input.streams().best(Type::Video).ok_or(Error::StreamNotFound)?;
/// let stream_index = stream.index();
///
/// // Create decoder for this stream
/// let decoder = stream.codec().decoder().video()?;
/// ```
pub fn input<P: AsRef<Path> + ?Sized>(path: &P) -> Result<context::Input, Error> {
    unsafe {
        let mut ps = ptr::null_mut();
        let path = from_path(path);

        match avformat_open_input(&mut ps, path.as_ptr(), ptr::null_mut(), ptr::null_mut()) {
            0 => match avformat_find_stream_info(ps, ptr::null_mut()) {
                r if r >= 0 => Ok(context::Input::wrap(ps)),
                e => {
                    avformat_close_input(&mut ps);
                    Err(Error::from(e))
                }
            },

            e => Err(Error::from(e)),
        }
    }
}

/// Opens a media file for reading with options dictionary.
///
/// Like [`input()`] but allows passing format-specific options (e.g., timeouts,
/// buffer sizes, protocol options).
///
/// # Parameters
///
/// * `path` - Path to the media file
/// * `options` - Dictionary of format/protocol options
pub fn input_with_dictionary<P: AsRef<Path> + ?Sized>(path: &P, options: Dictionary) -> Result<context::Input, Error> {
    unsafe {
        let mut ps = ptr::null_mut();
        let path = from_path(path);
        let mut opts = options.disown();
        let res = avformat_open_input(&mut ps, path.as_ptr(), ptr::null_mut(), &mut opts);

        Dictionary::own(opts);

        match res {
            0 => match avformat_find_stream_info(ps, ptr::null_mut()) {
                r if r >= 0 => Ok(context::Input::wrap(ps)),
                e => {
                    avformat_close_input(&mut ps);
                    Err(Error::from(e))
                }
            },

            e => Err(Error::from(e)),
        }
    }
}

/// Opens a media file for reading with interrupt callback.
///
/// Allows cancellation of long-running operations (network streams, slow I/O).
/// The callback is called periodically; returning `true` aborts the operation.
///
/// # Parameters
///
/// * `path` - Path to the media file
/// * `closure` - Callback invoked periodically, return `true` to abort
///
/// # Example
///
/// ```ignore
/// use std::sync::atomic::{AtomicBool, Ordering};
/// use std::sync::Arc;
///
/// let should_abort = Arc::new(AtomicBool::new(false));
/// let abort_flag = should_abort.clone();
///
/// let input = ffmpeg::format::input_with_interrupt(&"http://stream.example.com/live", move || {
///     abort_flag.load(Ordering::Relaxed)
/// })?;
/// ```
pub fn input_with_interrupt<P: AsRef<Path> + ?Sized, F>(path: &P, closure: F) -> Result<context::Input, Error>
where
    F: FnMut() -> bool,
{
    unsafe {
        let mut ps = avformat_alloc_context();
        let path = from_path(path);
        // Set interrupt callback for cancellation support
        (*ps).interrupt_callback = interrupt::new(Box::new(closure)).interrupt;

        match avformat_open_input(&mut ps, path.as_ptr(), ptr::null_mut(), ptr::null_mut()) {
            0 => match avformat_find_stream_info(ps, ptr::null_mut()) {
                r if r >= 0 => Ok(context::Input::wrap(ps)),
                e => {
                    avformat_close_input(&mut ps);
                    Err(Error::from(e))
                }
            },

            e => Err(Error::from(e)),
        }
    }
}

/// Opens a media file for writing (muxing).
///
/// Creates a new output file with format auto-detected from the file extension.
/// The file is created/truncated and ready for writing after adding streams.
///
/// # Parameters
///
/// * `path` - Path to the output file
///
/// # Returns
///
/// An [`context::Output`] that can be used to add streams and write packets.
///
/// # Errors
///
/// - File cannot be created (permission denied, invalid path)
/// - Format cannot be determined from extension
/// - Unsupported format
///
/// # Example
///
/// ```ignore
/// let mut output = ffmpeg::format::output(&"output.mp4")?;
///
/// // Add video stream
/// let mut stream = output.add_stream(encoder)?;
/// stream.set_parameters(&encoder);
///
/// // Write header
/// output.write_header()?;
///
/// // Write packets...
/// output.write_packet(&packet)?;
///
/// // Finalize
/// output.write_trailer()?;
/// ```
pub fn output<P: AsRef<Path> + ?Sized>(path: &P) -> Result<context::Output, Error> {
    unsafe {
        let mut ps = ptr::null_mut();
        let path = from_path(path);

        match avformat_alloc_output_context2(&mut ps, ptr::null_mut(), ptr::null(), path.as_ptr()) {
            0 => match avio_open(&mut (*ps).pb, path.as_ptr(), AVIO_FLAG_WRITE) {
                0 => Ok(context::Output::wrap(ps)),
                e => Err(Error::from(e)),
            },

            e => Err(Error::from(e)),
        }
    }
}

/// Opens a media file for writing with options dictionary.
///
/// Like [`output()`] but allows passing I/O and format options.
///
/// # Parameters
///
/// * `path` - Path to the output file
/// * `options` - Dictionary of I/O and format options
pub fn output_with<P: AsRef<Path> + ?Sized>(path: &P, options: Dictionary) -> Result<context::Output, Error> {
    unsafe {
        let mut ps = ptr::null_mut();
        let path = from_path(path);
        let mut opts = options.disown();

        match avformat_alloc_output_context2(&mut ps, ptr::null_mut(), ptr::null(), path.as_ptr()) {
            0 => {
                let res = avio_open2(&mut (*ps).pb, path.as_ptr(), AVIO_FLAG_WRITE, ptr::null(), &mut opts);

                Dictionary::own(opts);

                match res {
                    0 => Ok(context::Output::wrap(ps)),
                    e => Err(Error::from(e)),
                }
            }

            e => Err(Error::from(e)),
        }
    }
}

/// Opens a media file for writing with explicit format specification.
///
/// Use this when the file extension doesn't match the desired format
/// or when writing to streams/pipes.
///
/// # Parameters
///
/// * `path` - Path to the output file
/// * `format` - Format name (e.g., "mp4", "matroska", "mpeg")
///
/// # Example
///
/// ```ignore
/// // Create MP4 file with .bin extension
/// let output = ffmpeg::format::output_as(&"stream.bin", "mp4")?;
/// ```
pub fn output_as<P: AsRef<Path> + ?Sized>(path: &P, format: &str) -> Result<context::Output, Error> {
    unsafe {
        let mut ps = ptr::null_mut();
        let path = from_path(path);
        let format = CString::new(format).unwrap();

        match avformat_alloc_output_context2(&mut ps, ptr::null_mut(), format.as_ptr(), path.as_ptr()) {
            0 => match avio_open(&mut (*ps).pb, path.as_ptr(), AVIO_FLAG_WRITE) {
                0 => Ok(context::Output::wrap(ps)),
                e => Err(Error::from(e)),
            },

            e => Err(Error::from(e)),
        }
    }
}

/// Opens a media file for writing with explicit format and options.
///
/// Combines [`output_as()`] with options dictionary support.
///
/// # Parameters
///
/// * `path` - Path to the output file
/// * `format` - Format name
/// * `options` - Dictionary of I/O and format options
pub fn output_as_with<P: AsRef<Path> + ?Sized>(path: &P, format: &str, options: Dictionary) -> Result<context::Output, Error> {
    unsafe {
        let mut ps = ptr::null_mut();
        let path = from_path(path);
        let format = CString::new(format).unwrap();
        let mut opts = options.disown();

        match avformat_alloc_output_context2(&mut ps, ptr::null_mut(), format.as_ptr(), path.as_ptr()) {
            0 => {
                let res = avio_open2(&mut (*ps).pb, path.as_ptr(), AVIO_FLAG_WRITE, ptr::null(), &mut opts);

                Dictionary::own(opts);

                match res {
                    0 => Ok(context::Output::wrap(ps)),
                    e => Err(Error::from(e)),
                }
            }

            e => Err(Error::from(e)),
        }
    }
}
