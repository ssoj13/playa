//! Generic attribute storage shared across core types.
//!
//! Used by Frame, Clip, Layer, Comp, Project.
//!
//! # Dirty Flag & Cache Invalidation
//!
//! Each `Attrs` instance has an atomic `dirty` flag used for cache invalidation.
//! With schema attached, `set()` auto-detects DAG vs non-DAG attributes:
//!
//! - **DAG attrs** (opacity, transforms, timing): `set()` marks dirty → cache invalidation
//! - **Non-DAG attrs** (playhead, selection, UI): `set()` skips dirty → cache preserved
//!
//! ## Dataflow
//!
//! ```text
//! User changes opacity → attrs.set() → schema.is_dag("opacity")=true → dirty=true
//!   → modify_comp() detects is_dirty()
//!   → emits AttrsChangedEvent
//!   → cache.clear_comp() invalidates frames
//!   → compute() recomposes → clear_dirty()
//!
//! Playback advances frame → attrs.set() → schema.is_dag("frame")=false → dirty unchanged
//!   → modify_comp() sees !is_dirty()
//!   → NO event emitted → cache stays valid
//! ```
//!
//! # Hashing
//!
//! - `hash_all()` and `hash_filtered()` hash keys in sorted order for determinism.
//! - `AttrValue` hashes floats via `to_bits`; matrices/vectors are flattened.
//! - `Attrs` hashing is used by `Comp::compute_comp_hash` to invalidate cached frames
//!   when any child attribute changes.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use uuid::Uuid;

use super::keys::{A_IN, A_SPEED, A_SRC_LEN, A_TRIM_IN, A_TRIM_OUT};

// ============================================================================
// Attribute Schema System
// ============================================================================

/// Attribute flags (bitfield)
/// Controls behavior and visibility of attributes
pub type AttrFlags = u8;

/// Attribute affects DAG/render - changes invalidate cache
pub const FLAG_DAG: AttrFlags = 1 << 0;
/// Attribute shown in Attribute Editor UI
pub const FLAG_DISPLAY: AttrFlags = 1 << 1;
/// Attribute can be keyframed for animation
pub const FLAG_KEYABLE: AttrFlags = 1 << 2;
/// Attribute is read-only (computed value)
pub const FLAG_READONLY: AttrFlags = 1 << 3;
/// Internal attribute, not shown to user
pub const FLAG_INTERNAL: AttrFlags = 1 << 4;

/// Expected type of attribute value
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttrType {
    Bool,
    Int,
    Float,
    String,
    Uuid,
    Vec3,
    Vec4,
    List,
    Json,
}

/// Single attribute definition
#[derive(Debug, Clone)]
pub struct AttrDef {
    pub name: &'static str,
    pub attr_type: AttrType,
    pub flags: AttrFlags,
}

impl AttrDef {
    /// Create new attribute definition
    pub const fn new(name: &'static str, attr_type: AttrType, flags: AttrFlags) -> Self {
        Self { name, attr_type, flags }
    }
    
    /// Check if attribute affects DAG (render graph)
    pub const fn is_dag(&self) -> bool {
        self.flags & FLAG_DAG != 0
    }
    
    /// Check if attribute is shown in UI
    pub const fn is_display(&self) -> bool {
        self.flags & FLAG_DISPLAY != 0
    }
    
    /// Check if attribute can be keyframed
    pub const fn is_keyable(&self) -> bool {
        self.flags & FLAG_KEYABLE != 0
    }
    
    /// Check if attribute is read-only
    pub const fn is_readonly(&self) -> bool {
        self.flags & FLAG_READONLY != 0
    }
    
    /// Check if attribute is internal
    pub const fn is_internal(&self) -> bool {
        self.flags & FLAG_INTERNAL != 0
    }
}

/// Schema: collection of attribute definitions for an entity type
#[derive(Debug, Clone)]
pub struct AttrSchema {
    pub name: &'static str,
    defs: &'static [AttrDef],
}

impl AttrSchema {
    /// Create new schema
    pub const fn new(name: &'static str, defs: &'static [AttrDef]) -> Self {
        Self { name, defs }
    }
    
    /// Find attribute definition by name
    pub fn get(&self, name: &str) -> Option<&AttrDef> {
        self.defs.iter().find(|d| d.name == name)
    }
    
    /// Check if attribute affects DAG
    pub fn is_dag(&self, name: &str) -> bool {
        self.get(name).is_some_and(|d| d.is_dag())
    }
    
    /// Check if attribute is display
    pub fn is_display(&self, name: &str) -> bool {
        self.get(name).is_some_and(|d| d.is_display())
    }
    
    /// Get all DAG attributes
    pub fn dag_attrs(&self) -> impl Iterator<Item = &AttrDef> {
        self.defs.iter().filter(|d| d.is_dag())
    }
    
    /// Get all display attributes
    pub fn display_attrs(&self) -> impl Iterator<Item = &AttrDef> {
        self.defs.iter().filter(|d| d.is_display())
    }
    
    /// Iterate all definitions
    pub fn iter(&self) -> impl Iterator<Item = &AttrDef> {
        self.defs.iter()
    }
}

/// Generic attribute value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AttrValue {
    Bool(bool),
    Str(String),
    Int8(i8),
    Int(i32),
    UInt(u32),
    Float(f32),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    Mat3([[f32; 3]; 3]),
    Mat4([[f32; 4]; 4]),
    /// UUID for entity identification
    Uuid(Uuid),
    /// Nested list of values (for children, etc.)
    List(Vec<AttrValue>),
    /// JSON-encoded nested data (HashMap, Vec, etc.)
    Json(String),
}

impl std::hash::Hash for AttrValue {
    fn hash<H: Hasher>(&self, state: &mut H) {
        use AttrValue::*;
        std::mem::discriminant(self).hash(state);
        match self {
            Bool(v) => v.hash(state),
            Str(v) => v.hash(state),
            Int8(v) => v.hash(state),
            Int(v) => v.hash(state),
            UInt(v) => v.hash(state),
            Float(v) => v.to_bits().hash(state),
            Vec3(arr) => arr.iter().for_each(|f| f.to_bits().hash(state)),
            Vec4(arr) => arr.iter().for_each(|f| f.to_bits().hash(state)),
            Mat3(m) => m.iter().flat_map(|r| r.iter()).for_each(|f| f.to_bits().hash(state)),
            Mat4(m) => m.iter().flat_map(|r| r.iter()).for_each(|f| f.to_bits().hash(state)),
            Uuid(v) => v.hash(state),
            List(v) => v.hash(state),
            Json(v) => v.hash(state),
        }
    }
}

/// Attribute container: string key → typed value.
///
/// Includes dirty tracking for cache invalidation.
/// Optional schema for automatic DAG detection.
///
/// # Type-Specific Getters/Setters
///
/// Available for all `AttrValue` variants: `get_i32`, `get_str`, `get_vec3`, etc.
/// Key constants are in `keys.rs` with `A_` prefix (e.g., `A_POSITION`).
#[derive(Debug, Serialize, Deserialize)]
pub struct Attrs {
    #[serde(default)]
    map: HashMap<String, AttrValue>,

    /// Dirty flag: set when DAG attributes are modified
    /// Used for cache invalidation instead of recomputing hashes
    /// Thread-safe AtomicBool for Send+Sync (allows background composition)
    #[serde(skip)]
    #[serde(default = "Attrs::default_dirty")]
    dirty: AtomicBool,
    
    /// Optional schema reference for automatic dirty detection
    /// If set, only DAG attributes mark dirty on change
    #[serde(skip)]
    schema: Option<&'static AttrSchema>,
}

impl Default for Attrs {
    fn default() -> Self {
        Self::new()
    }
}

impl Attrs {
    fn default_dirty() -> AtomicBool {
        AtomicBool::new(false)
    }

    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
            dirty: AtomicBool::new(false),
            schema: None,
        }
    }
    
    /// Create Attrs with schema for automatic DAG detection
    pub fn with_schema(schema: &'static AttrSchema) -> Self {
        Self {
            map: HashMap::new(),
            dirty: AtomicBool::new(false),
            schema: Some(schema),
        }
    }
    
    /// Attach schema (for deserialized entities)
    pub fn attach_schema(&mut self, schema: &'static AttrSchema) {
        self.schema = Some(schema);
    }
    
    /// Get current schema reference
    pub fn schema(&self) -> Option<&'static AttrSchema> {
        self.schema
    }

    /// Set attribute value.
    /// If schema exists: marks dirty only for DAG attributes AND only if value changed.
    /// If no schema: always marks dirty (legacy behavior).
    pub fn set(&mut self, key: impl Into<String>, value: AttrValue) {
        let key = key.into();
        
        // Check if value actually changed
        let changed = match self.map.get(&key) {
            Some(existing) => existing != &value,
            None => true, // New key = changed
        };
        
        self.map.insert(key.clone(), value);
        
        // Only mark dirty if value changed AND attr is DAG
        if changed {
            let is_dag = match &self.schema {
                Some(schema) => schema.is_dag(&key),
                None => true, // No schema = legacy, always dirty
            };
            
            if is_dag {
                self.dirty.store(true, Ordering::Relaxed);
            }
        }
    }


    pub fn get(&self, key: &str) -> Option<&AttrValue> {
        self.map.get(key)
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        match self.map.get(key) {
            Some(AttrValue::Str(s)) => Some(s),
            _ => None,
        }
    }

    pub fn get_i32(&self, key: &str) -> Option<i32> {
        match self.map.get(key) {
            Some(AttrValue::Int(v)) => Some(*v),
            _ => None,
        }
    }

    pub fn get_u32(&self, key: &str) -> Option<u32> {
        match self.map.get(key) {
            Some(AttrValue::UInt(v)) => Some(*v),
            _ => None,
        }
    }

    pub fn get_float(&self, key: &str) -> Option<f32> {
        match self.map.get(key) {
            Some(AttrValue::Float(v)) => Some(*v),
            _ => None,
        }
    }

    pub fn get_bool(&self, key: &str) -> Option<bool> {
        match self.map.get(key) {
            Some(AttrValue::Bool(v)) => Some(*v),
            _ => None,
        }
    }

    pub fn get_i8(&self, key: &str) -> Option<i8> {
        match self.map.get(key) {
            Some(AttrValue::Int8(v)) => Some(*v),
            _ => None,
        }
    }

    pub fn set_i8(&mut self, key: impl Into<String>, value: i8) {
        self.map.insert(key.into(), AttrValue::Int8(value));
        self.dirty.store(true, Ordering::Relaxed);
    }

    pub fn get_uuid(&self, key: &str) -> Option<Uuid> {
        match self.map.get(key) {
            Some(AttrValue::Uuid(v)) => Some(*v),
            _ => None,
        }
    }

    pub fn set_uuid(&mut self, key: impl Into<String>, value: Uuid) {
        self.map.insert(key.into(), AttrValue::Uuid(value));
        self.dirty.store(true, Ordering::Relaxed);
    }

    pub fn get_list(&self, key: &str) -> Option<&Vec<AttrValue>> {
        match self.map.get(key) {
            Some(AttrValue::List(v)) => Some(v),
            _ => None,
        }
    }

    pub fn get_list_mut(&mut self, key: &str) -> Option<&mut Vec<AttrValue>> {
        match self.map.get_mut(key) {
            Some(AttrValue::List(v)) => Some(v),
            _ => None,
        }
    }

    pub fn set_list(&mut self, key: impl Into<String>, value: Vec<AttrValue>) {
        self.map.insert(key.into(), AttrValue::List(value));
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// Get Vec3 attribute `[x, y, z]`.
    pub fn get_vec3(&self, key: &str) -> Option<[f32; 3]> {
        match self.map.get(key) {
            Some(AttrValue::Vec3(v)) => Some(*v),
            _ => None,
        }
    }

    /// Set Vec3 attribute `[x, y, z]`.
    pub fn set_vec3(&mut self, key: impl Into<String>, value: [f32; 3]) {
        self.set(key, AttrValue::Vec3(value));
    }

    /// Get Vec4 attribute `[x, y, z, w]`.
    pub fn get_vec4(&self, key: &str) -> Option<[f32; 4]> {
        match self.map.get(key) {
            Some(AttrValue::Vec4(v)) => Some(*v),
            _ => None,
        }
    }

    /// Set Vec4 attribute `[x, y, z, w]`.
    pub fn set_vec4(&mut self, key: impl Into<String>, value: [f32; 4]) {
        self.set(key, AttrValue::Vec4(value));
    }

    /// Get Mat3 attribute (3x3 matrix, column-major).
    pub fn get_mat3(&self, key: &str) -> Option<[[f32; 3]; 3]> {
        match self.map.get(key) {
            Some(AttrValue::Mat3(v)) => Some(*v),
            _ => None,
        }
    }

    /// Set Mat3 attribute (3x3 matrix, column-major).
    pub fn set_mat3(&mut self, key: impl Into<String>, value: [[f32; 3]; 3]) {
        self.set(key, AttrValue::Mat3(value));
    }

    /// Get Mat4 attribute (4x4 matrix, column-major).
    pub fn get_mat4(&self, key: &str) -> Option<[[f32; 4]; 4]> {
        match self.map.get(key) {
            Some(AttrValue::Mat4(v)) => Some(*v),
            _ => None,
        }
    }

    /// Set Mat4 attribute (4x4 matrix, column-major).
    pub fn set_mat4(&mut self, key: impl Into<String>, value: [[f32; 4]; 4]) {
        self.set(key, AttrValue::Mat4(value));
    }

    // Generic helpers with defaults (to reduce boilerplate)

    /// Get i32 value with default fallback of 0
    pub fn get_i32_or_zero(&self, key: &str) -> i32 {
        self.get_i32(key).unwrap_or(0)
    }

    /// Get i32 value with custom default
    pub fn get_i32_or(&self, key: &str, default: i32) -> i32 {
        self.get_i32(key).unwrap_or(default)
    }

    /// Get float value with custom default
    pub fn get_float_or(&self, key: &str, default: f32) -> f32 {
        self.get_float(key).unwrap_or(default)
    }

    /// Get bool value with custom default
    pub fn get_bool_or(&self, key: &str, default: bool) -> bool {
        self.get_bool(key).unwrap_or(default)
    }

    /// Layer visible start in parent coords: in + trim_in/speed
    /// trim_in is source frames offset, converted to timeline frames via speed
    pub fn layer_start(&self) -> i32 {
        let in_val = self.get_i32_or_zero(A_IN);
        let trim_in = self.get_i32_or_zero(A_TRIM_IN);
        // Clamp speed to safe range (0.1..4.0) to prevent duration explosion
        let speed = self.get_float_or(A_SPEED, 1.0).clamp(0.1, 4.0);
        // Use f64 for intermediate calc to prevent overflow on large values
        let offset = (trim_in as f64 / speed as f64).round().clamp(i32::MIN as f64, i32::MAX as f64) as i32;
        in_val.saturating_add(offset)
    }

    /// Layer visible end in parent coords: layer_start + visible_timeline_frames - 1
    /// visible_src_frames = src_len - trim_in - trim_out
    /// visible_timeline_frames = visible_src_frames / speed
    pub fn layer_end(&self) -> i32 {
        let layer_start = self.layer_start();
        let src_len = self.get_i32_or_zero(A_SRC_LEN);
        let trim_in = self.get_i32_or_zero(A_TRIM_IN);
        let trim_out = self.get_i32_or_zero(A_TRIM_OUT);
        let speed = self.get_float_or(A_SPEED, 1.0).clamp(0.1, 4.0);
        // Visible source frames (at least 1 to prevent negative duration)
        let visible_src = (src_len - trim_in - trim_out).max(1);
        // Use f64 to prevent overflow on large frame counts
        let visible_timeline = (visible_src as f64 / speed as f64).round().clamp(1.0, i32::MAX as f64) as i32;
        layer_start.saturating_add(visible_timeline).saturating_sub(1)
    }

    /// Get source length (original duration in source frames)
    pub fn src_len(&self) -> i32 {
        self.get_i32_or_zero(A_SRC_LEN)
    }

    /// Full bar end (untrimmed): in + src_len/speed - 1
    /// This replaces the "out" attribute - now computed, not stored
    pub fn full_bar_end(&self) -> i32 {
        let in_val = self.get_i32_or_zero(A_IN);
        let src_len = self.get_i32_or_zero(A_SRC_LEN);
        let speed = self.get_float_or(A_SPEED, 1.0).clamp(0.1, 4.0);
        // Use f64 to prevent overflow on large frame counts
        let duration = (src_len as f64 / speed as f64).ceil().clamp(1.0, i32::MAX as f64) as i32;
        in_val.saturating_add(duration).saturating_sub(1)
    }

    /// Full bar start (same as "in")
    pub fn full_bar_start(&self) -> i32 {
        self.get_i32_or_zero(A_IN)
    }

    /// Get mutable reference to attribute value
    pub fn get_mut(&mut self, key: &str) -> Option<&mut AttrValue> {
        self.map.get_mut(key)
    }

    /// Remove attribute by key
    pub fn remove(&mut self, key: &str) -> Option<AttrValue> {
        self.map.remove(key)
    }

    /// Iterate over all attributes (key, value)
    pub fn iter(&self) -> impl Iterator<Item = (&String, &AttrValue)> {
        self.map.iter()
    }

    /// Iterate mutably over all attributes (key, value)
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&String, &mut AttrValue)> {
        self.map.iter_mut()
    }

    /// Check if attribute exists
    pub fn contains(&self, key: &str) -> bool {
        self.map.contains_key(key)
    }

    /// Get number of attributes
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Hash attributes with optional include/exclude filters.
    /// Keys are processed in sorted order for deterministic output.
    pub fn hash_filtered(&self, include: Option<&[&str]>, exclude: Option<&[&str]>) -> u64 {
        let include_set: Option<HashSet<&str>> = include.map(|v| v.iter().copied().collect());
        let exclude_set: Option<HashSet<&str>> = exclude.map(|v| v.iter().copied().collect());

        let mut keys: Vec<&String> = self.map.keys().collect();
        keys.sort_unstable();

        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for key in keys {
            if let Some(ref inc) = include_set
                && !inc.contains(key.as_str()) {
                    continue;
                }
            if let Some(ref exc) = exclude_set
                && exc.contains(key.as_str()) {
                    continue;
                }
            key.hash(&mut hasher);
            if let Some(val) = self.map.get(key) {
                val.hash(&mut hasher);
            }
        }
        hasher.finish()
    }

    /// Hash all attributes.
    pub fn hash_all(&self) -> u64 {
        self.hash_filtered(None, None)
    }

    // === Dirty tracking methods ===

    /// Check if attributes have been modified since last clear
    pub fn is_dirty(&self) -> bool {
        self.dirty.load(Ordering::Relaxed)
    }

    /// Clear dirty flag (call after cache update)
    /// Thread-safe via AtomicBool, can be called with &self
    pub fn clear_dirty(&self) {
        self.dirty.store(false, Ordering::Relaxed);
    }

    /// Mark as dirty manually (e.g., for child attr changes)
    /// Thread-safe via AtomicBool, can be called with &self
    pub fn mark_dirty(&self) {
        self.dirty.store(true, Ordering::Relaxed);
    }

    // === JSON helpers ===

    /// Get JSON value and deserialize to type T
    pub fn get_json<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
        match self.map.get(key) {
            Some(AttrValue::Json(s)) => serde_json::from_str(s).ok(),
            _ => None,
        }
    }

    /// Serialize value to JSON and store
    pub fn set_json<T: serde::Serialize>(&mut self, key: impl Into<String>, value: &T) {
        if let Ok(json) = serde_json::to_string(value) {
            self.map.insert(key.into(), AttrValue::Json(json));
            self.dirty.store(true, Ordering::Relaxed);
        }
    }

    /// Get raw JSON string
    pub fn get_json_str(&self, key: &str) -> Option<&str> {
        match self.map.get(key) {
            Some(AttrValue::Json(s)) => Some(s),
            _ => None,
        }
    }
}

// Manual Clone impl because AtomicBool doesn't impl Clone
impl Clone for Attrs {
    fn clone(&self) -> Self {
        Self {
            map: self.map.clone(),
            dirty: AtomicBool::new(self.dirty.load(Ordering::Relaxed)),
            schema: self.schema,
        }
    }
}
