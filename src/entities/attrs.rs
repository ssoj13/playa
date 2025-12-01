//! Generic attribute storage shared across core types.
//!
//! Used by Frame, Clip, Layer, Comp, Project.
//! Hashing notes:
//! - `hash_all()` and `hash_filtered()` hash keys in sorted order for determinism.
//! - `AttrValue` hashes floats via `to_bits`; matrices/vectors are flattened.
//! - `Attrs` hashing is used by `Comp::compute_comp_hash` to invalidate cached frames
//!   when any child attribute changes.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};

/// Generic attribute value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AttrValue {
    Bool(bool),
    Str(String),
    Int(i32),
    UInt(u32),
    Float(f32),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    Mat3([[f32; 3]; 3]),
    Mat4([[f32; 4]; 4]),
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
            Int(v) => v.hash(state),
            UInt(v) => v.hash(state),
            Float(v) => v.to_bits().hash(state),
            Vec3(arr) => arr.iter().for_each(|f| f.to_bits().hash(state)),
            Vec4(arr) => arr.iter().for_each(|f| f.to_bits().hash(state)),
            Mat3(m) => m.iter().flat_map(|r| r.iter()).for_each(|f| f.to_bits().hash(state)),
            Mat4(m) => m.iter().flat_map(|r| r.iter()).for_each(|f| f.to_bits().hash(state)),
            Json(v) => v.hash(state),
        }
    }
}

/// Attribute container: string key → typed value.
///
/// Includes dirty tracking for cache invalidation.
#[derive(Debug, Serialize, Deserialize)]
pub struct Attrs {
    #[serde(default)]
    map: HashMap<String, AttrValue>,

    /// Dirty flag: set when attributes are modified via set()
    /// Used for cache invalidation instead of recomputing hashes
    /// Thread-safe AtomicBool for Send+Sync (allows background composition)
    #[serde(skip)]
    #[serde(default = "Attrs::default_dirty")]
    dirty: AtomicBool,
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
        }
    }

    /// Set attribute value and mark as dirty
    pub fn set(&mut self, key: impl Into<String>, value: AttrValue) {
        self.map.insert(key.into(), value);
        self.dirty.store(true, Ordering::Relaxed); // Mark as dirty for cache invalidation
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
            if let Some(ref inc) = include_set {
                if !inc.contains(key.as_str()) {
                    continue;
                }
            }
            if let Some(ref exc) = exclude_set {
                if exc.contains(key.as_str()) {
                    continue;
                }
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
        }
    }
}
