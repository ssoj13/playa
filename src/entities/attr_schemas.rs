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
    AttrDef::new("fps", AttrType::Float, DISP),
    
    // Timing
    AttrDef::new("in", AttrType::Int, DAG_DISP),
    AttrDef::new("out", AttrType::Int, DAG_DISP),
    AttrDef::new("trim_in", AttrType::Int, DAG_DISP),
    AttrDef::new("trim_out", AttrType::Int, DAG_DISP),
    AttrDef::new("src_len", AttrType::Int, DAG),
    
    // Playhead - NON-DAG! This is what we're fixing
    AttrDef::new("frame", AttrType::Int, 0), // No flags = non-DAG
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
];

pub static LAYER_SCHEMA: AttrSchema = AttrSchema::new("Layer", LAYER_DEFS);

// ============================================================================
// Project Schema
// ============================================================================

const PROJECT_DEFS: &[AttrDef] = &[
    AttrDef::new("name", AttrType::String, DISP),
    AttrDef::new("uuid", AttrType::Uuid, INT),
    AttrDef::new("comps_order", AttrType::Json, INT), // UI state
    AttrDef::new("selection", AttrType::Json, INT),   // UI state
    AttrDef::new("active", AttrType::Json, INT),      // UI state
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
