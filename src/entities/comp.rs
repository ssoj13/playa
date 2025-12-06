//! Composition-level types (timeline unit for playback/encoding).
//!
//! `Comp` is now a unified entity that can work in two modes:
//! - Layer mode: composes children comps
//! - File mode: loads image sequence from disk (ex-Clip functionality)
//! Used by: timeline rendering (`widgets::timeline`), encoding (`dialogs::encode`),
//! playback (`player.rs`), and project serialization. Data flow: UI emits events →
//! `Comp` mutates attrs/children → cached frames/computed hashes drive compositor
//! work and encoding output.
//! Cache & hash notes:
//! - `compute_comp_hash` hashes mode, file params, child order, and full child Attrs
//!   (`Attrs::hash_all`), plus select transform attrs. Any child attr change produces
//!   a new hash, forcing cache miss and recomposition on next `get_frame`/`compose`.
//! - Composed frames cache keys: `(compute_comp_hash(), frame_idx)`. File comps also
//!   include sequence frame number in the key for stability when numbering shifts.
//! - Layer comps recurse into children via `get_frame`; child Layer comps compose
//!   their children the same way, so attr changes propagate through hashes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use half::f16;
use glob::glob;
use log::{debug, info};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::core::cache_man::CacheManager;
use crate::core::workers::Workers;

use super::frame::{CropAlign, Frame, FrameError, FrameStatus, PixelBuffer, PixelDepth, PixelFormat};
use super::loader::Loader;
use super::{AttrValue, Attrs};
use super::keys::*;
use super::compositor::BlendMode;
use crate::entities::loader_video;
use crate::core::event_bus::CompEventEmitter;
use crate::entities::comp_events::{LayersChangedEvent, CurrentFrameChangedEvent, AttrsChangedEvent};
use crate::utils::media;

use std::cell::RefCell;
use super::compositor::CpuCompositor;

// Thread-local CPU compositor for background composition
// Each worker thread gets its own compositor instance (zero allocation after init)
// GPU compositor remains in Project.compositor (main thread only)
thread_local! {
    static THREAD_COMPOSITOR: RefCell<CpuCompositor> = RefCell::new(CpuCompositor);
}

/// Unified composition descriptor with dual-mode operation.
///
/// **Mode stored in attrs as A_MODE (i8)**:
/// - COMP_NORMAL (0): Composes children comps recursively
/// - COMP_FILE (1): Loads image sequence from disk
///
/// All properties stored in `core.attrs`:
/// - Identity: uuid, name, mode
/// - Timeline: in, out, trim_in, trim_out, fps, frame
/// - Compose: solo, mute, visible, opacity, blend_mode
/// - Transform: position, rotation, scale, pivot
/// - Playback: speed
/// - Relationships: source_uuid, parent
/// - File mode: file_mask, file_start, file_end
/// - Dimensions: width, height
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Comp {
    /// Persistent attributes (all properties)
    pub attrs: Attrs,

    /// Transient runtime data (not serialized)
    #[serde(skip, default)]
    pub data: Attrs,

    /// Children layers - each child is full Attrs with uuid, source_uuid, timing, etc.
    /// Legacy format: Vec<(Uuid, Attrs)> - auto-converted on load
    #[serde(default)]
    pub children: Vec<(Uuid, Attrs)>,

    // === Runtime-only fields ===
    #[serde(default)]
    pub layer_selection: Vec<Uuid>,
    #[serde(default)]
    pub layer_selection_anchor: Option<Uuid>,

    #[serde(skip, default)]
    event_emitter: CompEventEmitter,

    #[serde(skip)]
    cache_manager: Option<Arc<CacheManager>>,

    #[serde(skip)]
    global_cache: Option<Arc<crate::core::global_cache::GlobalFrameCache>>,
}

impl Default for Comp {
    fn default() -> Self {
        Self::new("Untitled", 0, 100, 24.0)
    }
}

impl Comp {
    /// Create comp with ALL attributes (unified schema).
    /// Mode determines get_frame() behavior: COMP_NORMAL -> compose(), COMP_FILE -> load()
    /// All attributes always present, unused ones have default/nil values.
    pub fn new_comp(name: &str, start: i32, end: i32, fps: f32) -> Self {
        let mut attrs = Attrs::new();
        let uuid = Uuid::new_v4();

        // === Identity ===
        attrs.set_uuid(A_UUID, uuid);
        attrs.set(A_NAME, AttrValue::Str(name.to_string()));
        attrs.set_i8(A_MODE, COMP_NORMAL); // Default: layer composition

        // === Timeline ===
        attrs.set(A_IN, AttrValue::Int(start));
        attrs.set(A_OUT, AttrValue::Int(end));
        attrs.set(A_TRIM_IN, AttrValue::Int(start));
        attrs.set(A_TRIM_OUT, AttrValue::Int(end));
        attrs.set(A_FPS, AttrValue::Float(fps));
        attrs.set(A_FRAME, AttrValue::Int(start));

        // === Compose flags ===
        attrs.set(A_SOLO, AttrValue::Bool(false));
        attrs.set(A_MUTE, AttrValue::Bool(false));
        attrs.set(A_VISIBLE, AttrValue::Bool(true));
        attrs.set(A_OPACITY, AttrValue::Float(1.0));
        attrs.set(A_BLEND_MODE, AttrValue::Str("normal".to_string()));
        // Legacy alias
        attrs.set("transparency", AttrValue::Float(1.0));
        attrs.set("layer_mode", AttrValue::Str("normal".to_string()));

        // === Transform ===
        attrs.set(A_POSITION, AttrValue::Vec3([0.0, 0.0, 0.0]));
        attrs.set(A_ROTATION, AttrValue::Vec3([0.0, 0.0, 0.0]));
        attrs.set(A_SCALE, AttrValue::Vec3([1.0, 1.0, 1.0]));
        attrs.set(A_PIVOT, AttrValue::Vec3([0.0, 0.0, 0.0]));

        // === Playback ===
        attrs.set(A_SPEED, AttrValue::Float(1.0));

        // === Relationships (always present, nil when unused) ===
        attrs.set_uuid(A_SOURCE_UUID, Uuid::nil()); // nil = no source
        attrs.set_uuid(A_PARENT, Uuid::nil()); // nil = root level

        // === File mode attrs (always present, empty when unused) ===
        attrs.set(A_FILE_MASK, AttrValue::Str(String::new())); // empty = no file
        attrs.set(A_FILE_START, AttrValue::Int(0));
        attrs.set(A_FILE_END, AttrValue::Int(0));

        // === Dimensions (0 = auto-detect from content) ===
        attrs.set(A_WIDTH, AttrValue::Int(0));
        attrs.set(A_HEIGHT, AttrValue::Int(0));

        Self {
            attrs,
            data: Attrs::new(),
            children: Vec::new(),
            layer_selection: Vec::new(),
            layer_selection_anchor: None,
            event_emitter: CompEventEmitter::dummy(),
            cache_manager: None,
            global_cache: None,
        }
    }

    /// Legacy constructor - calls new_comp internally
    pub fn new(name: impl Into<String>, start: i32, end: i32, fps: f32) -> Self {
        Self::new_comp(&name.into(), start, end, fps)
    }

    /// Create new composition in File mode for loading image sequences
    pub fn new_file_comp(pattern: impl Into<String>, start: i32, end: i32, fps: f32) -> Self {
        let pattern_str = pattern.into();
        let mut comp = Self::new_comp("File Comp", start, end, fps);

        // Set mode to file
        comp.attrs.set_i8(A_MODE, COMP_FILE);

        // Set file attrs
        comp.attrs.set(A_FILE_MASK, AttrValue::Str(pattern_str));
        comp.attrs.set(A_FILE_START, AttrValue::Int(start));
        comp.attrs.set(A_FILE_END, AttrValue::Int(end));

        comp
    }

    // Getters for attrs-based properties
    pub fn name(&self) -> &str {
        self.attrs.get_str("name").unwrap_or("Untitled")
    }

    pub fn get_uuid(&self) -> Uuid {
        self.attrs.get_uuid(A_UUID).unwrap_or(Uuid::nil())
    }

    pub fn file_start(&self) -> Option<i32> {
        self.attrs.get_i32(A_FILE_START)
    }

    pub fn file_end(&self) -> Option<i32> {
        self.attrs.get_i32(A_FILE_END)
    }

    pub fn file_mask(&self) -> Option<String> {
        self.attrs.get_str(A_FILE_MASK).map(|s| s.to_string())
    }

    pub fn get_parent(&self) -> Option<Uuid> {
        self.attrs.get_uuid(A_PARENT)
    }

    pub fn set_parent(&mut self, parent_uuid: Option<Uuid>) {
        if let Some(uuid) = parent_uuid {
            self.attrs.set_uuid(A_PARENT, uuid);
        } else {
            self.attrs.remove(A_PARENT);
        }
    }

    // NOTE: Methods use underscore prefix (_in/_out) because `in` is a Rust keyword
    pub fn _in(&self) -> i32 {
        self.attrs.get_i32("in").unwrap_or(0)
    }

    pub fn _out(&self) -> i32 {
        self.attrs.get_i32("out").unwrap_or(100)
    }

    pub fn fps(&self) -> f32 {
        self.attrs.get_float("fps").unwrap_or(24.0)
    }

    pub fn trim_in(&self) -> i32 {
        self.attrs.get_i32("trim_in").unwrap_or_else(|| self._in())
    }

    pub fn trim_out(&self) -> i32 {
        self.attrs.get_i32("trim_out").unwrap_or_else(|| self._out())
    }

    // Setters for attrs-based properties
    pub fn set_name(&mut self, name: impl Into<String>) {
        self.attrs.set("name", AttrValue::Str(name.into()));
    }

    pub fn set_in(&mut self, val: i32) {
        self.attrs.set("in", AttrValue::Int(val));
    }

    pub fn set_out(&mut self, val: i32) {
        self.attrs.set("out", AttrValue::Int(val));
    }

    // ===== Children accessor methods =====

    /// Get immutable reference to child attrs by UUID
    pub fn children_attrs_get(&self, uuid: &Uuid) -> Option<&Attrs> {
        self.children.iter().find(|(u, _)| u == uuid).map(|(_, a)| a)
    }

    /// Get mutable reference to child attrs by UUID
    pub fn children_attrs_get_mut(&mut self, uuid: &Uuid) -> Option<&mut Attrs> {
        self.children.iter_mut().find(|(u, _)| u == uuid).map(|(_, a)| a)
    }

    /// Insert or update child attrs
    pub fn children_attrs_insert(&mut self, uuid: Uuid, attrs: Attrs) {
        if let Some(existing) = self.children.iter_mut().find(|(u, _)| *u == uuid) {
            existing.1 = attrs;
        } else {
            self.children.push((uuid, attrs));
        }
    }

    /// Remove child attrs and return it
    pub fn children_attrs_remove(&mut self, uuid: &Uuid) -> Option<Attrs> {
        if let Some(pos) = self.children.iter().position(|(u, _)| u == uuid) {
            Some(self.children.remove(pos).1)
        } else {
            None
        }
    }

    /// Get child UUIDs iterator
    pub fn children_uuids(&self) -> impl Iterator<Item = &Uuid> {
        self.children.iter().map(|(u, _)| u)
    }

    /// Get child UUIDs as vec (cloned)
    pub fn children_uuids_vec(&self) -> Vec<Uuid> {
        self.children.iter().map(|(u, _)| *u).collect()
    }

    /// Get number of children
    pub fn children_len(&self) -> usize {
        self.children.len()
    }

    /// Check if children is empty
    pub fn children_is_empty(&self) -> bool {
        self.children.is_empty()
    }

    /// Check if contains child UUID
    pub fn children_contains(&self, uuid: &Uuid) -> bool {
        self.children.iter().any(|(u, _)| u == uuid)
    }

    /// Get child at index (uuid, attrs)
    pub fn children_get(&self, idx: usize) -> Option<&(Uuid, Attrs)> {
        self.children.get(idx)
    }

    /// Get child UUID at index
    pub fn children_uuid_at(&self, idx: usize) -> Option<Uuid> {
        self.children.get(idx).map(|(u, _)| *u)
    }

    // Layer UUID <-> Index conversion helpers
    /// Convert layer UUID to index in children array
    pub fn uuid_to_idx(&self, uuid: Uuid) -> Option<usize> {
        self.children.iter().position(|(u, _)| *u == uuid)
    }

    /// Convert index to layer UUID
    pub fn idx_to_uuid(&self, idx: usize) -> Option<Uuid> {
        self.children.get(idx).map(|(u, _)| *u)
    }

    /// Convert multiple UUIDs to indices
    pub fn uuids_to_indices(&self, uuids: &[Uuid]) -> Vec<usize> {
        uuids.iter()
            .filter_map(|&uuid| self.uuid_to_idx(uuid))
            .collect()
    }

    /// Convert multiple indices to UUIDs
    pub fn indices_to_uuids(&self, indices: &[usize]) -> Vec<Uuid> {
        indices.iter()
            .filter_map(|&idx| self.idx_to_uuid(idx))
            .collect()
    }

    // Domain-specific helpers for layer attributes

    /// Get child layer's start position
    pub fn child_start(&self, child_uuid: Uuid) -> i32 {
        self.children_attrs_get(&child_uuid)
            .map(|a| a.get_i32_or_zero("in"))
            .unwrap_or(0)
    }

    /// Get child layer's end position
    pub fn child_end(&self, child_uuid: Uuid) -> i32 {
        self.children_attrs_get(&child_uuid)
            .map(|a| a.get_i32_or_zero("out"))
            .unwrap_or(0)
    }

    /// Get child layer's play_start (with fallback to start)
    pub fn child_play_start(&self, child_uuid: Uuid) -> i32 {
        self.children_attrs_get(&child_uuid)
            .map(|a| {
                let start = a.get_i32_or_zero("in");
                a.get_i32_or("trim_in", start)
            })
            .unwrap_or(0)
    }

    /// Get child layer's play_end (with fallback to end)
    pub fn child_play_end(&self, child_uuid: Uuid) -> i32 {
        self.children_attrs_get(&child_uuid)
            .map(|a| {
                let end = a.get_i32_or_zero("out");
                a.get_i32_or("trim_out", end)
            })
            .unwrap_or(0)
    }

    /// Check if a specific layer is in selection
    pub fn is_layer_selected(&self, layer_uuid: Uuid) -> bool {
        self.layer_selection.contains(&layer_uuid)
    }

    /// Check if layer is selected and part of multi-selection
    pub fn is_multi_selected(&self, layer_uuid: Uuid) -> bool {
        !self.layer_selection.is_empty()
            && self.is_layer_selected(layer_uuid)
            && self.layer_selection.len() > 1
    }

    pub fn set_fps(&mut self, fps: f32) {
        self.attrs.set("fps", AttrValue::Float(fps));
    }

    pub fn set_trim_in(&mut self, play_start: i32) {
        self.attrs.set("trim_in", AttrValue::Int(play_start));
    }

    pub fn set_trim_out(&mut self, play_end: i32) {
        self.attrs.set("trim_out", AttrValue::Int(play_end));
    }

    /// Inclusive play range (work area) in absolute comp frames.
    ///
    /// Returns [start, end] inclusive in timeline coordinates.
    pub fn play_range(&self, use_work_area: bool) -> (i32, i32) {
        // If comp has children (Layer mode), calculate bounds from them (with their trims)
        if !self.children.is_empty() {
            let mut min_frame = i32::MAX;
            let mut max_frame = i32::MIN;

            for (_child_uuid, attrs) in &self.children {
                let child_start = attrs.get_i32("in").unwrap_or(0);
                let child_end = attrs.get_i32("out").unwrap_or(child_start);
                let child_play_start = attrs.get_i32("trim_in").unwrap_or(child_start);
                let child_play_end = attrs.get_i32("trim_out").unwrap_or(child_end);

                min_frame = min_frame.min(child_play_start);
                max_frame = max_frame.max(child_play_end);
            }

            if min_frame != i32::MAX && max_frame != i32::MIN {
                let (base_start, base_end) = (min_frame, max_frame);
                if use_work_area {
                    let work_start = self
                        .attrs
                        .get_i32("trim_in")
                        .unwrap_or(base_start)
                        .clamp(base_start, base_end);
                    let work_end = self
                        .attrs
                        .get_i32("trim_out")
                        .unwrap_or(base_end)
                        .clamp(work_start, base_end);
                    return (work_start, work_end);
                }
                return (base_start, base_end);
            }
        }

        // Fallback: File mode comp or no children - use comp's own range
        self.work_area_abs(use_work_area)
    }

    /// Comp-level work area in absolute frames, clamped to comp bounds.
    /// If `use_work_area` is false, returns full comp bounds.
    pub fn work_area_abs(&self, use_work_area: bool) -> (i32, i32) {
        let start = self._in();
        let end = self._out();
        if !use_work_area {
            return (start, end);
        }

        let work_start = self.attrs.get_i32("trim_in").unwrap_or(start);
        let work_end = self.attrs.get_i32("trim_out").unwrap_or(end);
        let clamped_start = work_start.clamp(start, end);
        let clamped_end = work_end.clamp(clamped_start, end);
        (clamped_start, clamped_end)
    }

    /// Set comp work area in absolute frames. Automatically clamps/order-fixes.
    pub fn set_work_area_abs(&mut self, start: i32, end: i32) {
        let (comp_start, comp_end) = (self._in(), self._out());
        let lo = start.min(end).clamp(comp_start, comp_end);
        let hi = end.max(start).clamp(lo, comp_end);
        self.set_trim_in(lo);
        self.set_trim_out(hi);
        // Don't clear cache - already loaded frames remain valid
        // Preload will automatically load new frames in the updated work area
        self.event_emitter.emit(LayersChangedEvent {
            comp_uuid: self.get_uuid(),
            affected_range: None,
        });
    }

    /// Child placement bounds (start/end) in parent timeline, ordered and clamped if needed.
    fn child_bounds_abs(start_attr: Option<i32>, end_attr: Option<i32>) -> (i32, i32) {
        let s = start_attr.unwrap_or(0);
        let e = end_attr.unwrap_or(s);
        if e < s {
            (e, s)
        } else {
            (s, e)
        }
    }

    /// Child work area in parent timeline (absolute). Defaults to full bounds.
    pub fn child_work_area_abs(&self, child_uuid: Uuid) -> Option<(i32, i32)> {
        let attrs = self.children_attrs_get(&child_uuid)?;
        let (bounds_start, bounds_end) =
            Self::child_bounds_abs(attrs.get_i32("in"), attrs.get_i32("out"));
        let play_start = attrs.get_i32("trim_in").unwrap_or(bounds_start);
        let play_end = attrs.get_i32("trim_out").unwrap_or(bounds_end);
        Some(Self::clamp_range_to_bounds(
            (play_start, play_end),
            (bounds_start, bounds_end),
        ))
    }

    /// Clamp range to bounds and ensure ordering.
    fn clamp_range_to_bounds(
        (start, end): (i32, i32),
        (bounds_start, bounds_end): (i32, i32),
    ) -> (i32, i32) {
        let ordered_start = start.min(end);
        let ordered_end = end.max(start);
        let clamped_start = ordered_start.clamp(bounds_start, bounds_end);
        let clamped_end = ordered_end.clamp(clamped_start, bounds_end);
        (clamped_start, clamped_end)
    }

    // ===== Time Conversion Methods =====

    /// Convert parent comp frame to child's local frame.
    ///
    /// Takes into account:
    /// - child's start position in parent timeline
    /// - child's speed multiplier
    ///
    /// # Arguments
    /// * `child_uuid` - UUID of child layer (instance UUID)
    /// * `comp_frame` - Frame number in parent comp timeline
    ///
    /// # Returns
    /// Local frame number in child's coordinate system, or None if child not found
    pub fn comp2local(&self, child_uuid: Uuid, comp_frame: i32) -> Option<i32> {
        let attrs = self.children_attrs_get(&child_uuid)?;
        let child_start = attrs.get_i32("in").unwrap_or(0);
        let speed = attrs.get_float("speed").unwrap_or(1.0);

        // Offset from child's start position
        let offset = comp_frame - child_start;

        // Apply speed (AE-style: speed=2 means clip plays 2x faster)
        // local = offset * speed
        let local_frame = (offset as f32 * speed).round() as i32;

        Some(local_frame)
    }

    /// Convert child's local frame to parent comp frame.
    ///
    /// Inverse of comp2local().
    ///
    /// # Arguments
    /// * `child_uuid` - UUID of child layer (instance UUID)
    /// * `local_frame` - Frame number in child's local timeline
    ///
    /// # Returns
    /// Frame number in parent comp timeline, or None if child not found
    pub fn local2comp(&self, child_uuid: Uuid, local_frame: i32) -> Option<i32> {
        let attrs = self.children_attrs_get(&child_uuid)?;
        let child_start = attrs.get_i32("in").unwrap_or(0);
        let speed = attrs.get_float("speed").unwrap_or(1.0);

        // Inverse of comp2local (AE-style)
        // comp = in + local / speed
        if speed.abs() < 0.0001 {
            return Some(child_start); // Avoid division by zero
        }
        let comp_frame = child_start + (local_frame as f32 / speed).round() as i32;

        Some(comp_frame)
    }

    /// Get source comp's frame for given parent frame.
    ///
    /// Combines comp2local with source comp lookup.
    /// Used in compose() for recursive frame fetching.
    ///
    /// # Returns
    /// (source_uuid, source_frame) or None if child not found
    pub fn resolve_source_frame(
        &self,
        child_uuid: Uuid,
        comp_frame: i32,
        project: &super::Project,
    ) -> Option<(Uuid, i32)> {
        let attrs = self.children_attrs_get(&child_uuid)?;

        // Get source UUID
        let source_uuid_str = attrs.get_str("uuid")?;
        let source_uuid = Uuid::parse_str(source_uuid_str).ok()?;

        // Get source comp to find its start
        let source = project.get_comp(source_uuid)?;

        // Convert to local frame
        let local_frame = self.comp2local(child_uuid, comp_frame)?;

        // Map to source comp's timeline
        let source_frame = source._in() + local_frame;

        Some((source_uuid, source_frame))
    }

    /// Number of frames in full composition (not limited by play_area)
    pub fn frame_count(&self) -> i32 {
        let start = self._in();
        let end = self._out();
        if end >= start { end - start + 1 } else { 0 }
    }

    /// Number of frames in play range (work area)
    pub fn play_frame_count(&self) -> i32 {
        let (visible_start, visible_end) = self.play_range(true);
        if visible_end >= visible_start {
            visible_end - visible_start + 1
        } else {
            0
        }
    }

    /// Return cached frame statuses aligned to comp timeline.
    ///
    /// Works for BOTH File and Layer mode comps:
    /// - File mode: shows which image sequence frames are loaded
    /// - Layer mode: shows which composed frames are cached
    ///
    /// Status colors:
    /// - Header (blue): not yet loaded/rendered
    /// - Loading (orange): currently loading
    /// - Loaded (green): in cache
    /// - Error (red): failed to load/render
    pub fn cache_frame_statuses(&self) -> Option<Vec<FrameStatus>> {
        let duration = self.frame_count();
        if duration <= 0 {
            log::debug!("cache_frame_statuses: duration <= 0 (duration={})", duration);
            return None;
        }

        // Query GlobalFrameCache for real frame status (Header/Loading/Loaded/Error)
        if let Some(ref global_cache) = self.global_cache {
            // Lazy init: prefill cache with Header frames on first access
            if !global_cache.has_comp(self.get_uuid()) {
                self.prefill_cache();
            }

            let comp_start = self._in();
            let mut statuses = Vec::with_capacity(duration as usize);

            for frame_offset in 0..duration {
                let frame_idx = comp_start + frame_offset;

                // Get actual status from cache, or Placeholder if not cached yet
                let status = global_cache
                    .get_status(self.get_uuid(), frame_idx)
                    .unwrap_or(FrameStatus::Placeholder);
                statuses.push(status);
            }

            Some(statuses)
        } else {
            // Fallback if global_cache not available yet
            Some(vec![FrameStatus::Placeholder; duration as usize])
        }
    }

    /// Set global cache manager (called once after creation)
    pub fn set_cache_manager(&mut self, manager: Arc<CacheManager>) {
        self.cache_manager = Some(manager);
    }

    /// Set global frame cache (called once after creation)
    pub fn set_global_cache(&mut self, cache: Arc<crate::core::global_cache::GlobalFrameCache>) {
        self.global_cache = Some(cache);
    }

    /// Pre-fill cache with Header frames for entire work area (lazy init)
    ///
    /// Creates unloaded frames for all positions in the comp's work area.
    /// Called automatically on first access to cache_frame_statuses().
    /// Does NOT trigger loading - that's done by signal_preload().
    pub fn prefill_cache(&self) {
        let global_cache = match &self.global_cache {
            Some(cache) => cache,
            None => return,
        };

        // Skip if already prefilled
        if global_cache.has_comp(self.get_uuid()) {
            return;
        }

        let uuid = self.get_uuid();
        let comp_start = self._in();
        let duration = self.frame_count();
        if duration <= 0 {
            return;
        }

        // File mode: create Header frames with paths
        if self.is_file_mode() {
            let (w, h) = self.dim();
            let seq_start = self.file_start().unwrap_or(comp_start);

            for frame_offset in 0..duration {
                let frame_idx = comp_start + frame_offset;
                let seq_frame = seq_start.saturating_add(frame_offset);

                if let Some(path) = self.resolve_frame_path(seq_frame) {
                    let frame = Frame::new_unloaded(path);
                    frame.crop(w, h, CropAlign::LeftTop);
                    global_cache.insert(uuid, frame_idx, frame);
                }
            }

            debug!(
                "Prefilled cache for file comp {}: {} frames as Header",
                uuid, duration
            );
        } else {
            // Layer mode: create Placeholder frames (status Placeholder)
            // Layer frames are composed, not loaded - no path to set
            // We skip prefill for layer comps - compose happens on demand
            debug!(
                "Layer comp {} - skipping prefill (compose on demand)",
                uuid
            );
        }
    }

    /// Signal background preload for frames around current position
    ///
    /// Determines preload strategy (spiral vs forward) based on file type and enqueues frames.
    /// Does NOT increment epoch - let all frames load even during scrubbing.
    /// Cache eviction will handle memory limits automatically.
    ///
    /// Smart optimization: Skips preload if entire work area is already cached.
    ///
    /// Strategies:
    /// - Spiral: image sequences (0, ±1, ±2, ...) - cheap seeking both directions
    /// - Forward: video files (center → end) - expensive backward seeking
    pub fn signal_preload(
        &self,
        workers: &Arc<Workers>,
        project: &super::Project,
        center_override: Option<i32>,
    ) {
        // Get current epoch without incrementing (let frames load in background)
        let epoch = if let Some(ref manager) = self.cache_manager {
            manager.current_epoch()
        } else {
            return;
        };

        let center = center_override.unwrap_or(self.frame());
        let (play_start, play_end) = self.work_area_abs(true);

        // Debug: show coordinate spaces
        debug!(
            "signal_preload: current_frame={}, comp[{}..{}], file_start={:?}, work_area[{}..{}]",
            center,
            self._in(),
            self._out(),
            self.file_start(),
            play_start,
            play_end
        );

        if play_end < play_start {
            return;
        }

        // Layer mode: also preload children (file comps need their frames loaded)
        if !self.is_file_mode() {
            for (child_uuid, attrs) in &self.children {
                let source_uuid_str = match attrs.get_str("uuid") {
                    Some(s) => s,
                    None => continue,
                };
                let source_uuid = match Uuid::parse_str(source_uuid_str) {
                    Ok(u) => u,
                    Err(_) => continue,
                };

                // Get source comp and trigger its preload
                let source = {
                    let media = project.media.read().unwrap();
                    media.get(&source_uuid).cloned()
                };

                if let Some(source) = source {
                    // Calculate child's center frame based on parent's center
                    let child_center = self.comp2local(*child_uuid, center)
                        .map(|local| source._in() + local);
                    source.signal_preload(workers, project, child_center);
                }
            }
        }

        // Smart check: Skip preload if entire work area is already Loaded
        if let Some(ref global_cache) = self.global_cache {
            let mut all_loaded = true;
            for frame_idx in play_start..=play_end {
                match global_cache.get_status(self.get_uuid(), frame_idx) {
                    Some(FrameStatus::Loaded) => continue,
                    _ => {
                        all_loaded = false;
                        break;
                    }
                }
            }

            if all_loaded {
                debug!(
                    "Preload skipped: work area [{}..{}] fully loaded ({} frames)",
                    play_start, play_end, play_end - play_start + 1
                );
                return;
            }
        }

        // Determine strategy based on file type (video vs image sequence)
        let is_video = self.detect_video_at_frame(center);
        let strategy = if is_video {
            crate::core::cache_man::PreloadStrategy::Forward
        } else {
            crate::core::cache_man::PreloadStrategy::Spiral
        };

        debug!(
            "Preload epoch {}: center={}, range={}..{}, strategy={:?}",
            epoch, center, play_start, play_end, strategy
        );

        match strategy {
            crate::core::cache_man::PreloadStrategy::Spiral => {
                self.preload_spiral(workers, project, epoch, center, play_start, play_end);
            }
            crate::core::cache_man::PreloadStrategy::Forward => {
                self.preload_forward(workers, project, epoch, center, play_start, play_end);
            }
        }
    }

    /// Detect if frame points to a video file (vs image sequence)
    fn detect_video_at_frame(&self, frame_idx: i32) -> bool {
        if let Some(path) = self.resolve_frame_path(frame_idx) {
            return media::is_video(&path);
        }
        false
    }

    /// Spiral preload: 0, +1, -1, +2, -2, ...
    ///
    /// Loads frames around center in spiral pattern for cheap bidirectional seeking.
    /// Optimal for image sequences where seeking backwards is fast.
    fn preload_spiral(
        &self,
        workers: &Arc<Workers>,
        project: &super::Project,
        epoch: u64,
        center: i32,
        play_start: i32,
        play_end: i32,
    ) {
        // Calculate max offset to reach furthest boundary (forward or backward)
        let offset_backward = center - play_start;
        let offset_forward = play_end - center;
        let max_offset = offset_backward.max(offset_forward).max(0);

        debug!(
            "preload_spiral: center={}, range=[{}..{}], offset_backward={}, offset_forward={}, max_offset={}",
            center, play_start, play_end, offset_backward, offset_forward, max_offset
        );

        for offset in 0..=max_offset {
            // Backward: center - offset
            if center >= offset {
                let idx = center - offset;
                if idx >= play_start && idx <= play_end {
                    self.enqueue_frame(workers, project, epoch, idx);
                }
            }

            // Forward: center + offset (skip offset=0 as already loaded)
            if offset > 0 {
                let idx = center + offset;
                if idx >= play_start && idx <= play_end {
                    self.enqueue_frame(workers, project, epoch, idx);
                }
            }
        }
    }

    /// Forward-only preload: center, center+1, center+2, ...
    ///
    /// Loads frames forward from center for expensive backward seeking.
    /// Optimal for video files where seeking backwards costs decompression.
    fn preload_forward(
        &self,
        workers: &Arc<Workers>,
        project: &super::Project,
        epoch: u64,
        center: i32,
        play_start: i32,
        play_end: i32,
    ) {
        let start = center.max(play_start);
        for idx in start..=play_end {
            self.enqueue_frame(workers, project, epoch, idx);
        }
    }

    /// Enqueue single frame for background processing with epoch check
    ///
    /// File mode: loads frame from disk
    /// Layer mode: composes frame from children
    ///
    /// Skips frames that are already in cache.
    /// Uses execute_with_epoch() to automatically cancel stale requests.
    fn enqueue_frame(
        &self,
        workers: &Arc<Workers>,
        project: &super::Project,
        epoch: u64,
        frame_idx: i32,
    ) {
        let global_cache = match &self.global_cache {
            Some(cache) => cache.clone(),
            None => return,
        };

        let uuid = self.get_uuid();

        // Skip if already Loaded, Loading, or Error (don't re-enqueue)
        if let Some(status) = global_cache.get_status(uuid, frame_idx) {
            match status {
                FrameStatus::Loaded | FrameStatus::Loading | FrameStatus::Error => return,
                _ => {} // Header/Placeholder - proceed to enqueue
            }
        }

        if self.is_file_mode() {
            // File mode: load from disk
            let comp_start = self._in();
            let local_idx = frame_idx - comp_start;
            let seq_start = self.file_start().unwrap_or(comp_start);
            let seq_frame = seq_start.saturating_add(local_idx);

            // Get frame path
            let frame_path = match self.resolve_frame_path(seq_frame) {
                Some(path) => path,
                None => return,
            };

            let (w, h) = self.dim();

            // Get or create frame from cache
            let frame = if let Some(existing) = global_cache.get(uuid, frame_idx) {
                existing
            } else {
                // Create new Header frame and insert
                let new_frame = Frame::new_unloaded(frame_path.clone());
                new_frame.crop(w, h, CropAlign::LeftTop);
                global_cache.insert(uuid, frame_idx, new_frame.clone());
                new_frame
            };

            // Enqueue background load - frame is shared via Arc
            workers.execute_with_epoch(epoch, move || {
                // frame.load() uses try_claim_for_loading() internally
                // Atomically transitions Header → Loading, prevents duplicate loads
                match frame.load() {
                    Ok(_) => {
                        // Re-insert to update memory tracking (buffer grew from 1x1 to full size)
                        global_cache.insert(uuid, frame_idx, frame);
                        log::debug!("Background preload completed: comp={}, frame={}", uuid, frame_idx);
                    }
                    Err(e) => {
                        log::warn!("Background load failed for frame {}: {:?}", frame_idx, e);
                        // Status is already Error from load(), no need to re-insert
                    }
                }
            });
        } else {
            // Layer mode: compose from children in background
            let project_clone = project.clone();
            let comp_clone = self.clone();

            // Enqueue background composition
            workers.execute_with_epoch(epoch, move || {
                // Skip if already Loaded (check status, not presence)
                if let Some(status) = global_cache.get_status(uuid, frame_idx) {
                    if status == FrameStatus::Loaded {
                        return;
                    }
                }

                // Compose frame using CPU compositor (use_gpu=false for thread safety)
                if let Some(frame) = comp_clone.compose(frame_idx, &project_clone, false) {
                    global_cache.insert(uuid, frame_idx, frame);
                } else {
                    log::debug!(
                        "Background composition returned None: comp={}, frame={}",
                        uuid,
                        frame_idx
                    );
                }
            });
        }
    }



    /// Set event emitter (called after deserialization or when creating new comp in app)
    pub fn set_event_emitter(&mut self, emitter: CompEventEmitter) {
        self.event_emitter = emitter;
    }

    /// Get current frame (hot path - called 60fps during playback)
    #[inline]
    pub fn frame(&self) -> i32 {
        self.attrs.get_i32(A_FRAME).unwrap_or(0)
    }

    /// Set current frame and emit CurrentFrameChanged event.
    /// This is the proper way to change frame position - emits event that triggers frame loading.
    #[inline]
    pub fn set_frame(&mut self, new_frame: i32) {
        let old_frame = self.frame();
        if old_frame != new_frame {
            self.attrs.set(A_FRAME, AttrValue::Int(new_frame));
            self.event_emitter.emit(CurrentFrameChangedEvent {
                comp_uuid: self.get_uuid(),
                old_frame,
                new_frame,
            });
        }
    }

    /// Get composed frame at given global frame index.
    ///
    /// File mode:
    /// - Interprets frame_idx as 0-based within the clip (independent of on-disk numbering).
    /// - Returns a sized green placeholder outside the work area without touching the loader.
    ///
    /// Layer mode:
    /// - Resolves children recursively and blends them.
    pub fn get_frame(&self, frame_idx: i32, project: &super::Project, use_gpu: bool) -> Option<Frame> {
        if self.is_file_mode() {
            self.get_file_frame(frame_idx, project)
        } else {
            self.get_layer_frame(frame_idx, project, use_gpu)
        }
    }

    fn get_file_frame(&self, frame_idx: i32, project: &super::Project) -> Option<Frame> {
        let duration = self.frame_count();
        if duration <= 0 {
            return None;
        }

        let (work_start, work_end) = self.work_area_abs(true);
        if work_end < work_start {
            return Some(self.placeholder_frame());
        }

        // Outside work area -> placeholder, no load
        if frame_idx < work_start || frame_idx > work_end {
            return Some(self.placeholder_frame());
        }

        let comp_start = self._in();
        let comp_end = self._out();
        if comp_end < comp_start {
            return None;
        }

        // Convert absolute comp frame to local frame (0-based)
        let clamped_frame = frame_idx.clamp(comp_start, comp_end);
        let local_idx = clamped_frame - comp_start;
        if local_idx < 0 || local_idx >= duration {
            return Some(self.placeholder_frame());
        }

        // Map local frame_idx to absolute sequence number (preserve original numbering)
        let seq_start = self.file_start().unwrap_or(self._in());
        let seq_end = self.file_end().unwrap_or(self._out());
        let seq_frame = seq_start.saturating_add(local_idx);
        if seq_frame < seq_start || seq_frame > seq_end {
            return Some(self.placeholder_frame());
        }

        // Check global cache (using comp UUID + frame_idx as key - unified with Layer mode)
        if let Some(ref global_cache) = project.global_cache {
            if let Some(frame) = global_cache.get(self.get_uuid(), frame_idx) {
                return Some(frame);
            }
        }

        // Cache miss: load frame from disk
        let frame_path = self.resolve_frame_path(seq_frame).unwrap_or_default();
        if frame_path.as_os_str().is_empty() {
            return Some(self.placeholder_frame());
        }

        let frame = self.frame_from_path(frame_path);

        // Insert into global cache with frame_idx as key (unified with Layer mode)
        if let Some(ref global_cache) = project.global_cache {
            global_cache.insert(self.get_uuid(), frame_idx, frame.clone());
        }

        Some(frame)
    }

    fn get_layer_frame(&self, frame_idx: i32, project: &super::Project, use_gpu: bool) -> Option<Frame> {
        // Check if frame is within play area (work area)
        let (play_start, play_end) = self.play_range(true);
        if frame_idx < play_start || frame_idx > play_end {
            return None; // Frame outside work area - don't compose
        }

        // Check dirty flag on comp OR any child attrs OR cache miss
        let any_child_dirty = self.children.iter().any(|(_, attrs)| attrs.is_dirty());
        let needs_recompose = self.attrs.is_dirty()
            || any_child_dirty
            || project.global_cache.as_ref()
                .map(|cache| !cache.contains(self.get_uuid(), frame_idx))
                .unwrap_or(true);

        if !needs_recompose {
            // Frame is cached and clean - return from cache
            if let Some(ref global_cache) = project.global_cache {
                if let Some(frame) = global_cache.get(self.get_uuid(), frame_idx) {
                    return Some(frame);
                }
            }
        }

        // Compose frame recursively
        let composed = self.compose(frame_idx, project, use_gpu)?;

        // GlobalFrameCache::insert() rejects non-Loaded frames automatically
        // Only clear dirty flag if frame is Loaded (complete)
        if let Some(ref global_cache) = project.global_cache {
            global_cache.insert(self.get_uuid(), frame_idx, composed.clone());
        }
        if composed.status() == FrameStatus::Loaded {
            self.attrs.clear_dirty();
            // Clear dirty on all children too
            for (_, attrs) in &self.children {
                attrs.clear_dirty();
            }
        }

        Some(composed)
    }

    fn resolve_frame_path(&self, frame_number: i32) -> Option<PathBuf> {
        let mask = self.file_mask()?;
        if media::is_video(Path::new(&mask)) {
            // Video files use @frame suffix to target specific frame
            return Some(PathBuf::from(format!("{}@{}", mask, frame_number)));
        }

        if mask.contains('*') {
            let padding = self.attrs.get_u32("padding").unwrap_or(4) as usize;
            let mut parts = mask.splitn(2, '*');
            let prefix = parts.next().unwrap_or_default();
            let suffix = parts.next().unwrap_or_default();
            let path = format!("{}{:0padding$}{}", prefix, frame_number, suffix);
            Some(PathBuf::from(path))
        } else {
            Some(PathBuf::from(mask))
        }
    }

    /// Resolution of this comp (used for placeholders/compositing).
    /// For layer comps, set from the first added child.
    pub fn dim(&self) -> (usize, usize) {
        let w = self.attrs.get_u32("width").unwrap_or(64) as usize;
        let h = self.attrs.get_u32("height").unwrap_or(64) as usize;
        (w.max(1), h.max(1))
    }

    /// Determine dimensions from the earliest (smallest start) child.
    /// Falls back to current comp dim if no children.
    pub fn first_child_dim(&self) -> (usize, usize) {
        let mut best: Option<(i32, usize, usize)> = None;
        for (_child_uuid, attrs) in &self.children {
            let start = attrs.get_i32("in").unwrap_or(0);
            let w = attrs.get_u32("width").unwrap_or(0) as usize;
            let h = attrs.get_u32("height").unwrap_or(0) as usize;
            match best {
                None => best = Some((start, w, h)),
                Some((best_start, _, _)) if start < best_start => best = Some((start, w, h)),
                _ => {}
            }
        }

        if let Some((_, w, h)) = best {
            (w.max(1), h.max(1))
        } else {
            self.dim()
        }
    }

    fn placeholder_frame(&self) -> Frame {
        let (w, h) = self.dim();
        Frame::new(w, h, PixelDepth::U8)
    }

    fn frame_from_path(&self, path: PathBuf) -> Frame {
        let (w, h) = self.dim();
        let frame = Frame::new_unloaded(path);
        frame.crop(w, h, CropAlign::LeftTop);
        frame
    }

    /// Compose frame at given global frame index.
    ///
    /// Recursively resolves all active children:
    /// - Converts global comp frame to local source frame
    /// - Resolves Comp from Project.media by UUID
    /// - Recursively gets frames (supports nested Comps)
    /// - Blends multiple children with CPU or GPU compositor
    /// - use_gpu=true: uses Project.compositor (main thread only)
    /// - use_gpu=false: uses thread_local CPU compositor (safe for background threads)
    /// Compose frame from children.
    /// If any child is not fully loaded, the result Frame will have status=Loading
    /// (not Loaded), so GlobalFrameCache will reject it - preventing green frame caching.
    fn compose(&self, frame_idx: i32, project: &super::Project, use_gpu: bool) -> Option<Frame> {
        use log::debug;
        let mut source_frames: Vec<(Frame, f32, BlendMode)> = Vec::new();
        let mut earliest: Option<(i32, usize)> = None; // (start_frame, index in source_frames)
        let mut target_format: PixelFormat = PixelFormat::Rgba8;
        let mut all_children_loaded = true; // Track if all source frames are fully loaded

        debug!(
            "compose() called: frame_idx={}, children.len()={}",
            frame_idx,
            self.children.len()
        );

        // Collect frames from all active children
        // IMPORTANT: Reverse iteration - last child (bottom layer) becomes base,
        // first child (top layer) composited last
        for (child_uuid, attrs) in self.children.iter().rev() {
            // Placement and bounds in parent timeline
            let child_start = attrs.get_i32("in").unwrap_or(0);
            let child_end = attrs.get_i32("out").unwrap_or(child_start);
            let duration = (child_end - child_start + 1).max(0);
            let (play_start, play_end) = self
                .child_work_area_abs(*child_uuid)
                .unwrap_or((child_start, child_end));

            // Outside work area -> skip
            if frame_idx < play_start || frame_idx > play_end {
                debug!(
                    "  child {} TRIMMED OUT: frame {} not in play range [{}, {}]",
                    child_uuid, frame_idx, play_start, play_end
                );
                continue;
            }

            debug!(
                "  child {} ACTIVE: comp_frame={}, child_start={}, play_range=[{}, {}]",
                child_uuid, frame_idx, child_start, play_start, play_end
            );

            // Get source UUID from child attrs (child_uuid is now instance UUID)
            let Some(source_uuid_str) = attrs.get_str("uuid") else {
                continue;
            };
            let Ok(source_uuid) = Uuid::parse_str(source_uuid_str) else {
                continue;
            };

            // Resolve source from Project.media and clone to avoid holding lock during recursive compose
            let source_comp = {
                let media = project.media.read().unwrap();
                media.get(&source_uuid).cloned()
            };

            if let Some(source) = source_comp {
                // Visibility toggle
                if attrs.get_bool("visible").unwrap_or(true) == false {
                    continue;
                }

                // Convert parent comp frame to child's local frame using comp2local
                let local_frame = self.comp2local(*child_uuid, frame_idx).unwrap_or(0);
                if local_frame < 0 || local_frame >= duration {
                    continue;
                }
                let source_frame = source._in() + local_frame;

                // Recursively get frame from source (Clip or Comp)
                if let Some(frame) = source.get_frame(source_frame, project, use_gpu) {
                    // Check if this child frame is fully loaded
                    let frame_status = frame.status();
                    if frame_status != FrameStatus::Loaded {
                        all_children_loaded = false;
                        debug!(
                            "  child {} frame {} NOT LOADED (status={:?}), marking composite as incomplete",
                            child_uuid, source_frame, frame_status
                        );
                    }

                    let opacity = attrs.get_float("opacity").unwrap_or(1.0);
                    let blend_mode = attrs
                        .get_str("blend_mode")
                        .or_else(|| attrs.get_str("layer_mode"))
                        .map(|s| match s {
                            "screen" => BlendMode::Screen,
                            "add" => BlendMode::Add,
                            "subtract" => BlendMode::Subtract,
                            "multiply" => BlendMode::Multiply,
                            "divide" => BlendMode::Divide,
                            "difference" => BlendMode::Difference,
                            _ => BlendMode::Normal,
                        })
                        .unwrap_or(BlendMode::Normal);
                    source_frames.push((frame, opacity, blend_mode));
                    let idx = source_frames.len() - 1;
                    if earliest.map_or(true, |(s, _)| child_start < s) {
                        earliest = Some((child_start, idx));
                    }
                    // Track highest precision format
                    target_format = match (target_format, source_frames[idx].0.pixel_format()) {
                        (PixelFormat::RgbaF32, _) | (_, PixelFormat::RgbaF32) => PixelFormat::RgbaF32,
                        (PixelFormat::RgbaF16, _) | (_, PixelFormat::RgbaF16) => PixelFormat::RgbaF16,
                        _ => PixelFormat::Rgba8,
                    };
                }
            }
        }

        // Blend all children with project compositor (CPU or GPU)
        let dim = earliest
            .as_ref()
            .and_then(|(_, idx)| {
                source_frames
                    .get(*idx)
                    .map(|(f, _, _)| (f.width().max(1), f.height().max(1)))
            })
            .unwrap_or_else(|| self.dim());

        // Promote all frames to target_format to avoid compositor mismatches
        fn promote_frame(frame: &Frame, target: PixelFormat) -> Frame {
            match (frame.pixel_format(), target) {
                (PixelFormat::Rgba8, PixelFormat::Rgba8)
                | (PixelFormat::RgbaF16, PixelFormat::RgbaF16)
                | (PixelFormat::RgbaF32, PixelFormat::RgbaF32) => frame.clone(),
                (PixelFormat::Rgba8, PixelFormat::RgbaF16) => {
                    if let PixelBuffer::U8(buf) = &*frame.buffer() {
                        let mut out = Vec::with_capacity(buf.len());
                        for chunk in buf.chunks_exact(4) {
                            out.push(f16::from_f32(chunk[0] as f32 / 255.0));
                            out.push(f16::from_f32(chunk[1] as f32 / 255.0));
                            out.push(f16::from_f32(chunk[2] as f32 / 255.0));
                            out.push(f16::from_f32(chunk[3] as f32 / 255.0));
                        }
                        Frame::from_f16_buffer(out, frame.width(), frame.height())
                    } else {
                        frame.clone()
                    }
                }
                (PixelFormat::Rgba8, PixelFormat::RgbaF32) => {
                    if let PixelBuffer::U8(buf) = &*frame.buffer() {
                        let mut out = Vec::with_capacity(buf.len());
                        for chunk in buf.chunks_exact(4) {
                            out.push(chunk[0] as f32 / 255.0);
                            out.push(chunk[1] as f32 / 255.0);
                            out.push(chunk[2] as f32 / 255.0);
                            out.push(chunk[3] as f32 / 255.0);
                        }
                        Frame::from_f32_buffer(out, frame.width(), frame.height())
                    } else {
                        frame.clone()
                    }
                }
                (PixelFormat::RgbaF16, PixelFormat::RgbaF32) => {
                    if let PixelBuffer::F16(buf) = &*frame.buffer() {
                        let mut out = Vec::with_capacity(buf.len());
                        for f in buf {
                            out.push(f.to_f32());
                        }
                        Frame::from_f32_buffer(out, frame.width(), frame.height())
                    } else {
                        frame.clone()
                    }
                }
                // Avoid down-conversion to preserve precision; fall back to clone
                _ => frame.clone(),
            }
        }

        for (frame, _opacity, _mode) in source_frames.iter_mut() {
            *frame = promote_frame(frame, target_format);
        }

        // Always add a solid black base underneath so fully transparent layers show black.
        // Match the pixel format of the first child; fall back to U8 when no children.
        let make_u8_base = || {
            let mut buf = vec![0u8; dim.0 * dim.1 * 4];
            for px in buf.chunks_exact_mut(4) {
                px[3] = 255; // opaque alpha
            }
            Frame::from_buffer(PixelBuffer::U8(buf), PixelFormat::Rgba8, dim.0, dim.1)
        };
        let base = match target_format {
            PixelFormat::RgbaF32 => {
                let mut buf = vec![0.0f32; dim.0 * dim.1 * 4];
                for px in buf.chunks_exact_mut(4) {
                    px[3] = 1.0;
                }
                Frame::from_f32_buffer(buf, dim.0, dim.1)
            }
            PixelFormat::RgbaF16 => {
                let mut buf = vec![f16::from_f32(0.0); dim.0 * dim.1 * 4];
                for px in buf.chunks_exact_mut(4) {
                    px[3] = f16::from_f32(1.0);
                }
                Frame::from_f16_buffer(buf, dim.0, dim.1)
            }
            PixelFormat::Rgba8 => make_u8_base(),
        };
        source_frames.insert(0, (base, 1.0, BlendMode::Normal));
        debug!(
            "compose() collected {} frames, calling compositor.blend_with_dim({}, {}) [use_gpu={}, all_loaded={}]",
            source_frames.len(),
            dim.0,
            dim.1,
            use_gpu,
            all_children_loaded
        );

        // Use GPU compositor (main thread) or CPU compositor (background threads)
        let result = if use_gpu {
            project.compositor.borrow_mut().blend_with_dim(source_frames, dim)
        } else {
            THREAD_COMPOSITOR.with(|comp| {
                comp.borrow_mut().blend_with_dim(source_frames, dim)
            })
        };

        // If not all children loaded, mark frame as Loading so cache rejects it
        result.map(|frame| {
            if !all_children_loaded {
                debug!(
                    "compose() returning INCOMPLETE frame (some children not loaded), setting status=Loading"
                );
                let _ = frame.set_status(FrameStatus::Loading);
            }
            frame
        })
    }

    /// Add a new child to the composition at specified start frame.
    ///
    /// Automatically determines duration from source and creates child attributes.
    /// Add child by looking up source from project
    pub fn add_child(
        &mut self,
        source_uuid: Uuid,
        start_frame: i32,
        project: &super::Project,
    ) -> anyhow::Result<()> {
        // Get source to determine duration
        let source = project
            .get_comp(source_uuid)
            .ok_or_else(|| anyhow::anyhow!("Source {} not found", source_uuid))?;

        // First child defines comp resolution
        if self.children.is_empty() {
            let (w, h) = source.dim();
            self.attrs.set("width", AttrValue::UInt(w as u32));
            self.attrs.set("height", AttrValue::UInt(h as u32));
        }

        let duration = source.frame_count();
        let dim = source.dim();
        self.add_child_with_duration(source_uuid, start_frame, duration, None, dim)
    }

    /// Add child with explicit duration and optional target row
    pub fn add_child_with_duration(
        &mut self,
        source_uuid: Uuid,
        start_frame: i32,
        duration: i32,
        target_row: Option<usize>,
        source_dim: (usize, usize),
    ) -> anyhow::Result<()> {
        let end_frame = start_frame + duration - 1;

        // Generate unique instance UUID for this child
        let instance_uuid = Uuid::new_v4();

        // Create child attributes
        let mut attrs = Attrs::new();
        attrs.set("uuid", AttrValue::Str(source_uuid.to_string())); // Reference to source comp (stored as string)
        attrs.set("name", AttrValue::Str("Child".to_string()));
        attrs.set("in", AttrValue::Int(start_frame));
        attrs.set("out", AttrValue::Int(end_frame));
        // Work area defaults to full placement range in parent timeline
        attrs.set("trim_in", AttrValue::Int(start_frame));
        attrs.set("trim_out", AttrValue::Int(end_frame));
        attrs.set("opacity", AttrValue::Float(1.0));
        attrs.set("visible", AttrValue::Bool(true));
        attrs.set("blend_mode", AttrValue::Str("normal".to_string()));
        attrs.set("speed", AttrValue::Float(1.0));
        attrs.set("width", AttrValue::UInt(source_dim.0 as u32));
        attrs.set("height", AttrValue::UInt(source_dim.1 as u32));

        // Add to children at appropriate position for target row
        if let Some(target_row) = target_row {
            let insert_pos = self.find_insert_position_for_row(target_row);
            self.children.insert(insert_pos, (instance_uuid, attrs));
        } else {
            self.children.push((instance_uuid, attrs));
        }

        self.rebound();
        self.update_dim_from_children();
        // Mark as dirty for cache invalidation
        self.attrs.mark_dirty();
        self.event_emitter.emit(LayersChangedEvent {
            comp_uuid: self.get_uuid(),
            affected_range: Some((start_frame, end_frame)),
        });
        self.event_emitter.emit(AttrsChangedEvent(self.get_uuid()));

        Ok(())
    }

    /// Find insertion position in children array to achieve target visual row
    fn find_insert_position_for_row(&self, target_row: usize) -> usize {
        use std::collections::HashMap;

        // Compute current layout for all existing children
        let mut layer_rows: HashMap<usize, usize> = HashMap::new();
        let mut occupied_rows: HashMap<usize, Vec<(i32, i32)>> = HashMap::new();

        for (idx, (_child_uuid, attrs)) in self.children.iter().enumerate() {
            let start = attrs.get_i32("in").unwrap_or(0);
            let end = attrs.get_i32("out").unwrap_or(0);

            // Find first free row for this layer
            let mut row = 0;
            loop {
                let mut row_free = true;
                if let Some(ranges) = occupied_rows.get(&row) {
                    for (occupied_start, occupied_end) in ranges {
                        if start <= *occupied_end && end >= *occupied_start {
                            row_free = false;
                            break;
                        }
                    }
                }

                if row_free {
                    occupied_rows
                        .entry(row)
                        .or_insert_with(Vec::new)
                        .push((start, end));
                    layer_rows.insert(idx, row);
                    break;
                }

                row += 1;
            }
        }

        // Find insertion position: before first layer with row >= target_row
        for idx in 0..self.children.len() {
            if let Some(&row) = layer_rows.get(&idx) {
                if row >= target_row {
                    return idx;
                }
            }
        }

        // If no layer found with row >= target_row, insert at end
        self.children.len()
    }

    /// Move a child to a new start position, preserving duration.
    /// Supports negative start positions and automatically extends parent comp boundaries.
    pub fn move_child(&mut self, child_idx: usize, new_start: i32) -> anyhow::Result<()> {
        let (_child_uuid, attrs) = self
            .children
            .get_mut(child_idx)
            .ok_or_else(|| anyhow::anyhow!("Child {} not found", child_idx))?;

        let old_start = attrs.get_i32("in").unwrap_or(0);
        let old_end = attrs.get_i32("out").unwrap_or(0);
        let duration = (old_end - old_start).max(0);
        let new_end = new_start + duration;
        let delta = new_start - old_start;

        let play_start_old = attrs.get_i32("trim_in").unwrap_or(old_start);
        let play_end_old = attrs.get_i32("trim_out").unwrap_or(old_end);

        attrs.set("in", AttrValue::Int(new_start));
        attrs.set("out", AttrValue::Int(new_end));

        // Shift work area by the same delta, clamped to new bounds
        let shifted_start = play_start_old + delta;
        let shifted_end = play_end_old + delta;
        let (clamped_start, clamped_end) =
            Self::clamp_range_to_bounds((shifted_start, shifted_end), (new_start, new_end));
        attrs.set("trim_in", AttrValue::Int(clamped_start));
        attrs.set("trim_out", AttrValue::Int(clamped_end));

        self.rebound();
        self.update_dim_from_children();

        // Affected range: union of old and new positions
        let range_start = old_start.min(new_start);
        let range_end = old_end.max(new_end);

        // Mark as dirty for cache invalidation
        self.attrs.mark_dirty();
        self.event_emitter.emit(LayersChangedEvent {
            comp_uuid: self.get_uuid(),
            affected_range: Some((range_start, range_end)),
        });
        self.event_emitter.emit(AttrsChangedEvent(self.get_uuid()));

        Ok(())
    }

    /// Move multiple layers by delta; optionally reorder block to target_row (visual row).
    /// `indices` are child indices in current order. Handles both single and multi-layer moves.
    pub fn move_layers(
        &mut self,
        indices: &[usize],
        delta: i32,
        target_row: Option<usize>,
    ) -> anyhow::Result<()> {
        if indices.is_empty() {
            return Ok(());
        }

        // Dedup and sort
        let mut idxs: Vec<usize> = indices.to_vec();
        idxs.sort_unstable();
        idxs.dedup();

        // layer_selection already stores UUIDs - just clone them
        let selected_uuids: Vec<Uuid> = self.layer_selection.clone();

        // Build block of (Uuid, Attrs) tuples
        let mut block: Vec<(Uuid, Attrs)> = Vec::new();
        let mut reordered = self.children.clone();
        for idx in idxs.iter().rev() {
            if *idx < reordered.len() {
                block.push(reordered.remove(*idx));
            }
        }
        block.reverse();

        // Reorder block if target_row is provided
        let insert_at = target_row.unwrap_or_else(|| {
            // default: place back at first removed index
            *idxs.first().unwrap_or(&0)
        });
        let insert_at = insert_at.min(reordered.len());
        let mut cursor = insert_at;
        for item in block.iter() {
            reordered.insert(cursor, item.clone());
            cursor += 1;
        }
        self.children = reordered;

        // layer_selection stores UUIDs - no need to update after reorder
        // Just restore the UUIDs that still exist in children
        self.layer_selection = selected_uuids
            .into_iter()
            .filter(|uuid| self.children_contains(uuid))
            .collect();

        // Move each by delta (preserve relative offsets)
        for (uuid, _) in block {
            if let Some(idx) = self.children.iter().position(|(u, _)| *u == uuid) {
                let current_start = self
                    .children_attrs_get(&uuid)
                    .map(|a| a.get_i32("in").unwrap_or(0))
                    .unwrap_or(0);
                let _ = self.move_child(idx, current_start + delta);
            }
        }

        Ok(())
    }

    /// Trim multiple layers by delta on start or end.
    /// `indices` are child indices; delta applied to play_start (is_start=true) or play_end (false).
    pub fn trim_layers(&mut self, indices: &[usize], delta: i32, is_start: bool) -> anyhow::Result<()> {
        if indices.is_empty() {
            return Ok(());
        }

        let mut idxs: Vec<usize> = indices.to_vec();
        idxs.sort_unstable();
        idxs.dedup();

        // Track affected range across all layers
        let mut range_min = i32::MAX;
        let mut range_max = i32::MIN;

        for idx in idxs {
            if idx >= self.children.len() {
                continue;
            }
            if let Some((_child_uuid, attrs)) = self.children.get_mut(idx) {
                let bounds_start = attrs.get_i32("in").unwrap_or(0);
                let bounds_end = attrs.get_i32("out").unwrap_or(0);
                let (bounds_start, bounds_end) = Self::child_bounds_abs(Some(bounds_start), Some(bounds_end));

                // Track range before and after trim
                range_min = range_min.min(bounds_start);
                range_max = range_max.max(bounds_end);

                if is_start {
                    let current = attrs.get_i32("trim_in").unwrap_or(bounds_start);
                    let (clamped_start, clamped_end) = Self::clamp_range_to_bounds(
                        (current + delta, attrs.get_i32("trim_out").unwrap_or(bounds_end)),
                        (bounds_start, bounds_end),
                    );
                    attrs.set("trim_in", AttrValue::Int(clamped_start));
                    attrs.set("trim_out", AttrValue::Int(clamped_end));
                } else {
                    let current = attrs.get_i32("trim_out").unwrap_or(bounds_end);
                    let (clamped_start, clamped_end) = Self::clamp_range_to_bounds(
                        (attrs.get_i32("trim_in").unwrap_or(bounds_start), current + delta),
                        (bounds_start, bounds_end),
                    );
                    attrs.set("trim_in", AttrValue::Int(clamped_start));
                    attrs.set("trim_out", AttrValue::Int(clamped_end));
                }
            }
        }

        let affected_range = if range_min <= range_max {
            Some((range_min, range_max))
        } else {
            None
        };

        // Mark as dirty for cache invalidation
        self.attrs.mark_dirty();
        self.event_emitter.emit(LayersChangedEvent {
            comp_uuid: self.get_uuid(),
            affected_range,
        });
        self.event_emitter.emit(AttrsChangedEvent(self.get_uuid()));
        Ok(())
    }

    /// Set child play start (adjust play_start attribute - visible start offset from child start).
    pub fn set_child_play_start(
        &mut self,
        child_idx: usize,
        new_play_start: i32,
    ) -> anyhow::Result<()> {
        let (_child_uuid, attrs) = self
            .children
            .get_mut(child_idx)
            .ok_or_else(|| anyhow::anyhow!("Child {} not found", child_idx))?;

        let (bounds_start, bounds_end) =
            Self::child_bounds_abs(attrs.get_i32("in"), attrs.get_i32("out"));
        let old_start = attrs.get_i32("trim_in").unwrap_or(bounds_start);
        let current_end = attrs.get_i32("trim_out").unwrap_or(bounds_end);
        let (clamped_start, clamped_end) =
            Self::clamp_range_to_bounds((new_play_start, current_end), (bounds_start, bounds_end));
        attrs.set("trim_in", AttrValue::Int(clamped_start));
        attrs.set("trim_out", AttrValue::Int(clamped_end));

        // Affected range: union of old and new start positions
        let range_start = old_start.min(clamped_start);
        let range_end = old_start.max(clamped_start);

        self.attrs.mark_dirty();
        self.event_emitter.emit(LayersChangedEvent {
            comp_uuid: self.get_uuid(),
            affected_range: Some((range_start, range_end)),
        });
        self.event_emitter.emit(AttrsChangedEvent(self.get_uuid()));

        Ok(())
    }

    /// Set child play end (adjust play_end attribute - visible end offset from child end).
    pub fn set_child_play_end(
        &mut self,
        child_idx: usize,
        new_play_end: i32,
    ) -> anyhow::Result<()> {
        let (_child_uuid, attrs) = self
            .children
            .get_mut(child_idx)
            .ok_or_else(|| anyhow::anyhow!("Child {} not found", child_idx))?;

        let (bounds_start, bounds_end) =
            Self::child_bounds_abs(attrs.get_i32("in"), attrs.get_i32("out"));
        let current_start = attrs.get_i32("trim_in").unwrap_or(bounds_start);
        let old_end = attrs.get_i32("trim_out").unwrap_or(bounds_end);
        let (clamped_start, clamped_end) =
            Self::clamp_range_to_bounds((current_start, new_play_end), (bounds_start, bounds_end));
        attrs.set("trim_in", AttrValue::Int(clamped_start));
        attrs.set("trim_out", AttrValue::Int(clamped_end));

        // Affected range: union of old and new end positions
        let range_start = old_end.min(clamped_end);
        let range_end = old_end.max(clamped_end);

        self.attrs.mark_dirty();
        self.event_emitter.emit(LayersChangedEvent {
            comp_uuid: self.get_uuid(),
            affected_range: Some((range_start, range_end)),
        });
        self.event_emitter.emit(AttrsChangedEvent(self.get_uuid()));

        Ok(())
    }

    /// Set comp play start in absolute comp frames (inclusive).
    /// Ensures `play_end` remains >= start and clamps to comp bounds.
    pub fn set_comp_play_start(&mut self, new_play_start: i32) {
        let current_end = self.trim_out();
        self.set_work_area_abs(new_play_start, current_end);
    }

    /// Set comp play end in absolute comp frames (inclusive).
    /// Ensures `play_start` remains <= end and clamps to comp bounds.
    pub fn set_comp_play_end(&mut self, new_play_end: i32) {
        let current_start = self.trim_in();
        self.set_work_area_abs(current_start, new_play_end);
    }

    /// Get all child edges (start and end frames) sorted by distance from given frame
    /// Returns vec of (frame_number, is_start) tuples
    pub fn get_child_edges_near(&self, _from_frame: i32) -> Vec<(i32, bool)> {
        let mut edges = Vec::new();

        for (_child_uuid, attrs) in &self.children {
            let start = attrs.get_i32("in").unwrap_or(0);
            let end = attrs.get_i32("out").unwrap_or(0);
            let play_start = attrs.get_i32("trim_in").unwrap_or(start);
            let play_end = attrs.get_i32("trim_out").unwrap_or(end);

            // Visible range accounting for play range offsets
            let visible_start = play_start;
            let visible_end = play_end;

            if visible_start <= visible_end {
                edges.push((visible_start, true)); // Start edge
                edges.push((visible_end, false)); // End edge
            }
        }

        // Sort by frame number to allow deterministic next/previous jumps
        edges.sort_by_key(|(frame, _)| *frame);
        edges.dedup_by_key(|(frame, _)| *frame);

        edges
    }

    // ===== Parent-Child Management =====

    /// Remove child comp from this composition
    pub fn remove_child(&mut self, child_uuid: Uuid) {
        // Get removed child's range before removing
        let affected_range = self
            .children
            .iter()
            .find(|(uuid, _)| *uuid == child_uuid)
            .map(|(_, attrs)| {
                let (bounds_start, bounds_end) =
                    Self::child_bounds_abs(attrs.get_i32("in"), attrs.get_i32("out"));
                let start = attrs.get_i32("trim_in").unwrap_or(bounds_start);
                let end = attrs.get_i32("trim_out").unwrap_or(bounds_end);
                (start, end)
            });

        self.children.retain(|(uuid, _)| *uuid != child_uuid);
        self.rebound();
        self.update_dim_from_children();
        self.attrs.mark_dirty();
        self.event_emitter.emit(LayersChangedEvent {
            comp_uuid: self.get_uuid(),
            affected_range,
        });
        self.event_emitter.emit(AttrsChangedEvent(self.get_uuid()));
    }

    /// Recalculate comp start/end based on children (negative starts allowed).
    pub fn rebound(&mut self) {
        // File-mode comps have their own start/end; don't override them.
        if self.is_file_mode() {
            return;
        }
        let old_bounds = (self._in(), self._out());
        let old_work = self.play_range(true);
        if self.children.is_empty() {
            // Default span when no children: 0..100 for a visible timeline
            self.attrs.set("in", AttrValue::Int(0));
            self.attrs.set("out", AttrValue::Int(100));
            if old_work == old_bounds {
                self.attrs.set("trim_in", AttrValue::Int(0));
                self.attrs.set("trim_out", AttrValue::Int(100));
            }
            return;
        }

        let mut min_start = i32::MAX;
        let mut max_end = i32::MIN;

        for (child_uuid, _attrs) in &self.children {
            if let Some((visible_start, visible_end)) = self.child_work_area_abs(*child_uuid) {
                min_start = min_start.min(visible_start);
                max_end = max_end.max(visible_end);
            }
        }

        let (new_start, new_end) = if min_start == i32::MAX || max_end == i32::MIN {
            (0, 0)
        } else {
            (min_start, max_end)
        };

        self.attrs.set("in", AttrValue::Int(new_start));
        self.attrs.set("out", AttrValue::Int(new_end));

        // Keep work area in sync only if it used to match full bounds
        if old_work == old_bounds {
            self.attrs.set("trim_in", AttrValue::Int(new_start));
            self.attrs.set("trim_out", AttrValue::Int(new_end));
        }
    }

    /// Called when comp becomes active in timeline.
    /// Recalculates bounds and realigns play_range if needed.
    pub fn on_activate(&mut self) {
        self.rebound();
    }

    /// Ensure comp resolution matches the earliest (by start) child.
    fn update_dim_from_children(&mut self) {
        if self.children.is_empty() {
            return;
        }

        let (w, h) = self.first_child_dim();
        self.attrs.set("width", AttrValue::UInt(w as u32));
        self.attrs.set("height", AttrValue::UInt(h as u32));
    }

    /// Get children composition UUIDs
    /// Get children with their attrs
    pub fn get_children(&self) -> &[(Uuid, Attrs)] {
        &self.children
    }

    /// Check if this comp has a specific child
    pub fn has_child(&self, child_uuid: Uuid) -> bool {
        self.children.iter().any(|(uuid, _)| *uuid == child_uuid)
    }

    /// Find all children (instance UUIDs) that reference a specific source UUID
    pub fn find_children_by_source(&self, source_uuid: Uuid) -> Vec<Uuid> {
        let source_str = source_uuid.to_string();
        let mut result = Vec::new();
        for (child_uuid, attrs) in &self.children {
            if let Some(uuid) = attrs.get_str("uuid") {
                if uuid == source_str {
                    result.push(*child_uuid);
                }
            }
        }
        result
    }

}

impl Comp {
    /// Detect image/video sequences from paths and create File-mode comps.
    pub fn detect_from_paths(paths: Vec<PathBuf>) -> Result<Vec<Comp>, FrameError> {
        let mut comps = Vec::new();

        for path in paths {
            // Video file: create comp from video metadata
            if media::is_video(&path) {
                comps.push(create_video_comp(&path)?);
                continue;
            }

            // Try to detect if this is part of an image sequence
            if let Some((prefix, _number, ext, padding)) = split_sequence_path(&path)? {
                let pattern = format!("{}*.{}", prefix, ext);
                match detect_sequence_from_pattern(&pattern, padding) {
                    Ok(comp) => comps.push(comp),
                    Err(e) => {
                        info!("Failed to detect sequence for {}: {}", path.display(), e);
                        if let Ok(comp) = create_single_file_comp(&path) {
                            comps.push(comp);
                        }
                    }
                }
            } else if let Ok(comp) = create_single_file_comp(&path) {
                // Single file, not a sequence
                comps.push(comp);
            }
        }

        // Deduplicate comps by pattern/mask
        let mut unique: HashMap<String, Comp> = HashMap::new();
        for comp in comps {
            if let Some(mask) = comp.file_mask() {
                unique.entry(mask).or_insert(comp);
            }
        }

        Ok(unique.into_values().collect())
    }
}

/// Detect sequence from glob pattern.
fn detect_sequence_from_pattern(pattern: &str, padding: usize) -> Result<Comp, FrameError> {
    let paths = glob_paths(pattern)?;
    if paths.is_empty() {
        return Err(FrameError::Image(format!(
            "No files matched pattern: {}",
            pattern
        )));
    }

    // Group by (prefix, ext), storing (number, path, padding)
    let mut groups: HashMap<(String, String), Vec<(usize, PathBuf, usize)>> = HashMap::new();

    for path in paths {
        if let Some((prefix, number, ext, pad)) = split_sequence_path(&path)? {
            let key = (prefix, ext);
            groups.entry(key).or_default().push((number, path, pad));
        }
    }

    // Select largest group as main sequence
    let (key, frames_data) = groups
        .into_iter()
        .max_by_key(|(_, v)| v.len())
        .ok_or_else(|| FrameError::Image("No valid sequence files found".into()))?;

    let (prefix, ext) = key;
    let (min_frame, max_frame) = frames_data
        .iter()
        .fold((usize::MAX, 0usize), |(min_f, max_f), (num, _, _)| {
            (min_f.min(*num), max_f.max(*num))
        });

    // Get frame dimensions from first frame
    let first_path = &frames_data[0].1;
    let attrs = Loader::header(first_path)?;
    let width = attrs.get_u32("width").unwrap_or(0) as usize;
    let height = attrs.get_u32("height").unwrap_or(0) as usize;

    // Create Comp with File mode
    let file_mask = format!("{}*.{}", prefix, ext);
    let mut comp = Comp::new_file_comp(file_mask.clone(), min_frame as i32, max_frame as i32, 24.0);

    // Store dimensions and padding
    comp.attrs.set("width", AttrValue::UInt(width as u32));
    comp.attrs.set("height", AttrValue::UInt(height as u32));
    comp.attrs.set("padding", AttrValue::UInt(padding as u32));

    // Set name from first file
    if let Some(filename) = first_path.file_stem().and_then(|s| s.to_str()) {
        comp.attrs.set("name", AttrValue::Str(filename.to_string()));
    }

    info!(
        "Created sequence comp: {} ({} frames, {}x{})",
        file_mask,
        frames_data.len(),
        width,
        height
    );

    Ok(comp)
}

/// Create Comp from single image file.
fn create_single_file_comp(path: &Path) -> Result<Comp, FrameError> {
    if media::is_video(path) {
        return create_video_comp(path);
    }

    let attrs = Loader::header(path)?;
    let width = attrs.get_u32("width").unwrap_or(0) as usize;
    let height = attrs.get_u32("height").unwrap_or(0) as usize;

    let file_mask = path.to_string_lossy().to_string();
    let mut comp = Comp::new_file_comp(file_mask.clone(), 0, 0, 24.0);

    comp.attrs.set("width", AttrValue::UInt(width as u32));
    comp.attrs.set("height", AttrValue::UInt(height as u32));

    if let Some(filename) = path.file_stem().and_then(|s| s.to_str()) {
        comp.attrs.set("name", AttrValue::Str(filename.to_string()));
    }

    info!(
        "Created single file comp: {} ({}x{})",
        file_mask, width, height
    );

    Ok(comp)
}

/// Create Comp from video file using FFmpeg metadata.
fn create_video_comp(path: &Path) -> Result<Comp, FrameError> {
    let meta = loader_video::VideoMetadata::from_file(path)?;
    let last_frame = meta.frame_count.saturating_sub(1) as i32;
    let mut comp = Comp::new_file_comp(
        path.to_string_lossy().to_string(),
        0,
        last_frame,
        meta.fps as f32,
    );

    comp.attrs.set("width", AttrValue::UInt(meta.width));
    comp.attrs.set("height", AttrValue::UInt(meta.height));
    comp.attrs.set("padding", AttrValue::UInt(0));
    comp.attrs
        .set("frames", AttrValue::UInt(meta.frame_count as u32));
    comp.attrs.set("fps", AttrValue::Float(meta.fps as f32));
    comp.attrs.set(
        "format",
        AttrValue::Str(format!("Video ({})", path.display())),
    );

    if let Some(filename) = path.file_stem().and_then(|s| s.to_str()) {
        comp.attrs.set("name", AttrValue::Str(filename.to_string()));
    }

    info!(
        "Created video comp: {} ({} frames, {}x{})",
        path.display(),
        meta.frame_count,
        meta.width,
        meta.height
    );

    Ok(comp)
}

/// Expand a glob pattern into a list of paths.
fn glob_paths(pattern: &str) -> Result<Vec<PathBuf>, FrameError> {
    let mut paths = Vec::new();
    for entry in glob(pattern)
        .map_err(|e| FrameError::Image(format!("Glob error for pattern {}: {}", pattern, e)))?
    {
        match entry {
            Ok(path) => paths.push(path),
            Err(e) => return Err(FrameError::Image(format!("Glob entry error: {}", e))),
        }
    }
    Ok(paths)
}

/// Split a sequence filename into (prefix, number, ext, padding).
///
/// Example: "/path/seq.0001.exr" -> ("/path/seq.", 1, "exr", 4)
fn split_sequence_path(path: &Path) -> Result<Option<(String, usize, String, usize)>, FrameError> {
    let ext = match path.extension().and_then(|s| s.to_str()) {
        Some(e) => e.to_string(),
        None => return Ok(None),
    };

    let stem = match path.file_stem().and_then(|s| s.to_str()) {
        Some(s) => s,
        None => return Ok(None),
    };

    // Find trailing digits in stem
    let mut digit_start = stem.len();
    for (i, ch) in stem.char_indices().rev() {
        if ch.is_ascii_digit() {
            digit_start = i;
        } else {
            break;
        }
    }

    if digit_start == stem.len() {
        // No trailing digits -> not a sequence frame
        return Ok(None);
    }

    let number_str = &stem[digit_start..];
    let number = number_str
        .parse::<usize>()
        .map_err(|e| FrameError::Image(format!("Invalid frame number '{}': {}", number_str, e)))?;
    let prefix_local = &stem[..digit_start]; // e.g. "seq." or "seq_"
    let padding = number_str.len(); // Actual padding from filename

    // Build full prefix including parent directory
    let mut prefix = String::new();
    if let Some(parent) = path.parent() {
        let parent_str = parent.to_string_lossy();
        prefix.push_str(parent_str.as_ref());
        if !parent_str.ends_with(std::path::MAIN_SEPARATOR) {
            prefix.push(std::path::MAIN_SEPARATOR);
        }
    }
    prefix.push_str(prefix_local);

    Ok(Some((prefix, number, ext, padding)))
}

// =============================================================================
// Mode helpers (i8-based)
// =============================================================================

impl Comp {
    /// Check if comp is in file mode (loads from disk)
    #[inline]
    pub fn is_file_mode(&self) -> bool {
        self.attrs.get_i8(A_MODE).unwrap_or(COMP_NORMAL) == COMP_FILE
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{AttrValue, Project};

    fn file_comp(name: &str, start: i32, end: i32, fps: f32) -> Comp {
        let mut comp = Comp::new_file_comp("placeholder", start, end, fps);
        comp.attrs.set("name", AttrValue::Str(name.to_string()));
        comp.attrs.set("width", AttrValue::UInt(32));
        comp.attrs.set("height", AttrValue::UInt(32));
        comp
    }

    #[test]
    fn test_recursive_composition() {
        let manager = Arc::new(CacheManager::new(0.75, 2.0));
        let mut project = Project::new(manager);

        // Leaf: file-mode comp that yields placeholder frames
        let leaf = file_comp("Leaf", 0, 9, 24.0);
        let leaf_uuid = leaf.get_uuid();
        project.push_comps_order(leaf_uuid);
        project.media.write().unwrap().insert(leaf_uuid, leaf);

        // Middle: layer comp that references leaf
        let mut inner = Comp::new("Inner", 0, 9, 24.0);
        inner.add_child(leaf_uuid, 0, &project).unwrap();
        let inner_uuid = inner.get_uuid();
        project.push_comps_order(inner_uuid);
        project.media.write().unwrap().insert(inner_uuid, inner);

        // Root: layer comp that references inner
        let mut root = Comp::new("Root", 0, 9, 24.0);
        root.add_child(inner_uuid, 0, &project).unwrap();
        let root_uuid = root.get_uuid();
        project.media.write().unwrap().insert(root_uuid, root);

        let frame = {
            let media = project.media.read().unwrap();
            let root_ref = media.get(&root_uuid).unwrap();
            root_ref.get_frame(5, &project, false)
        };
        assert!(
            frame.is_some(),
            "Recursive composition should resolve a frame"
        );
    }

    #[test]
    fn test_dirty_tracking_on_attr_change() {
        let manager = Arc::new(CacheManager::new(0.75, 2.0));
        let project = Project::new(manager);

        // Source clip placeholder
        let clip = file_comp("Clip", 0, 4, 24.0);
        let clip_uuid = clip.get_uuid();
        project.media.write().unwrap().insert(clip_uuid, clip);

        // Comp with single child
        let mut comp = Comp::new("Test Comp", 0, 4, 24.0);
        comp.add_child(clip_uuid, 0, &project).unwrap();
        let comp_uuid = comp.get_uuid();
        project.media.write().unwrap().insert(comp_uuid, comp);

        // First render - should return a frame (composition works)
        // Note: With placeholder sources (not Loaded), frame is NOT cached
        {
            let media = project.media.read().unwrap();
            let comp_ref = media.get(&comp_uuid).unwrap();
            let frame = comp_ref.get_frame(2, &project, false);
            assert!(frame.is_some(), "Composition should return a frame");
            // With placeholder sources, frame is NOT cached (by design - prevents caching incomplete frames)
        }

        // Change child opacity - should mark attrs as dirty
        {
            let mut media = project.media.write().unwrap();
            let comp_mut = media.get_mut(&comp_uuid).unwrap();
            let child_uuid = comp_mut.children.first().map(|(u, _)| *u).unwrap();
            if let Some(attrs) = comp_mut.children_attrs_get_mut(&child_uuid) {
                attrs.set("opacity", AttrValue::Float(0.5));
                assert!(attrs.is_dirty(), "Attrs should be marked dirty after set()");
            }
        }

        // Second render should still work (composition recomputes)
        {
            let media = project.media.read().unwrap();
            let comp_ref = media.get(&comp_uuid).unwrap();
            let frame = comp_ref.get_frame(2, &project, false);
            assert!(frame.is_some(), "Recomposition should return a frame");
        }
    }

    /// Test dirty tracking behavior
    #[test]
    fn test_dirty_flag_behavior() {
        let mut attrs = Attrs::new();

        // Fresh attrs should not be dirty
        assert!(!attrs.is_dirty(), "Fresh attrs should not be dirty");

        // Setting a value should mark as dirty
        attrs.set("opacity", AttrValue::Float(1.0));
        assert!(attrs.is_dirty(), "Attrs should be dirty after set()");

        // Clearing dirty flag
        attrs.clear_dirty();
        assert!(!attrs.is_dirty(), "Attrs should be clean after clear_dirty()");

        // Manual mark_dirty
        attrs.mark_dirty();
        assert!(attrs.is_dirty(), "Attrs should be dirty after mark_dirty()");
    }

    #[test]
    fn test_multi_layer_blending_placeholder_sources() {
        let manager = Arc::new(CacheManager::new(0.75, 2.0));
        let mut project = Project::new(manager);

        // Three placeholder sources
        let mut sources: Vec<Uuid> = Vec::new();
        for i in 0..3 {
            let comp = file_comp(&format!("Src{}", i), 0, 4, 24.0);
            let uuid = comp.get_uuid();
            project.media.write().unwrap().insert(uuid, comp);
            sources.push(uuid);
        }

        // Parent comp blending three children with different opacities
        let mut comp = Comp::new("Blend", 0, 4, 24.0);
        for (idx, uuid) in sources.iter().enumerate() {
            comp.add_child(*uuid, 0, &project).unwrap();
            // Set opacity based on order
            let child_uuid = comp.children.last().unwrap().0;
            let opacity = match idx {
                0 => 1.0,
                1 => 0.5,
                _ => 0.3,
            };
            if let Some(attrs) = comp.children_attrs_get_mut(&child_uuid) {
                attrs.set("opacity", AttrValue::Float(opacity));
            }
        }

        let comp_uuid = comp.get_uuid();
        project.media.write().unwrap().insert(comp_uuid, comp);

        let frame = {
            let media = project.media.read().unwrap();
            let comp_ref = media.get(&comp_uuid).unwrap();
            comp_ref.get_frame(2, &project, false)
        };
        assert!(
            frame.is_some(),
            "Multi-layer composition with placeholder sources should succeed"
        );

        // With placeholder sources, the frame should NOT be cached (incomplete result)
        // GlobalFrameCache rejects frames that aren't fully Loaded
        assert!(
            !project.global_cache.as_ref().unwrap().contains(comp_uuid, 2),
            "Frame with placeholder sources should NOT be cached"
        );
    }

    #[test]
    fn test_frame_serialization() {
        // Create comp and set frame
        let mut comp = Comp::new("Test", 0, 100, 24.0);
        comp.set_frame(42);
        assert_eq!(comp.frame(), 42);

        // Serialize to JSON
        let json = serde_json::to_string(&comp).unwrap();
        // Frame is stored inside attrs.map, so look for the pattern in nested structure
        assert!(json.contains("\"frame\""), "JSON should contain frame key");

        // Deserialize back
        let restored: Comp = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.frame(), 42, "Frame should survive serialization round-trip");
    }

    // ==========================================================================
    // Time conversion tests (use existing UUID-based methods)
    // ==========================================================================

    fn make_child_attrs_with_uuid(start: i32, end: i32) -> (Uuid, Attrs) {
        let mut attrs = Attrs::new();
        let uuid = Uuid::new_v4();
        attrs.set(A_IN, AttrValue::Int(start));
        attrs.set(A_OUT, AttrValue::Int(end));
        attrs.set(A_SPEED, AttrValue::Float(1.0));
        (uuid, attrs)
    }

    #[test]
    fn test_comp2local_basic() {
        let mut comp = Comp::new_comp("Test", 0, 100, 24.0);
        let (child_uuid, child_attrs) = make_child_attrs_with_uuid(20, 80);
        comp.children.push((child_uuid, child_attrs));

        // comp_frame=50, child.in=20, speed=1.0
        // local = (50 - 20) * 1.0 = 30
        assert_eq!(comp.comp2local(child_uuid, 50), Some(30));
        assert_eq!(comp.comp2local(child_uuid, 20), Some(0));
        assert_eq!(comp.comp2local(child_uuid, 80), Some(60));
    }

    #[test]
    fn test_comp2local_with_speed() {
        let mut comp = Comp::new_comp("Test", 0, 100, 24.0);
        let (child_uuid, mut child_attrs) = make_child_attrs_with_uuid(20, 80);
        child_attrs.set(A_SPEED, AttrValue::Float(2.0));
        comp.children.push((child_uuid, child_attrs));

        // comp_frame=50, child.in=20, speed=2.0 (plays 2x faster)
        // local = (50 - 20) * 2.0 = 60
        assert_eq!(comp.comp2local(child_uuid, 50), Some(60));
    }

    #[test]
    fn test_local2comp_basic() {
        let mut comp = Comp::new_comp("Test", 0, 100, 24.0);
        let (child_uuid, child_attrs) = make_child_attrs_with_uuid(20, 80);
        comp.children.push((child_uuid, child_attrs));

        // local_frame=30, child.in=20, speed=1.0
        // comp = 20 + 30 / 1.0 = 50
        assert_eq!(comp.local2comp(child_uuid, 30), Some(50));
    }

    #[test]
    fn test_local2comp_with_speed() {
        let mut comp = Comp::new_comp("Test", 0, 100, 24.0);
        let (child_uuid, mut child_attrs) = make_child_attrs_with_uuid(20, 80);
        child_attrs.set(A_SPEED, AttrValue::Float(2.0));
        comp.children.push((child_uuid, child_attrs));

        // local_frame=60, child.in=20, speed=2.0
        // comp = 20 + 60 / 2.0 = 50
        assert_eq!(comp.local2comp(child_uuid, 60), Some(50));
    }

    #[test]
    fn test_time_roundtrip() {
        let mut comp = Comp::new_comp("Test", 0, 100, 24.0);
        let (child_uuid, child_attrs) = make_child_attrs_with_uuid(10, 90);
        // Use speed=1.0 for exact roundtrip (non-integer speeds have rounding errors)
        comp.children.push((child_uuid, child_attrs));

        for comp_frame in [10, 25, 50, 75, 90] {
            let local = comp.comp2local(child_uuid, comp_frame).unwrap();
            let back = comp.local2comp(child_uuid, local).unwrap();
            assert_eq!(back, comp_frame, "Roundtrip failed for frame {}", comp_frame);
        }
    }

    #[test]
    fn test_invalid_child_uuid() {
        let comp = Comp::new_comp("Test", 0, 100, 24.0);
        let fake_uuid = Uuid::new_v4();
        assert_eq!(comp.comp2local(fake_uuid, 50), None);
        assert_eq!(comp.local2comp(fake_uuid, 50), None);
    }

    #[test]
    fn test_negative_frames() {
        let mut comp = Comp::new_comp("Test", -50, 50, 24.0);
        let (child_uuid, child_attrs) = make_child_attrs_with_uuid(-20, 30);
        comp.children.push((child_uuid, child_attrs));

        // child.in=-20, comp_frame=0 => local = (0 - (-20)) * 1.0 = 20
        assert_eq!(comp.comp2local(child_uuid, 0), Some(20));
        assert_eq!(comp.comp2local(child_uuid, -20), Some(0));
    }

    #[test]
    fn test_mode_dispatch() {
        let mut comp = Comp::new_comp("Test", 0, 100, 24.0);
        assert_eq!(comp.attrs.get_i8(A_MODE), Some(COMP_NORMAL));
        assert!(!comp.is_file_mode()); // Layer mode by default

        // Set file mode directly via attrs
        comp.attrs.set_i8(A_MODE, COMP_FILE);
        comp.attrs.set(A_FILE_MASK, AttrValue::Str("/path/seq.*.exr".into()));
        assert_eq!(comp.attrs.get_i8(A_MODE), Some(COMP_FILE));
        assert!(comp.is_file_mode());
    }

    #[test]
    fn test_new_comp_all_attrs() {
        let comp = Comp::new_comp("TestComp", 10, 200, 30.0);

        // Identity
        assert!(comp.attrs.get_uuid(A_UUID).is_some());
        assert_eq!(comp.attrs.get_str(A_NAME), Some("TestComp"));
        assert_eq!(comp.attrs.get_i8(A_MODE), Some(COMP_NORMAL));

        // Timeline
        assert_eq!(comp.attrs.get_i32(A_IN), Some(10));
        assert_eq!(comp.attrs.get_i32(A_OUT), Some(200));
        assert_eq!(comp.attrs.get_float(A_FPS), Some(30.0));

        // Compose flags
        assert_eq!(comp.attrs.get_bool(A_SOLO), Some(false));
        assert_eq!(comp.attrs.get_bool(A_MUTE), Some(false));
        assert_eq!(comp.attrs.get_bool(A_VISIBLE), Some(true));

        // Transform
        assert!(comp.attrs.get(A_POSITION).is_some());

        // Playback
        assert_eq!(comp.attrs.get_float(A_SPEED), Some(1.0));

        // Relationships
        assert_eq!(comp.attrs.get_uuid(A_SOURCE_UUID), Some(Uuid::nil()));
        assert_eq!(comp.attrs.get_uuid(A_PARENT), Some(Uuid::nil()));

        // File mode (empty by default)
        assert_eq!(comp.attrs.get_str(A_FILE_MASK), Some(""));
    }
}
