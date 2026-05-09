//! Audio and video filtering.
//!
//! This module provides filtering capabilities for audio and video streams.
//! Filters can modify, analyze, or transform media frames (scaling, cropping,
//! color correction, audio mixing, etc.).
//!
//! It wraps FFmpeg's `libavfilter` library.
//!
//! # Main Components
//!
//! - [`Graph`] - Filter graph connecting multiple filters
//! - [`Filter`] - Individual filter definition (scale, crop, overlay, etc.)
//! - [`Context`] - Instance of a filter within a graph
//! - [`Pad`] - Input/output connection point on a filter
//!
//! # Usage
//!
//! Filters are organized into graphs. Create a graph, add filters, link them together,
//! then push frames through the graph to process them.
//!
//! Common use cases include video scaling, format conversion, overlay composition,
//! and audio mixing.

pub mod flag;
pub use self::flag::Flags;

pub mod pad;
pub use self::pad::Pad;

pub mod filter;
pub use self::filter::Filter;

pub mod context;
pub use self::context::{Context, Sink, Source};

pub mod graph;
pub use self::graph::Graph;

use std::{
    ffi::{CStr, CString},
    str::from_utf8_unchecked,
};

#[cfg(not(feature = "ffmpeg_5_0"))]
use crate::Error;
use crate::ffi::*;

/// Registers all available filters (FFmpeg < 5.0 only).
///
/// In FFmpeg 5.0+, filters are automatically registered.
/// Called automatically by [`crate::init()`].
#[cfg(not(feature = "ffmpeg_5_0"))]
pub fn register_all() {
    unsafe {
        avfilter_register_all();
    }
}

/// Registers a specific filter (FFmpeg < 5.0 only).
///
/// Most users should rely on [`register_all()`] or automatic registration.
#[cfg(not(feature = "ffmpeg_5_0"))]
pub fn register(filter: &Filter) -> Result<(), Error> {
    unsafe {
        match avfilter_register(filter.as_ptr() as *mut _) {
            0 => Ok(()),
            _ => Err(Error::InvalidData),
        }
    }
}

/// Returns the libavfilter version number.
pub fn version() -> u32 {
    unsafe { avfilter_version() }
}

/// Returns the libavfilter build configuration string.
pub fn configuration() -> &'static str {
    unsafe { from_utf8_unchecked(CStr::from_ptr(avfilter_configuration()).to_bytes()) }
}

/// Returns the libavfilter license string.
pub fn license() -> &'static str {
    unsafe { from_utf8_unchecked(CStr::from_ptr(avfilter_license()).to_bytes()) }
}

/// Finds a filter by name.
///
/// Returns `None` if the filter doesn't exist.
///
/// # Examples
///
/// Common filter names: `"scale"`, `"crop"`, `"overlay"`, `"fps"`, `"format"`,
/// `"aresample"`, `"volume"`, `"concat"`.
pub fn find(name: &str) -> Option<Filter> {
    unsafe {
        let name = CString::new(name).unwrap();
        let ptr = avfilter_get_by_name(name.as_ptr());

        if ptr.is_null() { None } else { Some(Filter::wrap(ptr as *mut _)) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paditer() {
        #[cfg(not(feature = "ffmpeg_5_0"))]
        register_all();
        assert_eq!(find("overlay").unwrap().inputs().unwrap().map(|input| input.name().unwrap().to_string()).collect::<Vec<_>>(), vec!("main", "overlay"));
    }
}
