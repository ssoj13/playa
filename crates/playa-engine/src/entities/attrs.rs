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

use bitflags::bitflags;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use uuid::Uuid;

use super::keys::{A_IN, A_SPEED, A_SRC_LEN, A_TRIM_IN, A_TRIM_OUT};
use playa_time::{Round, Speed};

// ============================================================================
// Attribute Schema System
// ============================================================================

/// Schema-definition flags (bitfield).
///
/// These describe an attribute *kind* at the schema level (how every instance of
/// that named attr behaves/displays). They are distinct from the per-instance
/// [`AttrFlags`] below, which tag an individual key on a concrete [`Attrs`] map
/// (e.g. provenance). Kept as a plain `u8` so the static `FLAG_*` composition in
/// `attr_schemas.rs` stays `const`.
pub type SchemaFlags = u8;

/// Attribute affects DAG/render - changes invalidate cache
pub const FLAG_DAG: SchemaFlags = 1 << 0;
/// Attribute shown in Attribute Editor UI
pub const FLAG_DISPLAY: SchemaFlags = 1 << 1;
/// Attribute can be keyframed for animation
pub const FLAG_KEYABLE: SchemaFlags = 1 << 2;
/// Attribute is read-only (computed value)
pub const FLAG_READONLY: SchemaFlags = 1 << 3;
/// Internal attribute, not shown to user
pub const FLAG_INTERNAL: SchemaFlags = 1 << 4;

bitflags! {
    /// Per-attribute (per-key) flags on a concrete [`Attrs`] map.
    ///
    /// Orthogonal to the schema-level [`SchemaFlags`]: these tag one specific key
    /// on one entity instance, carrying *provenance* and edit policy rather than
    /// the static definition of an attribute kind.
    ///
    /// - [`SOURCE`](AttrFlags::SOURCE): the value was absorbed from the media file
    ///   header (chromaticities, EXIF, container tags, …) — i.e. file-derived
    ///   provenance, as opposed to a user-authored attr.
    /// - [`READONLY`](AttrFlags::READONLY): the editor must not mutate this key.
    ///   Set deliberately by future UI; absorption does NOT imply read-only, so
    ///   absorbed attrs stay editable and round-trip on encode.
    ///
    /// Serialized as the raw `u8` bits (see manual `Serialize`/`Deserialize`) so
    /// the on-disk project format is stable regardless of flag naming.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct AttrFlags: u8 {
        /// Absorbed from the media file (provenance).
        const SOURCE = 1 << 0;
        /// Editor must not mutate this attr.
        const READONLY = 1 << 1;
    }
}

impl Serialize for AttrFlags {
    /// Serialize as the raw bits (`u8`) for a stable, name-independent format.
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u8(self.bits())
    }
}

impl<'de> Deserialize<'de> for AttrFlags {
    /// Deserialize from the raw bits; unknown bits are truncated so forward-saved
    /// projects (with extra flags) still load on older builds.
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let bits = u8::deserialize(d)?;
        Ok(AttrFlags::from_bits_truncate(bits))
    }
}

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
    Map,
    Set,
    Json,
}

/// Single attribute definition
#[derive(Debug, Clone)]
pub struct AttrDef {
    pub name: &'static str,
    pub attr_type: AttrType,
    pub flags: SchemaFlags,
    /// UI hints: combobox options or slider range ["min", "max", "step"]
    pub ui_options: &'static [&'static str],
    /// Display order in Attribute Editor (lower = higher in list)
    pub order: f32,
}

impl AttrDef {
    /// Create new attribute definition (default: auto UI by type, order=99)
    pub const fn new(name: &'static str, attr_type: AttrType, flags: SchemaFlags) -> Self {
        Self {
            name,
            attr_type,
            flags,
            ui_options: &[],
            order: 99.0,
        }
    }

    /// Create attribute with UI options (combobox values or slider range)
    pub const fn with_ui(
        name: &'static str,
        attr_type: AttrType,
        flags: SchemaFlags,
        ui_options: &'static [&'static str],
    ) -> Self {
        Self {
            name,
            attr_type,
            flags,
            ui_options,
            order: 99.0,
        }
    }

    /// Create attribute with order (for AE display sorting)
    pub const fn with_order(
        name: &'static str,
        attr_type: AttrType,
        flags: SchemaFlags,
        order: f32,
    ) -> Self {
        Self {
            name,
            attr_type,
            flags,
            ui_options: &[],
            order,
        }
    }

    /// Create attribute with UI options and order
    pub const fn with_ui_order(
        name: &'static str,
        attr_type: AttrType,
        flags: SchemaFlags,
        ui_options: &'static [&'static str],
        order: f32,
    ) -> Self {
        Self {
            name,
            attr_type,
            flags,
            ui_options,
            order,
        }
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
    defs: Box<[AttrDef]>,
}

impl AttrSchema {
    /// Create schema from static slice (clones into Box)
    pub fn new(name: &'static str, defs: &[AttrDef]) -> Self {
        Self {
            name,
            defs: defs.to_vec().into_boxed_slice(),
        }
    }

    /// Create schema by composing multiple slices (for DRY schemas)
    /// Example: `AttrSchema::from_slices("Layer", &[IDENTITY, TIMING, TRANSFORM])`
    pub fn from_slices(name: &'static str, slices: &[&[AttrDef]]) -> Self {
        let defs: Vec<AttrDef> = slices.iter().flat_map(|s| s.iter().cloned()).collect();
        Self {
            name,
            defs: defs.into_boxed_slice(),
        }
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AttrValue {
    Bool(bool),
    Str(String),
    Int8(i8),
    Int(i32),
    /// 64-bit integer. Carries source/EXR integer attrs whose u32 flag bits
    /// overflow i32 (EXR `TimeCode`/`KeyCode`); bridged from `playa_io::AttrKv`.
    Int64(i64),
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
    /// Nested map of values (string key -> value)
    Map(HashMap<String, AttrValue>),
    /// Unordered set of values
    Set(HashSet<AttrValue>),
    /// JSON-encoded nested data (HashMap, Vec, etc.)
    Json(String),
}

impl std::hash::Hash for AttrValue {
    fn hash<H: Hasher>(&self, state: &mut H) {
        use AttrValue::*;
        use std::collections::hash_map::DefaultHasher;
        std::mem::discriminant(self).hash(state);
        match self {
            Bool(v) => v.hash(state),
            Str(v) => v.hash(state),
            Int8(v) => v.hash(state),
            Int(v) => v.hash(state),
            Int64(v) => v.hash(state),
            UInt(v) => v.hash(state),
            Float(v) => v.to_bits().hash(state),
            Vec3(arr) => arr.iter().for_each(|f| f.to_bits().hash(state)),
            Vec4(arr) => arr.iter().for_each(|f| f.to_bits().hash(state)),
            Mat3(m) => m
                .iter()
                .flat_map(|r| r.iter())
                .for_each(|f| f.to_bits().hash(state)),
            Mat4(m) => m
                .iter()
                .flat_map(|r| r.iter())
                .for_each(|f| f.to_bits().hash(state)),
            Uuid(v) => v.hash(state),
            List(v) => v.hash(state),
            Map(v) => {
                let mut acc: u64 = 0;
                for (k, val) in v {
                    let mut h = DefaultHasher::new();
                    k.hash(&mut h);
                    val.hash(&mut h);
                    acc ^= h.finish();
                }
                acc.hash(state);
            }
            Set(v) => {
                let mut acc: u64 = 0;
                for val in v {
                    let mut h = DefaultHasher::new();
                    val.hash(&mut h);
                    acc ^= h.finish();
                }
                acc.hash(state);
            }
            Json(v) => v.hash(state),
        }
    }
}

fn f32_bits_eq(a: f32, b: f32) -> bool {
    a.to_bits() == b.to_bits()
}

fn f32_slice_bits_eq(a: &[f32], b: &[f32]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b.iter()).all(|(x, y)| f32_bits_eq(*x, *y))
}

impl PartialEq for AttrValue {
    fn eq(&self, other: &Self) -> bool {
        use AttrValue::*;
        match (self, other) {
            (Bool(a), Bool(b)) => a == b,
            (Str(a), Str(b)) => a == b,
            (Int8(a), Int8(b)) => a == b,
            (Int(a), Int(b)) => a == b,
            (Int64(a), Int64(b)) => a == b,
            (UInt(a), UInt(b)) => a == b,
            (Float(a), Float(b)) => f32_bits_eq(*a, *b),
            (Vec3(a), Vec3(b)) => f32_slice_bits_eq(a, b),
            (Vec4(a), Vec4(b)) => f32_slice_bits_eq(a, b),
            (Mat3(a), Mat3(b)) => a
                .iter()
                .zip(b.iter())
                .all(|(ra, rb)| f32_slice_bits_eq(ra, rb)),
            (Mat4(a), Mat4(b)) => a
                .iter()
                .zip(b.iter())
                .all(|(ra, rb)| f32_slice_bits_eq(ra, rb)),
            (Uuid(a), Uuid(b)) => a == b,
            (List(a), List(b)) => a == b,
            (Map(a), Map(b)) => {
                if a.len() != b.len() {
                    return false;
                }
                a.iter().all(|(k, v)| b.get(k).is_some_and(|ov| ov == v))
            }
            (Set(a), Set(b)) => a == b,
            (Json(a), Json(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for AttrValue {}

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

    /// Per-key flags (provenance / edit policy). Sparse: only keys that carry a
    /// non-empty flag set appear here. `#[serde(default)]` so projects saved
    /// before this field existed still load (every key reads back as empty).
    #[serde(default)]
    flags: HashMap<String, AttrFlags>,

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
            flags: HashMap::new(),
            dirty: AtomicBool::new(false),
            schema: None,
        }
    }

    /// Create Attrs with schema for automatic DAG detection
    pub fn with_schema(schema: &'static AttrSchema) -> Self {
        Self {
            map: HashMap::new(),
            flags: HashMap::new(),
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

    /// Merge every key/value pair from `other` into self, overwriting on key
    /// collision. Absorbs a source file's full metadata header into a node
    /// without dropping any of it. Routes through `set` so dirty-tracking and
    /// schema rules apply uniformly to the merged-in keys.
    pub fn merge(&mut self, other: Attrs) {
        for (k, v) in other.map {
            self.set(k, v);
        }
        // Carry provenance/policy: union `other`'s per-key flags onto ours so an
        // absorbed source header keeps its SOURCE marks after the merge.
        for (k, f) in other.flags {
            self.add_flags(k, f);
        }
    }

    // === Per-key flags (provenance / edit policy) ===

    /// Replace the flag set for `key` (overwrites any existing flags). Use
    /// [`add_flags`](Self::add_flags) to union instead.
    pub fn set_flags(&mut self, key: impl Into<String>, flags: AttrFlags) {
        self.flags.insert(key.into(), flags);
    }

    /// Union `flags` into `key`'s existing flag set (creating it if absent).
    /// This is the absorption-friendly setter: tagging SOURCE never clears a
    /// previously set READONLY and vice-versa.
    pub fn add_flags(&mut self, key: impl Into<String>, flags: AttrFlags) {
        self.flags.entry(key.into()).or_default().insert(flags);
    }

    /// Flags for `key` (empty set if the key carries none).
    pub fn flags(&self, key: &str) -> AttrFlags {
        self.flags.get(key).copied().unwrap_or_default()
    }

    /// True if `key` was absorbed from the source media header (provenance).
    pub fn is_source(&self, key: &str) -> bool {
        self.flags(key).contains(AttrFlags::SOURCE)
    }

    /// True if `key` is marked read-only (editor must not mutate it).
    pub fn is_readonly(&self, key: &str) -> bool {
        self.flags(key).contains(AttrFlags::READONLY)
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
        self.set(key, AttrValue::Int8(value));
    }

    pub fn get_uuid(&self, key: &str) -> Option<Uuid> {
        match self.map.get(key) {
            Some(AttrValue::Uuid(v)) => Some(*v),
            _ => None,
        }
    }

    pub fn set_uuid(&mut self, key: impl Into<String>, value: Uuid) {
        self.set(key, AttrValue::Uuid(value));
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

    pub fn get_map(&self, key: &str) -> Option<&HashMap<String, AttrValue>> {
        match self.map.get(key) {
            Some(AttrValue::Map(v)) => Some(v),
            _ => None,
        }
    }

    pub fn get_map_mut(&mut self, key: &str) -> Option<&mut HashMap<String, AttrValue>> {
        match self.map.get_mut(key) {
            Some(AttrValue::Map(v)) => Some(v),
            _ => None,
        }
    }

    pub fn set_map(&mut self, key: impl Into<String>, value: HashMap<String, AttrValue>) {
        self.set(key, AttrValue::Map(value));
    }

    pub fn get_set(&self, key: &str) -> Option<&HashSet<AttrValue>> {
        match self.map.get(key) {
            Some(AttrValue::Set(v)) => Some(v),
            _ => None,
        }
    }

    pub fn get_set_mut(&mut self, key: &str) -> Option<&mut HashSet<AttrValue>> {
        match self.map.get_mut(key) {
            Some(AttrValue::Set(v)) => Some(v),
            _ => None,
        }
    }

    pub fn set_set(&mut self, key: impl Into<String>, value: HashSet<AttrValue>) {
        self.set(key, AttrValue::Set(value));
    }

    pub fn get_uuid_list(&self, key: &str) -> Option<Vec<Uuid>> {
        let list = self.get_list(key)?;
        let mut out = Vec::with_capacity(list.len());
        for v in list {
            match v {
                AttrValue::Uuid(id) => out.push(*id),
                _ => return None,
            }
        }
        Some(out)
    }

    pub fn set_uuid_list(&mut self, key: impl Into<String>, values: &[Uuid]) {
        let list = values.iter().copied().map(AttrValue::Uuid).collect();
        self.set(key, AttrValue::List(list));
    }

    pub fn set_list(&mut self, key: impl Into<String>, value: Vec<AttrValue>) {
        self.set(key, AttrValue::List(value));
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

    /// Layer visible start in parent coords: `in + trim_in/speed`.
    /// `trim_in` is a source-frames offset, converted to timeline frames via speed.
    pub fn layer_start(&self) -> i32 {
        let in_val = self.get_i32_or_zero(A_IN);
        let trim_in = self.get_i32_or_zero(A_TRIM_IN);
        let speed = Speed::new(self.get_float_or(A_SPEED, 1.0));
        let offset = speed.scale_src_to_timeline(trim_in, Round::Round);
        in_val.saturating_add(offset)
    }

    /// Layer visible end in parent coords: `layer_start + visible_timeline_frames - 1`,
    /// where `visible_src_frames = src_len - trim_in - trim_out` and
    /// `visible_timeline_frames = visible_src_frames / speed` (rounded).
    pub fn layer_end(&self) -> i32 {
        let layer_start = self.layer_start();
        let src_len = self.get_i32_or_zero(A_SRC_LEN);
        let trim_in = self.get_i32_or_zero(A_TRIM_IN);
        let trim_out = self.get_i32_or_zero(A_TRIM_OUT);
        // Visible source frames (at least 1 to prevent negative duration).
        let visible_src = (src_len - trim_in - trim_out).max(1);
        let speed = Speed::new(self.get_float_or(A_SPEED, 1.0));
        let visible_timeline = speed
            .scale_src_to_timeline(visible_src, Round::Round)
            .max(1);
        layer_start
            .saturating_add(visible_timeline)
            .saturating_sub(1)
    }

    /// Get source length (original duration in source frames)
    pub fn src_len(&self) -> i32 {
        self.get_i32_or_zero(A_SRC_LEN)
    }

    /// Full bar end (untrimmed): `in + src_len/speed - 1`. Computed, not stored —
    /// this replaces the legacy persisted `"out"` attribute on layers.
    ///
    /// Uses [`Round::Ceil`] (not `Round::Round`) so the bar always covers every
    /// source frame at least once on non-integer `speed`; matches the prior
    /// behaviour at this call site.
    pub fn full_bar_end(&self) -> i32 {
        let in_val = self.get_i32_or_zero(A_IN);
        let src_len = self.get_i32_or_zero(A_SRC_LEN);
        let speed = Speed::new(self.get_float_or(A_SPEED, 1.0));
        let duration = speed.scale_src_to_timeline(src_len, Round::Ceil).max(1);
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

    /// Remove attribute by key. Also drops the key's flag entry so stale
    /// provenance never lingers after the value is gone.
    pub fn remove(&mut self, key: &str) -> Option<AttrValue> {
        self.flags.remove(key);
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
                && !inc.contains(key.as_str())
            {
                continue;
            }
            if let Some(ref exc) = exclude_set
                && exc.contains(key.as_str())
            {
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
            self.set(key, AttrValue::Json(json));
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
            flags: self.flags.clone(),
            dirty: AtomicBool::new(self.dirty.load(Ordering::Relaxed)),
            schema: self.schema,
        }
    }
}

#[cfg(test)]
mod flag_tests {
    use super::*;

    #[test]
    fn attrflags_serde_roundtrips_as_bits() {
        let f = AttrFlags::SOURCE | AttrFlags::READONLY;
        let json = serde_json::to_string(&f).unwrap();
        // Serialized as the raw u8 bits, not a name list.
        assert_eq!(json, "3");
        let back: AttrFlags = serde_json::from_str(&json).unwrap();
        assert_eq!(back, f);
    }

    #[test]
    fn attrflags_empty_serde() {
        let f = AttrFlags::empty();
        let json = serde_json::to_string(&f).unwrap();
        assert_eq!(json, "0");
        assert_eq!(serde_json::from_str::<AttrFlags>(&json).unwrap(), f);
    }

    #[test]
    fn set_preserves_existing_flags() {
        let mut a = Attrs::new();
        a.set("k", AttrValue::Int(1));
        a.add_flags("k", AttrFlags::SOURCE);
        // Editing the value must NOT wipe provenance.
        a.set("k", AttrValue::Int(2));
        assert!(a.is_source("k"));
        assert_eq!(a.get_i32("k"), Some(2));
    }

    #[test]
    fn get_mut_preserves_flags() {
        let mut a = Attrs::new();
        a.set("k", AttrValue::Int(1));
        a.add_flags("k", AttrFlags::SOURCE);
        if let Some(AttrValue::Int(v)) = a.get_mut("k") {
            *v = 9;
        }
        assert!(a.is_source("k"));
    }

    #[test]
    fn add_flags_unions_not_overwrites() {
        let mut a = Attrs::new();
        a.add_flags("k", AttrFlags::SOURCE);
        a.add_flags("k", AttrFlags::READONLY);
        assert!(a.is_source("k"));
        assert!(a.is_readonly("k"));
    }

    #[test]
    fn merge_carries_flags() {
        let mut src = Attrs::new();
        src.set("exr:owner", AttrValue::Str("vfx".into()));
        src.add_flags("exr:owner", AttrFlags::SOURCE);

        let mut dst = Attrs::new();
        dst.merge(src);
        assert!(dst.is_source("exr:owner"));
        assert_eq!(dst.get_str("exr:owner"), Some("vfx"));
    }

    #[test]
    fn remove_drops_flags() {
        let mut a = Attrs::new();
        a.set("k", AttrValue::Int(1));
        a.add_flags("k", AttrFlags::SOURCE | AttrFlags::READONLY);
        a.remove("k");
        assert_eq!(a.flags("k"), AttrFlags::empty());
        assert!(!a.is_source("k"));
    }

    #[test]
    fn attrs_serde_roundtrips_flags() {
        let mut a = Attrs::new();
        a.set("exr:owner", AttrValue::Str("vfx".into()));
        a.add_flags("exr:owner", AttrFlags::SOURCE);
        let json = serde_json::to_string(&a).unwrap();
        let back: Attrs = serde_json::from_str(&json).unwrap();
        assert!(back.is_source("exr:owner"));
    }
}
