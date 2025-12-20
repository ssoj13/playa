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

/// Identity: name + uuid (used by all entities)
const IDENTITY: &[AttrDef] = &[
    AttrDef::new("name", AttrType::String, DISP),
    AttrDef::new("uuid", AttrType::Uuid, INT),
];

/// Timing attributes: in/out points, trim, speed (used by timed entities)
const TIMING: &[AttrDef] = &[
    AttrDef::new("in", AttrType::Int, DAG_DISP),
    AttrDef::new("out", AttrType::Int, DAG_DISP),
    AttrDef::new("trim_in", AttrType::Int, DAG_DISP),
    AttrDef::new("trim_out", AttrType::Int, DAG_DISP),
    AttrDef::new("src_len", AttrType::Int, DAG),
    AttrDef::with_ui("speed", AttrType::Float, DAG_DISP_KEY, &["0.1", "4", "0.1"]),
];

/// Transform attributes: position/rotation/scale/pivot (used by spatial entities)
const TRANSFORM: &[AttrDef] = &[
    AttrDef::new("position", AttrType::Vec3, DAG_DISP_KEY),
    AttrDef::new("rotation", AttrType::Vec3, DAG_DISP_KEY),
    AttrDef::new("scale", AttrType::Vec3, DAG_DISP_KEY),
    AttrDef::new("pivot", AttrType::Vec3, DAG_DISP_KEY),
];

/// Node editor position (UI only, non-DAG)
const NODE_POS: &[AttrDef] = &[
    AttrDef::new("node_pos", AttrType::Vec3, 0),
];

/// Resolution attributes (readonly - derived from source)
const RESOLUTION_RO: &[AttrDef] = &[
    AttrDef::new("width", AttrType::Int, DISP_RO),
    AttrDef::new("height", AttrType::Int, DISP_RO),
];

/// Opacity for compositing (keyable)
const OPACITY: &[AttrDef] = &[
    AttrDef::with_ui("opacity", AttrType::Float, DAG_DISP_KEY, &["0", "1", "0.01"]),
];

// ============================================================================
// FileNode Schema
// ============================================================================

/// FileNode-specific attributes (source file info)
const FILE_SPECIFIC: &[AttrDef] = &[
    // Source file
    AttrDef::new("file_mask", AttrType::String, DAG_DISP),
    AttrDef::new("file_dir", AttrType::String, DAG_DISP),
    AttrDef::new("file_start", AttrType::Int, DAG_DISP),
    AttrDef::new("file_end", AttrType::Int, DAG_DISP),
    AttrDef::new("padding", AttrType::Int, DAG),
    // FPS from source (readonly)
    AttrDef::new("fps", AttrType::Float, DISP_RO),
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
    AttrDef::new("fps", AttrType::Float, DISP),
    // Playhead - NON-DAG! (no cache invalidation on scrub)
    AttrDef::new("frame", AttrType::Int, 0),
];

pub static COMP_SCHEMA: LazyLock<AttrSchema> = LazyLock::new(|| {
    AttrSchema::from_slices("CompNode", &[IDENTITY, RESOLUTION_RO, COMP_SPECIFIC, TIMING, NODE_POS])
});

// ============================================================================
// Layer Schema
// ============================================================================

/// Layer-specific attributes (compositing)
const LAYER_SPECIFIC: &[AttrDef] = &[
    AttrDef::new("source_uuid", AttrType::Uuid, INT_DAG), // DAG - affects source lookup
    // Compositing
    AttrDef::with_ui("blend_mode", AttrType::String, DAG_DISP,
        &["normal", "screen", "add", "subtract", "multiply", "divide", "difference"]),
    AttrDef::new("visible", AttrType::Bool, DAG_DISP),
    AttrDef::new("renderable", AttrType::Bool, DAG_DISP),  // false for camera/light/null/audio
    AttrDef::new("mute", AttrType::Bool, DAG_DISP),
    AttrDef::new("solo", AttrType::Bool, DAG_DISP),
];

pub static LAYER_SCHEMA: LazyLock<AttrSchema> = LazyLock::new(|| {
    AttrSchema::from_slices("Layer", &[IDENTITY, LAYER_SPECIFIC, TIMING, OPACITY, TRANSFORM, NODE_POS])
});

// ============================================================================
// Project Schema
// ============================================================================

/// Project-specific attributes (UI state)
const PROJECT_SPECIFIC: &[AttrDef] = &[
    AttrDef::new("order", AttrType::List, INT),      // UI: media pool order (Uuid list)
    AttrDef::new("selection", AttrType::List, INT),  // UI: selected items (Uuid list)
    AttrDef::new("active", AttrType::Uuid, INT),     // UI: active comp (Uuid)
    AttrDef::with_ui("tool", AttrType::String, 0,    // Viewport tool
        &["select", "move", "rotate", "scale"]),
    AttrDef::new("prefs", AttrType::Map, INT),       // UI: project preferences (gizmo, etc)
];

pub static PROJECT_SCHEMA: LazyLock<AttrSchema> = LazyLock::new(|| {
    AttrSchema::from_slices("Project", &[IDENTITY, PROJECT_SPECIFIC])
});

// ============================================================================
// Player Schema
// ============================================================================

/// Player attributes (all non-DAG, playback state)
const PLAYER_SPECIFIC: &[AttrDef] = &[
    AttrDef::new("is_playing", AttrType::Bool, 0),
    AttrDef::new("fps_base", AttrType::Float, 0),
    AttrDef::new("fps_play", AttrType::Float, 0),
    AttrDef::new("loop_enabled", AttrType::Bool, 0),
    AttrDef::new("play_direction", AttrType::Float, 0),
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
    AttrDef::with_ui("projection_type", AttrType::String, DAG_DISP,
        &["perspective", "orthographic"]),
    // Look-at target (alternative to rotation)
    AttrDef::new("point_of_interest", AttrType::Vec3, DAG_DISP_KEY),
    AttrDef::new("use_poi", AttrType::Bool, DAG_DISP),
    // Lens (perspective mode)
    AttrDef::with_ui("fov", AttrType::Float, DAG_DISP_KEY, &["1", "180", "0.1"]),
    AttrDef::new("near_clip", AttrType::Float, DAG_DISP),
    AttrDef::new("far_clip", AttrType::Float, DAG_DISP),
    // Ortho zoom (orthographic mode)
    AttrDef::with_ui("ortho_scale", AttrType::Float, DAG_DISP_KEY, &["0.01", "10", "0.01"]),
    // Depth of field (future)
    AttrDef::new("dof_enabled", AttrType::Bool, DAG_DISP),
    AttrDef::new("focus_distance", AttrType::Float, DAG_DISP_KEY),
    AttrDef::with_ui("aperture", AttrType::Float, DAG_DISP_KEY, &["0.5", "32", "0.1"]),
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
    AttrDef::new("width", AttrType::Int, DAG_DISP),
    AttrDef::new("height", AttrType::Int, DAG_DISP),
    // Text content
    AttrDef::new("text", AttrType::String, DAG_DISP),
    AttrDef::new("font", AttrType::String, DAG_DISP),
    AttrDef::with_ui("font_size", AttrType::Float, DAG_DISP_KEY, &["1", "500", "1"]),
    AttrDef::new("color", AttrType::Vec4, DAG_DISP_KEY),
    AttrDef::with_ui("alignment", AttrType::String, DAG_DISP, &["left", "center", "right"]),
    AttrDef::with_ui("line_height", AttrType::Float, DAG_DISP, &["0.5", "3", "0.1"]),
    // Background
    AttrDef::new("bg_color", AttrType::Vec4, DAG_DISP),
];

pub static TEXT_SCHEMA: LazyLock<AttrSchema> = LazyLock::new(|| {
    AttrSchema::from_slices("TextNode", &[IDENTITY, TEXT_SPECIFIC, TIMING, OPACITY])
});
