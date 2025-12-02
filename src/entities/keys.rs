//! Attribute key constants for Attrs access.
//!
//! Avoid string typos, enable IDE autocomplete.
//! Usage: `comp.attrs.get_i32(A_FRAME)`

/// Current playback frame position
pub const A_FRAME: &str = "frame";
/// In-point (start frame)
pub const A_IN: &str = "in";
/// Out-point (end frame)
pub const A_OUT: &str = "out";
/// Trim in-point (work area start)
pub const A_TRIM_IN: &str = "trim_in";
/// Trim out-point (work area end)
pub const A_TRIM_OUT: &str = "trim_out";
/// Frames per second
pub const A_FPS: &str = "fps";
/// Human-readable name
pub const A_NAME: &str = "name";
/// Source comp UUID (for child layers)
pub const A_SOURCE: &str = "source_uuid";
/// Entity UUID
pub const A_UUID: &str = "uuid";
