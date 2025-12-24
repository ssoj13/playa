//! Attribute key constants for Attrs access.
//!
//! Avoid string typos, enable IDE autocomplete.
//! Usage: `comp.attrs.get_i32(A_FRAME)`

// === Mode constants (i8) ===
/// Normal comp mode - compose children
pub const COMP_NORMAL: i8 = 0;
/// File mode - load frames from disk
pub const COMP_FILE: i8 = 1;

// === Identity ===
/// Entity UUID
pub const A_UUID: &str = "uuid";
/// Human-readable name
pub const A_NAME: &str = "name";
/// Comp mode (COMP_NORMAL or COMP_FILE)
pub const A_MODE: &str = "mode";

// === Timeline ===
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
/// Current playback frame position
pub const A_FRAME: &str = "frame";

// === Compose flags ===
/// Solo flag - only render this layer
pub const A_SOLO: &str = "solo";
/// Mute flag - skip this layer in compose
pub const A_MUTE: &str = "mute";
/// Visibility flag
pub const A_VISIBLE: &str = "visible";
/// Listed in Project UI (false = hidden preview comp)
pub const A_LISTED: &str = "listed";
/// Blend mode (normal, screen, add, multiply, etc.)
pub const A_BLEND_MODE: &str = "blend_mode";

// === Transform ===
/// Position (Vec3)
pub const A_POSITION: &str = "position";
/// Rotation (Vec3)
pub const A_ROTATION: &str = "rotation";
/// Scale (Vec3)
pub const A_SCALE: &str = "scale";
/// Pivot point (Vec3)
pub const A_PIVOT: &str = "pivot";
/// Opacity (0.0-1.0)
pub const A_OPACITY: &str = "opacity";

// === Playback ===
/// Playback speed multiplier
pub const A_SPEED: &str = "speed";

// === Relationships ===
/// Source comp UUID (for linked comps)
pub const A_SOURCE_UUID: &str = "source_uuid";
/// Children list (Vec<Attrs>)
pub const A_CHILDREN: &str = "children";

// === File mode attributes ===
/// File mask pattern (e.g., "frame.%04d.exr")
pub const A_FILE_MASK: &str = "file_mask";
/// Directory containing frames
pub const A_FILE_DIR: &str = "file_dir";
/// First frame number in sequence
pub const A_FILE_START: &str = "file_start";
/// Last frame number in sequence
pub const A_FILE_END: &str = "file_end";

// === Dimensions ===
/// Width in pixels (0 = auto-detect)
pub const A_WIDTH: &str = "width";
/// Height in pixels (0 = auto-detect)
pub const A_HEIGHT: &str = "height";

// === Layer attributes ===
/// Source length in frames (invariant, doesn't change with speed)
pub const A_SRC_LEN: &str = "src_len";

