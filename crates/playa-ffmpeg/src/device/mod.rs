//! Hardware device input and output.
//!
//! This module provides access to hardware capture and playback devices.
//! It wraps FFmpeg's `libavdevice` library.
//!
//! # Capabilities
//!
//! - Video capture (webcams, screen capture, video capture cards)
//! - Audio capture (microphones, line-in)
//! - Video/audio playback to hardware devices
//! - Device enumeration and discovery
//!
//! # Submodules
//!
//! - [`input`] - Input devices (capture)
//! - [`output`] - Output devices (playback)
//! - [`extensions`] - Device-specific extensions
//!
//! # Platform Support
//!
//! Available devices vary by platform:
//! - **Linux**: v4l2 (video), alsa (audio), xcbgrab (screen capture)
//! - **Windows**: dshow (DirectShow), gdigrab (screen capture)
//! - **macOS**: avfoundation (video/audio), screencapture

pub mod extensions;
pub mod input;
pub mod output;

use std::{ffi::CStr, marker::PhantomData, str::from_utf8_unchecked};

use crate::ffi::*;

/// Information about a hardware device.
///
/// Provides device name and human-readable description for enumerated devices.
pub struct Info<'a> {
    ptr: *mut AVDeviceInfo,

    _marker: PhantomData<&'a ()>,
}

impl<'a> Info<'a> {
    /// Wraps a raw FFmpeg device info pointer.
    pub unsafe fn wrap(ptr: *mut AVDeviceInfo) -> Self {
        Info { ptr, _marker: PhantomData }
    }

    /// Returns the raw pointer.
    pub unsafe fn as_ptr(&self) -> *const AVDeviceInfo {
        self.ptr as *const _
    }

    /// Returns the mutable raw pointer.
    pub unsafe fn as_mut_ptr(&mut self) -> *mut AVDeviceInfo {
        self.ptr
    }
}

impl<'a> Info<'a> {
    /// Returns the device name.
    ///
    /// This is typically the system identifier for the device (e.g., "/dev/video0", "video=0").
    pub fn name(&self) -> &str {
        unsafe { from_utf8_unchecked(CStr::from_ptr((*self.as_ptr()).device_name).to_bytes()) }
    }

    /// Returns a human-readable device description.
    ///
    /// This is a user-friendly name (e.g., "HD Webcam", "Built-in Microphone").
    pub fn description(&self) -> &str {
        unsafe { from_utf8_unchecked(CStr::from_ptr((*self.as_ptr()).device_description).to_bytes()) }
    }
}

/// Registers all available devices.
///
/// Must be called before using device functionality. Called automatically by [`crate::init()`].
pub fn register_all() {
    unsafe {
        avdevice_register_all();
    }
}

/// Returns the libavdevice version number.
pub fn version() -> u32 {
    unsafe { avdevice_version() }
}

/// Returns the libavdevice build configuration string.
pub fn configuration() -> &'static str {
    unsafe { from_utf8_unchecked(CStr::from_ptr(avdevice_configuration()).to_bytes()) }
}

/// Returns the libavdevice license string.
pub fn license() -> &'static str {
    unsafe { from_utf8_unchecked(CStr::from_ptr(avdevice_license()).to_bytes()) }
}
