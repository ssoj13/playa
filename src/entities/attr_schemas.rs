//! Static attribute schemas for all entity types.
//!
//! Each schema defines attribute metadata (DAG, display, keyable, etc.)
//! Used by Attrs::set() to auto-determine cache invalidation.
//!
//! # Schema Composition
//!
//! Common attribute groups are defined once and composed into entity schemas:
//! - `IDENTITY`: name + uuid (all entities)
//! - `TIMING`: in/out/trim/speed (timed entities)
//! - `TRANSFORM`: position/rotation/scale/pivot (spatial entities)

use std::sync::LazyLock;
use super::attrs::{AttrDef, AttrSchema, AttrType, FLAG_DAG, FLAG_DISPLAY, FLAG_INTERNAL, FLAG_KEYABLE, FLAG_READONLY};

// ============================================================================
// Flag Shorthand Combos
// ============================================================================

const DAG: u8 = FLAG_DAG;
const DAG_DISP: u8 = FLAG_DAG | FLAG_DISPLAY;
const DAG_DISP_KEY: u8 = FLAG_DAG | FLAG_DISPLAY | FLAG_KEYABLE;
const DISP: u8 = FLAG_DISPLAY;
const DISP_RO: u8 = FLAG_DISPLAY | FLAG_READONLY;
const INT: u8 = FLAG_INTERNAL;
const INT_DAG: u8 = FLAG_INTERNAL | FLAG_DAG;

// ============================================================================
// Common Attribute Groups (DRY)
// ============================================================================

/// Identity: name + uuid + listed (used by all entities)
/// - `listed`: controls visibility in Project UI panel (default true)
///   Used by preview comp singleton which has listed=false to stay hidden.
///   Also filtered during serialization - unlisted nodes not saved.
const IDENTITY: &[AttrDef] = &[
    AttrDef::with_order("name", AttrType::String, DISP, 0.0),
    AttrDef::with_order("uuid", AttrType::Uuid, INT, 0.1),
    AttrDef::with_order("listed", AttrType::Bool, INT, 0.2),
];

/// Timing attributes: in/out points, trim, speed (used by timed entities)
const TIMING: &[AttrDef] = &[
    AttrDef::with_order("in", AttrType::Int, DAG_DISP, 20.0),
    AttrDef::with_order("out", AttrType::Int, DAG_DISP, 20.1),
    AttrDef::with_order("trim_in", AttrType::Int, DAG_DISP, 20.2),
    AttrDef::with_order("trim_out", AttrType::Int, DAG_DISP, 20.3),
    AttrDef::with_order("src_len", AttrType::Int, DAG, 20.4),
    AttrDef::with_ui_order("speed", AttrType::Float, DAG_DISP_KEY, &["0.1", "4", "0.1"], 20.5),
];

/// Transform attributes: position/rotation/scale/pivot (used by spatial entities)
const TRANSFORM: &[AttrDef] = &[
    AttrDef::with_order("position", AttrType::Vec3, DAG_DISP_KEY, 40.0),
    AttrDef::with_order("rotation", AttrType::Vec3, DAG_DISP_KEY, 40.1),
    AttrDef::with_order("scale", AttrType::Vec3, DAG_DISP_KEY, 40.2),
    AttrDef::with_order("pivot", AttrType::Vec3, DAG_DISP_KEY, 50.0),
];

/// Node editor position (UI only, non-DAG)
const NODE_POS: &[AttrDef] = &[
    AttrDef::with_order("node_pos", AttrType::Vec3, 0, 70.0),
];

/// Resolution attributes (readonly - derived from source)
const RESOLUTION_RO: &[AttrDef] = &[
    AttrDef::with_order("width", AttrType::Int, DISP_RO, 10.0),
    AttrDef::with_order("height", AttrType::Int, DISP_RO, 10.1),
];

/// Opacity for compositing (keyable)
const OPACITY: &[AttrDef] = &[
    AttrDef::with_ui_order("opacity", AttrType::Float, DAG_DISP_KEY, &["0", "1", "0.01"], 50.1),
];

// ============================================================================
// FileNode Schema
// ============================================================================

/// FileNode-specific attributes (source file info)
const FILE_SPECIFIC: &[AttrDef] = &[
    // Source file
    AttrDef::with_order("file_dir", AttrType::String, DAG_DISP, 60.0),
    AttrDef::with_order("file_mask", AttrType::String, DAG_DISP, 60.1),
    AttrDef::with_order("file_start", AttrType::Int, DAG_DISP, 60.2),
    AttrDef::with_order("file_end", AttrType::Int, DAG_DISP, 60.3),
    AttrDef::with_order("padding", AttrType::Int, DAG, 60.4),
    // FPS from source (readonly)
    AttrDef::with_order("fps", AttrType::Float, DISP_RO, 20.6),
];

pub static FILE_SCHEMA: LazyLock<AttrSchema> = LazyLock::new(|| {
    AttrSchema::from_slices("FileNode", &[IDENTITY, FILE_SPECIFIC, RESOLUTION_RO, TIMING])
});

// ============================================================================
// CompNode Schema
// ============================================================================

/// CompNode-specific attributes
const COMP_SPECIFIC: &[AttrDef] = &[
    // FPS is playback rate, not render (non-DAG)
    AttrDef::with_order("fps", AttrType::Float, DISP, 20.6),
    // Playhead - NON-DAG! (no cache invalidation on scrub)
    AttrDef::with_order("frame", AttrType::Int, 0, 20.7),
    // Timeline bookmarks: Map of digit "0"-"9" -> frame number
    AttrDef::with_order("bookmarks", AttrType::Map, 0, 90.0),
];

pub static COMP_SCHEMA: LazyLock<AttrSchema> = LazyLock::new(|| {
    AttrSchema::from_slices("CompNode", &[IDENTITY, RESOLUTION_RO, COMP_SPECIFIC, TIMING, NODE_POS])
});

// ============================================================================
// Layer Schema
// ============================================================================

/// Layer-specific attributes (compositing)
const LAYER_SPECIFIC: &[AttrDef] = &[
    AttrDef::with_order("source_uuid", AttrType::Uuid, INT_DAG, 80.0), // DAG - affects source lookup
    // Compositing
    AttrDef::with_ui_order("blend_mode", AttrType::String, DAG_DISP,
        &["normal", "screen", "add", "subtract", "multiply", "divide", "difference", "overlay"], 30.3),
    AttrDef::with_order("visible", AttrType::Bool, DAG_DISP, 30.0),
    AttrDef::with_order("renderable", AttrType::Bool, DAG_DISP, 30.4),  // false for camera/light/null/audio
    AttrDef::with_order("mute", AttrType::Bool, DAG_DISP, 30.2),
    AttrDef::with_order("solo", AttrType::Bool, DAG_DISP, 30.1),
];

pub static LAYER_SCHEMA: LazyLock<AttrSchema> = LazyLock::new(|| {
    AttrSchema::from_slices("Layer", &[IDENTITY, LAYER_SPECIFIC, TIMING, OPACITY, TRANSFORM, NODE_POS])
});

// ============================================================================
// Project Schema
// ============================================================================

/// Project-specific attributes (UI state)
const PROJECT_SPECIFIC: &[AttrDef] = &[
    AttrDef::with_order("order", AttrType::List, INT, 90.0),      // UI: media pool order (Uuid list)
    AttrDef::with_order("selection", AttrType::List, INT, 90.1),  // UI: selected items (Uuid list)
    AttrDef::with_order("active", AttrType::Uuid, INT, 90.2),     // UI: active comp (Uuid)
    AttrDef::with_ui_order("tool", AttrType::String, 0,    // Viewport tool
        &["select", "move", "rotate", "scale"], 90.3),
    AttrDef::with_order("prefs", AttrType::Map, INT, 90.4),       // UI: project preferences (gizmo, etc)
];

pub static PROJECT_SCHEMA: LazyLock<AttrSchema> = LazyLock::new(|| {
    AttrSchema::from_slices("Project", &[IDENTITY, PROJECT_SPECIFIC])
});

// ============================================================================
// Player Schema
// ============================================================================

/// Player attributes (all non-DAG, playback state)
const PLAYER_SPECIFIC: &[AttrDef] = &[
    AttrDef::with_order("is_playing", AttrType::Bool, 0, 90.0),
    AttrDef::with_order("fps_base", AttrType::Float, 0, 90.1),
    AttrDef::with_order("fps_play", AttrType::Float, 0, 90.2),
    AttrDef::with_order("loop_enabled", AttrType::Bool, 0, 90.3),
    AttrDef::with_order("play_direction", AttrType::Float, 0, 90.4),
];

pub static PLAYER_SCHEMA: LazyLock<AttrSchema> = LazyLock::new(|| {
    AttrSchema::new("Player", PLAYER_SPECIFIC)
});

// ============================================================================
// CameraNode Schema
// ============================================================================

/// Camera-specific attributes (lens, DOF)
const CAMERA_SPECIFIC: &[AttrDef] = &[
    // Projection type
    AttrDef::with_ui_order("projection_type", AttrType::String, DAG_DISP,
        &["perspective", "orthographic"], 60.0),
    // Look-at target (alternative to rotation)
    AttrDef::with_order("point_of_interest", AttrType::Vec3, DAG_DISP_KEY, 60.1),
    AttrDef::with_order("use_poi", AttrType::Bool, DAG_DISP, 60.2),
    // Lens (perspective mode)
    AttrDef::with_ui_order("fov", AttrType::Float, DAG_DISP_KEY, &["1", "180", "0.1"], 61.0),
    AttrDef::with_order("near_clip", AttrType::Float, DAG_DISP, 61.1),
    AttrDef::with_order("far_clip", AttrType::Float, DAG_DISP, 61.2),
    // Ortho zoom (orthographic mode)
    AttrDef::with_ui_order("ortho_scale", AttrType::Float, DAG_DISP_KEY, &["0.01", "10", "0.01"], 62.0),
    // Depth of field (future)
    AttrDef::with_order("dof_enabled", AttrType::Bool, DAG_DISP, 63.0),
    AttrDef::with_order("focus_distance", AttrType::Float, DAG_DISP_KEY, 63.1),
    AttrDef::with_ui_order("aperture", AttrType::Float, DAG_DISP_KEY, &["0.5", "32", "0.1"], 63.2),
];

pub static CAMERA_SCHEMA: LazyLock<AttrSchema> = LazyLock::new(|| {
    // WHY NO TRANSFORM:
    // Camera is a spatial object in the composition, like any other layer.
    // Position/rotation/scale come from the LAYER that references this camera,
    // not from CameraNode itself. This follows After Effects model where
    // camera transform is on the layer, and CameraNode only stores lens params
    // (fov, near/far, projection type, DOF settings).
    //
    // Benefits:
    // - No duplicate position attrs (layer.position vs camera.position)
    // - Animation works naturally on layer transform
    // - Consistent with how all spatial objects (lights, nulls) will work
    AttrSchema::from_slices("CameraNode", &[IDENTITY, CAMERA_SPECIFIC, TIMING, OPACITY])
});

// ============================================================================
// TextNode Schema
// ============================================================================

/// Text-specific attributes (content, styling)
const TEXT_SPECIFIC: &[AttrDef] = &[
    // Resolution (editable for text)
    AttrDef::with_order("width", AttrType::Int, DAG_DISP, 10.0),
    AttrDef::with_order("height", AttrType::Int, DAG_DISP, 10.1),
    // Text content
    AttrDef::with_order("text", AttrType::String, DAG_DISP, 60.0),
    AttrDef::with_order("font", AttrType::String, DAG_DISP, 60.1),
    AttrDef::with_ui_order("font_size", AttrType::Float, DAG_DISP_KEY, &["1", "500", "1"], 60.2),
    AttrDef::with_order("color", AttrType::Vec4, DAG_DISP_KEY, 60.3),
    AttrDef::with_ui_order("alignment", AttrType::String, DAG_DISP, &["left", "center", "right"], 60.4),
    AttrDef::with_ui_order("line_height", AttrType::Float, DAG_DISP, &["0.5", "3", "0.1"], 60.5),
    // Background
    AttrDef::with_order("bg_color", AttrType::Vec4, DAG_DISP, 60.6),
];

pub static TEXT_SCHEMA: LazyLock<AttrSchema> = LazyLock::new(|| {
    AttrSchema::from_slices("TextNode", &[IDENTITY, TEXT_SPECIFIC, TIMING, OPACITY])
});
