//! Static attribute schemas for all entity types.
//!
//! Each schema defines attribute metadata (DAG, display, keyable, etc.)
//! Used by Attrs::set() to auto-determine cache invalidation.

use super::attrs::{AttrDef, AttrSchema, AttrType, FLAG_DAG, FLAG_DISPLAY, FLAG_INTERNAL, FLAG_KEYABLE, FLAG_READONLY};

// Shorthand flag combos
const DAG: u8 = FLAG_DAG;
const DAG_DISP: u8 = FLAG_DAG | FLAG_DISPLAY;
const DAG_DISP_KEY: u8 = FLAG_DAG | FLAG_DISPLAY | FLAG_KEYABLE;
const DISP: u8 = FLAG_DISPLAY;
const DISP_RO: u8 = FLAG_DISPLAY | FLAG_READONLY;
const INT: u8 = FLAG_INTERNAL;
const INT_DAG: u8 = FLAG_INTERNAL | FLAG_DAG;

// ============================================================================
// FileNode Schema
// ============================================================================

const FILE_DEFS: &[AttrDef] = &[
    // Identity
    AttrDef::new("name", AttrType::String, DISP),
    AttrDef::new("uuid", AttrType::Uuid, INT),
    
    // Source file
    AttrDef::new("file_mask", AttrType::String, DAG_DISP),
    AttrDef::new("file_dir", AttrType::String, DAG_DISP),
    AttrDef::new("file_start", AttrType::Int, DAG_DISP),
    AttrDef::new("file_end", AttrType::Int, DAG_DISP),
    AttrDef::new("padding", AttrType::Int, DAG),
    
    // Resolution (readonly - from source)
    AttrDef::new("width", AttrType::Int, DISP_RO),
    AttrDef::new("height", AttrType::Int, DISP_RO),
    AttrDef::new("fps", AttrType::Float, DISP_RO),
    
    // Timing
    AttrDef::new("in", AttrType::Int, DAG_DISP),
    AttrDef::new("out", AttrType::Int, DAG_DISP),
    AttrDef::new("trim_in", AttrType::Int, DAG_DISP),
    AttrDef::new("trim_out", AttrType::Int, DAG_DISP),
    AttrDef::new("src_len", AttrType::Int, DAG),
];

pub static FILE_SCHEMA: AttrSchema = AttrSchema::new("FileNode", FILE_DEFS);

// ============================================================================
// CompNode Schema
// ============================================================================

const COMP_DEFS: &[AttrDef] = &[
    // Identity
    AttrDef::new("name", AttrType::String, DISP),
    AttrDef::new("uuid", AttrType::Uuid, INT),
    
    // Resolution
    AttrDef::new("width", AttrType::Int, DISP_RO),
    AttrDef::new("height", AttrType::Int, DISP_RO),
    AttrDef::new("fps", AttrType::Float, DISP),              // Non-DAG: fps is playback rate, not render
    
    // Timing
    AttrDef::new("in", AttrType::Int, DAG_DISP),
    AttrDef::new("out", AttrType::Int, DAG_DISP),
    AttrDef::new("trim_in", AttrType::Int, DAG_DISP),
    AttrDef::new("trim_out", AttrType::Int, DAG_DISP),
    AttrDef::new("src_len", AttrType::Int, DAG),
    
    // Playhead - NON-DAG! This is what we're fixing
    AttrDef::new("frame", AttrType::Int, 0), // No flags = non-DAG
    
    // Node editor position (UI only, non-DAG)
    AttrDef::new("node_pos", AttrType::Vec3, 0),
];

pub static COMP_SCHEMA: AttrSchema = AttrSchema::new("CompNode", COMP_DEFS);

// ============================================================================
// Layer Schema
// ============================================================================

const LAYER_DEFS: &[AttrDef] = &[
    // Identity
    AttrDef::new("name", AttrType::String, DISP),
    AttrDef::new("uuid", AttrType::Uuid, INT),
    AttrDef::new("source_uuid", AttrType::Uuid, INT_DAG), // DAG - affects source lookup
    
    // Timing
    AttrDef::new("in", AttrType::Int, DAG_DISP),
    AttrDef::new("out", AttrType::Int, DAG_DISP),
    AttrDef::new("trim_in", AttrType::Int, DAG_DISP),
    AttrDef::new("trim_out", AttrType::Int, DAG_DISP),
    AttrDef::new("src_len", AttrType::Int, DAG),
    AttrDef::new("speed", AttrType::Float, DAG_DISP_KEY),
    
    // Compositing
    AttrDef::new("opacity", AttrType::Float, DAG_DISP_KEY),
    AttrDef::new("blend_mode", AttrType::String, DAG_DISP),
    AttrDef::new("visible", AttrType::Bool, DAG_DISP),
    AttrDef::new("mute", AttrType::Bool, DAG_DISP),
    AttrDef::new("solo", AttrType::Bool, DAG_DISP),
    
    // Transform (all keyable)
    AttrDef::new("position", AttrType::Vec3, DAG_DISP_KEY),
    AttrDef::new("rotation", AttrType::Vec3, DAG_DISP_KEY),
    AttrDef::new("scale", AttrType::Vec3, DAG_DISP_KEY),
    AttrDef::new("pivot", AttrType::Vec3, DAG_DISP_KEY),
    
    // Node editor position (UI only, non-DAG)
    AttrDef::new("node_pos", AttrType::Vec3, 0),
];

pub static LAYER_SCHEMA: AttrSchema = AttrSchema::new("Layer", LAYER_DEFS);

// ============================================================================
// Project Schema
// ============================================================================

const PROJECT_DEFS: &[AttrDef] = &[
    AttrDef::new("name", AttrType::String, DISP),
    AttrDef::new("uuid", AttrType::Uuid, INT),
    AttrDef::new("order", AttrType::Json, INT),      // UI: media pool order
    AttrDef::new("selection", AttrType::Json, INT),  // UI: selected items
    AttrDef::new("active", AttrType::Json, INT),     // UI: active comp
    AttrDef::new("tool", AttrType::String, 0),       // Viewport tool: select/move/rotate/scale
    AttrDef::new("prefs", AttrType::Json, INT),      // UI: project preferences (gizmo, etc)
];

pub static PROJECT_SCHEMA: AttrSchema = AttrSchema::new("Project", PROJECT_DEFS);

// ============================================================================
// Player Schema
// ============================================================================

const PLAYER_DEFS: &[AttrDef] = &[
    AttrDef::new("is_playing", AttrType::Bool, 0),     // Non-DAG
    AttrDef::new("fps_base", AttrType::Float, 0),      // Non-DAG
    AttrDef::new("fps_play", AttrType::Float, 0),      // Non-DAG
    AttrDef::new("loop_enabled", AttrType::Bool, 0),   // Non-DAG
    AttrDef::new("play_direction", AttrType::Float, 0), // Non-DAG
];

pub static PLAYER_SCHEMA: AttrSchema = AttrSchema::new("Player", PLAYER_DEFS);

// ============================================================================
// CameraNode Schema
// ============================================================================

const CAMERA_DEFS: &[AttrDef] = &[
    // Identity
    AttrDef::new("name", AttrType::String, DISP),
    AttrDef::new("uuid", AttrType::Uuid, INT),
    
    // Standard layer transform (like any other layer)
    AttrDef::new("position", AttrType::Vec3, DAG_DISP_KEY),       // [0, 0, -1000]
    AttrDef::new("rotation", AttrType::Vec3, DAG_DISP_KEY),       // [0, 0, 0] Euler XYZ
    AttrDef::new("scale", AttrType::Vec3, DAG_DISP_KEY),          // [1, 1, 1]
    AttrDef::new("pivot", AttrType::Vec3, DAG_DISP_KEY),          // [0, 0, 0]
    
    // Camera-specific: look-at target (alternative to rotation)
    AttrDef::new("point_of_interest", AttrType::Vec3, DAG_DISP_KEY), // [0, 0, 0]
    AttrDef::new("use_poi", AttrType::Bool, DAG_DISP),            // true = use POI, false = use rotation
    
    // Lens
    AttrDef::new("fov", AttrType::Float, DAG_DISP_KEY),           // 39.6 (AE default)
    AttrDef::new("near_clip", AttrType::Float, DAG_DISP),         // 1.0
    AttrDef::new("far_clip", AttrType::Float, DAG_DISP),          // 10000.0
    
    // Depth of field (future)
    AttrDef::new("dof_enabled", AttrType::Bool, DAG_DISP),        // false
    AttrDef::new("focus_distance", AttrType::Float, DAG_DISP_KEY), // 1000.0
    AttrDef::new("aperture", AttrType::Float, DAG_DISP_KEY),      // 2.8
    
    // Timing (unified with other nodes)
    AttrDef::new("in", AttrType::Int, DAG_DISP),                  // 0
    AttrDef::new("src_len", AttrType::Int, DAG),                  // 100
    AttrDef::new("trim_in", AttrType::Int, DAG_DISP),             // 0
    AttrDef::new("trim_out", AttrType::Int, DAG_DISP),            // 0
    AttrDef::new("speed", AttrType::Float, DAG_DISP_KEY),         // 1.0
    AttrDef::new("opacity", AttrType::Float, DAG_DISP_KEY),       // 1.0 (for camera fades)
];

pub static CAMERA_SCHEMA: AttrSchema = AttrSchema::new("CameraNode", CAMERA_DEFS);

// ============================================================================
// TextNode Schema
// ============================================================================

const TEXT_DEFS: &[AttrDef] = &[
    // Identity
    AttrDef::new("name", AttrType::String, DISP),
    AttrDef::new("uuid", AttrType::Uuid, INT),
    
    // Resolution (affects output frame size)
    AttrDef::new("width", AttrType::Int, DAG_DISP),               // 1920 (0 = auto-fit to text)
    AttrDef::new("height", AttrType::Int, DAG_DISP),              // 1080 (0 = auto-fit to text)
    
    // Text content
    AttrDef::new("text", AttrType::String, DAG_DISP),             // "Hello"
    AttrDef::new("font", AttrType::String, DAG_DISP),             // "Arial" or path
    AttrDef::new("font_size", AttrType::Float, DAG_DISP_KEY),     // 72.0
    AttrDef::new("color", AttrType::Vec4, DAG_DISP_KEY),          // [1,1,1,1] white
    AttrDef::new("alignment", AttrType::String, DAG_DISP),        // "left"|"center"|"right"
    AttrDef::new("line_height", AttrType::Float, DAG_DISP),       // 1.2
    
    // Background
    AttrDef::new("bg_color", AttrType::Vec4, DAG_DISP),           // [0,0,0,0] transparent
    
    // Timing (unified with other nodes)
    AttrDef::new("in", AttrType::Int, DAG_DISP),                  // 0
    AttrDef::new("src_len", AttrType::Int, DAG),                  // 100
    AttrDef::new("trim_in", AttrType::Int, DAG_DISP),             // 0
    AttrDef::new("trim_out", AttrType::Int, DAG_DISP),            // 0
    AttrDef::new("speed", AttrType::Float, DAG_DISP_KEY),         // 1.0
    AttrDef::new("opacity", AttrType::Float, DAG_DISP_KEY),        // 1.0 (for text fades)
];

pub static TEXT_SCHEMA: AttrSchema = AttrSchema::new("TextNode", TEXT_DEFS);
