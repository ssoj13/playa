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

use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use half::f16;
use eframe::egui;
use glob::glob;
use log::{debug, info};
use lru::LruCache;
use serde::{Deserialize, Serialize};

use crate::cache_man::CacheManager;
use crate::workers::Workers;

use super::frame::{CropAlign, Frame, FrameError, FrameStatus, PixelBuffer, PixelDepth, PixelFormat};
use super::loader::Loader;
use super::{AttrValue, Attrs};
use super::compositor::BlendMode;
use crate::entities::loader_video;
use crate::events::{CompEvent, CompEventSender};
use crate::utils::media;

/// Comp operating mode: Layer composition or File sequence loading
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CompMode {
    /// Layer mode: composes children comps (default)
    Layer,
    /// File mode: loads image sequence from disk
    File,
}

impl Default for CompMode {
    fn default() -> Self {
        CompMode::Layer
    }
}

/// Unified composition descriptor with dual-mode operation.
///
/// **Layer mode**: Composes children comps recursively
/// **File mode**: Loads image sequence from disk
///
/// All editable properties are stored in `attrs`:
/// - "name" (Str): Human-readable name
/// - "start" (UInt): Global start frame
/// - "end" (UInt): Global end frame
/// - "fps" (Float): Timeline framerate
/// - "play_start" (Int): Work area start (absolute comp frame)
/// - "play_end" (Int): Work area end (absolute comp frame)
///
/// **Transform attributes** (Vec3 or Float):
/// - "position" (Vec3): x, y, z position
/// - "rotation" (Vec3): euler angles (degrees)
/// - "scale" (Vec3): scale factors
/// - "pivot" (Vec3): pivot point
/// - "transparency" (Float): alpha (0.0 = transparent, 1.0 = opaque)
/// - "layer_mode" (Str): blend mode ("normal", "screen", "add", "subtract", "multiply", "divide")
/// - "speed" (Float): playback speed multiplier (1.0 = normal, 2.0 = double speed, 0.5 = half speed)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Comp {
    /// Stable identifier inside Project
    pub uuid: String,

    /// Operating mode: Layer or File
    #[serde(default)]
    pub mode: CompMode,

    /// Arbitrary attributes (all editable properties stored here)
    pub attrs: Attrs,

    // ===== Layer Mode Fields =====
    /// Parent composition UUID (if nested in another comp)
    #[serde(default)]
    pub parent: Option<String>,

    /// Children composition UUIDs (for Layer mode) - ordered list
    #[serde(default)]
    pub children: Vec<String>,

    /// Attributes for each child (start, end, play_start, play_end, opacity, etc.)
    #[serde(default)]
    pub children_attrs: HashMap<String, Attrs>,

    // ===== File Mode Fields =====
    /// File pattern for image sequence (e.g. "/path/seq.*.exr")
    /// Only used in File mode
    #[serde(default)]
    pub file_mask: Option<String>,

    /// First frame number in sequence
    /// Only used in File mode
    #[serde(default)]
    pub file_start: Option<i32>,

    /// Last frame number in sequence
    /// Only used in File mode
    #[serde(default)]
    pub file_end: Option<i32>,

    // ===== Common Fields =====
    /// Currently selected layers (layer_uuid from children_attrs)
    #[serde(default)]
    pub layer_selection: Vec<String>,
    #[serde(default)]
    pub layer_selection_anchor: Option<String>,

    /// Current playback position within this comp (persisted)
    #[serde(default)]
    pub current_frame: i32,

    /// Event sender for emitting comp events (runtime-only, rebuilt after deserialization)
    #[serde(skip)]
    #[serde(default)]
    event_sender: CompEventSender,

    /// Per-comp frame cache: (comp_hash, frame_idx) -> composed Frame (runtime-only)
    /// Uses LRU with memory-aware eviction (tracked by CacheManager)
    /// Hash invalidates cache when composition changes
    #[serde(skip)]
    #[serde(default = "Comp::default_cache")]
    cache: RefCell<LruCache<(u64, usize), Frame>>,

    /// Global cache manager (memory tracking + epoch)
    #[serde(skip)]
    cache_manager: Option<Arc<CacheManager>>,
}

impl Default for Comp {
    fn default() -> Self {
        Self::new("Untitled", 0, 100, 24.0)
    }
}

impl Comp {
    /// Default cache for serde deserialization
    fn default_cache() -> RefCell<LruCache<(u64, usize), Frame>> {
        RefCell::new(LruCache::new(NonZeroUsize::new(10000).unwrap()))
    }

    /// Create new composition in Layer mode (default)
    pub fn new(name: impl Into<String>, start: i32, end: i32, fps: f32) -> Self {
        let mut attrs = Attrs::new();
        attrs.set("name", AttrValue::Str(name.into()));
        attrs.set("start", AttrValue::Int(start));
        attrs.set("end", AttrValue::Int(end));
        attrs.set("fps", AttrValue::Float(fps));
        attrs.set("play_start", AttrValue::Int(start)); // Full range by default (absolute)
        attrs.set("play_end", AttrValue::Int(end)); // Full range by default (absolute)

        // Transform defaults
        attrs.set("visible", AttrValue::Bool(true));
        attrs.set("transparency", AttrValue::Float(1.0)); // Fully opaque
        attrs.set("layer_mode", AttrValue::Str("normal".to_string()));
        attrs.set("speed", AttrValue::Float(1.0)); // Normal speed

        Self {
            uuid: uuid::Uuid::new_v4().to_string(),
            mode: CompMode::Layer,
            attrs,
            parent: None,
            children: Vec::new(),
            children_attrs: HashMap::new(),
            file_mask: None,
            file_start: None,
            file_end: None,
            current_frame: start,
            layer_selection: Vec::new(),
            layer_selection_anchor: None,
            event_sender: CompEventSender::dummy(),
            cache: RefCell::new(LruCache::new(NonZeroUsize::new(10000).unwrap())),
            cache_manager: None,
        }
    }

    /// Create new composition in File mode for loading image sequences
    pub fn new_file_comp(pattern: impl Into<String>, start: i32, end: i32, fps: f32) -> Self {
        let mut comp = Self::new("File Comp", start, end, fps);
        comp.mode = CompMode::File;
        comp.file_mask = Some(pattern.into());
        comp.file_start = Some(start);
        comp.file_end = Some(end);
        comp
    }

    // Getters for attrs-based properties
    pub fn name(&self) -> &str {
        self.attrs.get_str("name").unwrap_or("Untitled")
    }

    pub fn start(&self) -> i32 {
        self.attrs.get_i32("start").unwrap_or(0)
    }

    pub fn end(&self) -> i32 {
        self.attrs.get_i32("end").unwrap_or(100)
    }

    pub fn fps(&self) -> f32 {
        self.attrs.get_float("fps").unwrap_or(24.0)
    }

    pub fn play_start(&self) -> i32 {
        self.attrs.get_i32("play_start").unwrap_or_else(|| self.start())
    }

    pub fn play_end(&self) -> i32 {
        self.attrs.get_i32("play_end").unwrap_or_else(|| self.end())
    }

    // Setters for attrs-based properties
    pub fn set_name(&mut self, name: impl Into<String>) {
        self.attrs.set("name", AttrValue::Str(name.into()));
    }

    pub fn set_start(&mut self, start: i32) {
        self.attrs.set("start", AttrValue::Int(start));
    }

    pub fn set_end(&mut self, end: i32) {
        self.attrs.set("end", AttrValue::Int(end));
    }

    // Layer UUID <-> Index conversion helpers
    /// Convert layer UUID to index in children array
    pub fn uuid_to_idx(&self, uuid: &str) -> Option<usize> {
        self.children.iter().position(|u| u == uuid)
    }

    /// Convert index to layer UUID
    pub fn idx_to_uuid(&self, idx: usize) -> Option<&String> {
        self.children.get(idx)
    }

    /// Convert multiple UUIDs to indices
    pub fn uuids_to_indices(&self, uuids: &[String]) -> Vec<usize> {
        uuids.iter()
            .filter_map(|uuid| self.uuid_to_idx(uuid))
            .collect()
    }

    /// Convert multiple indices to UUIDs
    pub fn indices_to_uuids(&self, indices: &[usize]) -> Vec<String> {
        indices.iter()
            .filter_map(|&idx| self.idx_to_uuid(idx).cloned())
            .collect()
    }

    // Domain-specific helpers for layer attributes

    /// Get child layer's start position
    pub fn child_start(&self, child_uuid: &str) -> i32 {
        self.children_attrs
            .get(child_uuid)
            .map(|a| a.get_i32_or_zero("start"))
            .unwrap_or(0)
    }

    /// Get child layer's end position
    pub fn child_end(&self, child_uuid: &str) -> i32 {
        self.children_attrs
            .get(child_uuid)
            .map(|a| a.get_i32_or_zero("end"))
            .unwrap_or(0)
    }

    /// Get child layer's play_start (with fallback to start)
    pub fn child_play_start(&self, child_uuid: &str) -> i32 {
        self.children_attrs
            .get(child_uuid)
            .map(|a| {
                let start = a.get_i32_or_zero("start");
                a.get_i32_or("play_start", start)
            })
            .unwrap_or(0)
    }

    /// Get child layer's play_end (with fallback to end)
    pub fn child_play_end(&self, child_uuid: &str) -> i32 {
        self.children_attrs
            .get(child_uuid)
            .map(|a| {
                let end = a.get_i32_or_zero("end");
                a.get_i32_or("play_end", end)
            })
            .unwrap_or(0)
    }

    /// Check if a specific layer is in selection
    pub fn is_layer_selected(&self, layer_uuid: &str) -> bool {
        self.layer_selection.contains(&layer_uuid.to_string())
    }

    /// Check if layer is selected and part of multi-selection
    pub fn is_multi_selected(&self, layer_uuid: &str) -> bool {
        !self.layer_selection.is_empty()
            && self.is_layer_selected(layer_uuid)
            && self.layer_selection.len() > 1
    }

    pub fn set_fps(&mut self, fps: f32) {
        self.attrs.set("fps", AttrValue::Float(fps));
    }

    pub fn set_play_start(&mut self, play_start: i32) {
        self.attrs.set("play_start", AttrValue::Int(play_start));
    }

    pub fn set_play_end(&mut self, play_end: i32) {
        self.attrs.set("play_end", AttrValue::Int(play_end));
    }

    /// Inclusive play range (work area) in absolute comp frames.
    ///
    /// Returns [start, end] inclusive in timeline coordinates.
    pub fn play_range(&self, use_work_area: bool) -> (i32, i32) {
        // If comp has children (Layer mode), calculate bounds from them (with their trims)
        if !self.children.is_empty() {
            let mut min_frame = i32::MAX;
            let mut max_frame = i32::MIN;

            for child_uuid in &self.children {
                if let Some(attrs) = self.children_attrs.get(child_uuid) {
                    let child_start = attrs.get_i32("start").unwrap_or(0);
                    let child_end = attrs.get_i32("end").unwrap_or(child_start);
                    let child_play_start = attrs.get_i32("play_start").unwrap_or(child_start);
                    let child_play_end = attrs.get_i32("play_end").unwrap_or(child_end);

                    min_frame = min_frame.min(child_play_start);
                    max_frame = max_frame.max(child_play_end);
                }
            }

            if min_frame != i32::MAX && max_frame != i32::MIN {
                let (base_start, base_end) = (min_frame, max_frame);
                if use_work_area {
                    let work_start = self
                        .attrs
                        .get_i32("play_start")
                        .unwrap_or(base_start)
                        .clamp(base_start, base_end);
                    let work_end = self
                        .attrs
                        .get_i32("play_end")
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
        let start = self.start();
        let end = self.end();
        if !use_work_area {
            return (start, end);
        }

        let work_start = self.attrs.get_i32("play_start").unwrap_or(start);
        let work_end = self.attrs.get_i32("play_end").unwrap_or(end);
        let clamped_start = work_start.clamp(start, end);
        let clamped_end = work_end.clamp(clamped_start, end);
        (clamped_start, clamped_end)
    }

    /// Set comp work area in absolute frames. Automatically clamps/order-fixes.
    pub fn set_work_area_abs(&mut self, start: i32, end: i32) {
        let (comp_start, comp_end) = (self.start(), self.end());
        let lo = start.min(end).clamp(comp_start, comp_end);
        let hi = end.max(start).clamp(lo, comp_end);
        self.set_play_start(lo);
        self.set_play_end(hi);
        self.clear_cache();
        self.event_sender.emit(CompEvent::LayersChanged {
            comp_uuid: self.uuid.clone(),
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
    pub fn child_work_area_abs(&self, child_uuid: &str) -> Option<(i32, i32)> {
        let attrs = self.children_attrs.get(child_uuid)?;
        let (bounds_start, bounds_end) =
            Self::child_bounds_abs(attrs.get_i32("start"), attrs.get_i32("end"));
        let play_start = attrs.get_i32("play_start").unwrap_or(bounds_start);
        let play_end = attrs.get_i32("play_end").unwrap_or(bounds_end);
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

    /// Number of frames in full composition (not limited by play_area)
    pub fn frame_count(&self) -> i32 {
        let start = self.start();
        let end = self.end();
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

    /// Return cached frame statuses for File comps, aligned to local frame indices.
    ///
    /// Uses per-comp cache entries to build a strip of statuses:
    /// - Default is `Header` (expected file, not yet loaded).
    /// - Cached frames override with their current status.
    /// Skips Layer comps and empty comps.
    pub fn file_frame_statuses(&self) -> Option<Vec<FrameStatus>> {
        if self.mode != CompMode::File {
            log::debug!("file_frame_statuses: mode is not File (mode={:?})", self.mode);
            return None;
        }

        let duration = self.frame_count();
        if duration <= 0 {
            log::debug!("file_frame_statuses: duration <= 0 (duration={})", duration);
            return None;
        }

        log::debug!("file_frame_statuses: returning statuses for {} frames", duration);

        let seq_start = self.file_start.unwrap_or(self.start());
        let mut statuses = vec![FrameStatus::Header; duration as usize];

        for ((_, seq_frame), frame) in self.cache.borrow().iter() {
            let seq_frame_i32 = *seq_frame as i32;
            let local_idx = seq_frame_i32 - seq_start;
            if local_idx < 0 || local_idx >= duration {
                continue;
            }

            if let Some(slot) = statuses.get_mut(local_idx as usize) {
                *slot = frame.status();
            }
        }

        Some(statuses)
    }

    /// Set global cache manager (called once after creation)
    pub fn set_cache_manager(&mut self, manager: Arc<CacheManager>) {
        self.cache_manager = Some(manager);
    }

    /// Clear per-comp frame cache with memory tracking
    pub fn clear_cache(&self) {
        // Free memory for all cached frames
        if let Some(ref manager) = self.cache_manager {
            for (_, frame) in self.cache.borrow().iter() {
                let size = frame.mem();
                manager.free_memory(size);
            }
        }
        self.cache.borrow_mut().clear();
    }

    /// Signal background preload for frames around current position
    ///
    /// Increments epoch to cancel stale requests, determines preload strategy
    /// (spiral vs forward) based on file type, and enqueues frames for loading.
    ///
    /// Strategies:
    /// - Spiral: image sequences (0, ±1, ±2, ...) - cheap seeking both directions
    /// - Forward: video files (center → end) - expensive backward seeking
    pub fn signal_preload(&self, workers: &Arc<Workers>) {
        // Only preload in File mode
        if self.mode != CompMode::File {
            return;
        }

        // Increment epoch to cancel stale preload requests
        let epoch = if let Some(ref manager) = self.cache_manager {
            manager.increment_epoch()
        } else {
            return;
        };

        let center = self.current_frame;
        let (play_start, play_end) = self.work_area_abs(true);

        if play_end < play_start {
            return;
        }

        // Determine strategy based on file type (video vs image sequence)
        let is_video = self.detect_video_at_frame(center);
        let strategy = if is_video {
            crate::cache_man::PreloadStrategy::Forward
        } else {
            crate::cache_man::PreloadStrategy::Spiral
        };

        debug!(
            "Preload epoch {}: center={}, range={}..{}, strategy={:?}",
            epoch, center, play_start, play_end, strategy
        );

        match strategy {
            crate::cache_man::PreloadStrategy::Spiral => {
                self.preload_spiral(workers, epoch, center, play_start, play_end);
            }
            crate::cache_man::PreloadStrategy::Forward => {
                self.preload_forward(workers, epoch, center, play_start, play_end);
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
        epoch: u64,
        center: i32,
        play_start: i32,
        play_end: i32,
    ) {
        let max_offset = ((play_end - play_start) / 2).max(0);

        for offset in 0..=max_offset {
            // Backward: center - offset
            if center >= offset {
                let idx = center - offset;
                if idx >= play_start && idx <= play_end {
                    self.enqueue_load(workers, epoch, idx);
                }
            }

            // Forward: center + offset (skip offset=0 as already loaded)
            if offset > 0 {
                let idx = center + offset;
                if idx >= play_start && idx <= play_end {
                    self.enqueue_load(workers, epoch, idx);
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
        epoch: u64,
        center: i32,
        play_start: i32,
        play_end: i32,
    ) {
        let start = center.max(play_start);
        for idx in start..=play_end {
            self.enqueue_load(workers, epoch, idx);
        }
    }

    /// Enqueue single frame for background loading with epoch check
    ///
    /// Skips frames that are already loaded or have no backing file.
    /// Uses execute_with_epoch() to automatically cancel stale requests.
    fn enqueue_load(&self, workers: &Arc<Workers>, epoch: u64, frame_idx: i32) {
        // Get frame (creates placeholder if needed)
        let comp_hash = self.compute_comp_hash();
        let key = (comp_hash, frame_idx as usize);

        // Check if already cached
        if let Some(_frame) = self.cache.borrow().peek(&key) {
            // Already in cache, skip
            return;
        }

        // Resolve path
        let Some(path) = self.resolve_frame_path(frame_idx) else {
            return;
        };

        // Create frame with unloaded status
        let frame = Frame::new_unloaded(path);

        // Enqueue with epoch check
        workers.execute_with_epoch(epoch, move || {
            // Load frame by setting status to Loaded
            // This triggers actual image loading via Frame::set_status()
            if let Err(e) = frame.set_status(FrameStatus::Loaded) {
                debug!("Failed to load frame {}: {}", frame_idx, e);
            }
        });
    }

    /// Insert frame into cache with LRU eviction and memory tracking
    fn cache_insert(&self, key: (u64, usize), frame: Frame) {
        let frame_size = frame.mem();

        // Perform LRU eviction if memory limit exceeded
        if let Some(ref manager) = self.cache_manager {
            while manager.check_memory_limit() {
                let mut cache = self.cache.borrow_mut();
                if let Some((_, evicted)) = cache.pop_lru() {
                    let evicted_size = evicted.mem();
                    manager.free_memory(evicted_size);
                    debug!(
                        "LRU evicted frame: freed {} MB (usage: {} MB / {} MB)",
                        evicted_size / 1024 / 1024,
                        manager.mem().0 / 1024 / 1024,
                        manager.mem().1 / 1024 / 1024
                    );
                } else {
                    break; // Cache empty, can't evict more
                }
            }

            // Track new frame memory
            manager.add_memory(frame_size);
        }

        // Insert into LRU cache
        self.cache.borrow_mut().push(key, frame);
    }

    /// Compute hash of composition configuration for cache invalidation.
    /// Hash changes based on mode:
    /// - File mode: file_mask, file_start, file_end
    /// - Layer mode: children UUIDs and layers (legacy, will be removed)
    /// Also hashes transform attributes (transparency, layer_mode, speed).
    fn compute_comp_hash(&self) -> u64 {
        let mut hasher = DefaultHasher::new();

        // Hash mode
        match self.mode {
            CompMode::Layer => 0u8.hash(&mut hasher),
            CompMode::File => 1u8.hash(&mut hasher),
        }

        match self.mode {
            CompMode::File => {
                // Hash file sequence parameters
                if let Some(ref mask) = self.file_mask {
                    mask.hash(&mut hasher);
                }
                self.file_start.hash(&mut hasher);
                self.file_end.hash(&mut hasher);
            }
            CompMode::Layer => {
                // Hash children UUIDs (order matters) and their full attribute sets
                self.children.len().hash(&mut hasher);
                for child_uuid in &self.children {
                    child_uuid.hash(&mut hasher);

                    // Hash child attributes if present
                    if let Some(attrs) = self.children_attrs.get(child_uuid) {
                        attrs.hash_all().hash(&mut hasher);
                    }
                }
            }
        }

        // Hash transform attributes
        let transparency_bits = self
            .attrs
            .get_float("transparency")
            .unwrap_or(1.0)
            .to_bits();
        transparency_bits.hash(&mut hasher);

        if let Some(layer_mode) = self.attrs.get_str("layer_mode") {
            layer_mode.hash(&mut hasher);
        }

        let speed_bits = self.attrs.get_float("speed").unwrap_or(1.0).to_bits();
        speed_bits.hash(&mut hasher);

        hasher.finish()
    }

    /// Set event sender (called after deserialization or when creating new comp in app)
    pub fn set_event_sender(&mut self, sender: CompEventSender) {
        self.event_sender = sender;
    }

    /// Set current frame and emit CurrentFrameChanged event.
    ///
    /// This is the proper way to change frame position - emits event that triggers frame loading.
    pub fn set_current_frame(&mut self, new_frame: i32) {
        let old_frame = self.current_frame;
        if old_frame != new_frame {
            self.current_frame = new_frame;

            // Emit event
            self.event_sender.emit(CompEvent::CurrentFrameChanged {
                comp_uuid: self.uuid.clone(),
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
    pub fn get_frame(&self, frame_idx: i32, project: &super::Project) -> Option<Frame> {
        match self.mode {
            CompMode::File => self.get_file_frame(frame_idx),
            CompMode::Layer => self.get_layer_frame(frame_idx, project),
        }
    }

    fn get_file_frame(&self, frame_idx: i32) -> Option<Frame> {
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

        let comp_start = self.start();
        let comp_end = self.end();
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
        let seq_start = self.file_start.unwrap_or(self.start());
        let seq_end = self.file_end.unwrap_or(self.end());
        let seq_frame = seq_start.saturating_add(local_idx);
        if seq_frame < seq_start || seq_frame > seq_end {
            return Some(self.placeholder_frame());
        }

        // Cache key uses sequence frame number to avoid collisions when start shifts
        let cache_key = (self.compute_comp_hash(), seq_frame.max(0) as usize);

        // Check cache (LRU::get is mutating, updates access order)
        if let Some(frame) = self.cache.borrow_mut().get(&cache_key) {
            return Some(frame.clone());
        }

        let frame_path = self.resolve_frame_path(seq_frame).unwrap_or_default();
        if frame_path.as_os_str().is_empty() {
            return Some(self.placeholder_frame());
        }

        let frame = self.frame_from_path(frame_path);

        // Insert with memory-aware eviction
        self.cache_insert(cache_key, frame.clone());

        Some(frame)
    }

    fn get_layer_frame(&self, frame_idx: i32, project: &super::Project) -> Option<Frame> {
        // Check if frame is within play area (work area)
        let (play_start, play_end) = self.play_range(true);
        if frame_idx < play_start || frame_idx > play_end {
            return None; // Frame outside work area - don't compose
        }

        // Compute composition hash for cache key
        let comp_hash = self.compute_comp_hash();
        let cache_key = (comp_hash, frame_idx.max(0) as usize); // Cache key uses positive values

        // Check cache (LRU::get is mutating)
        if let Some(frame) = self.cache.borrow_mut().get(&cache_key) {
            return Some(frame.clone());
        }

        // Compose frame recursively
        let composed = self.compose(frame_idx, project)?;

        // Insert with memory-aware eviction
        self.cache_insert(cache_key, composed.clone());

        Some(composed)
    }

    fn resolve_frame_path(&self, frame_number: i32) -> Option<PathBuf> {
        let mask = self.file_mask.as_ref()?;
        if media::is_video(Path::new(mask)) {
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
        for child_uuid in &self.children {
            if let Some(attrs) = self.children_attrs.get(child_uuid) {
                let start = attrs.get_i32("start").unwrap_or(0);
                let w = attrs.get_u32("width").unwrap_or(0) as usize;
                let h = attrs.get_u32("height").unwrap_or(0) as usize;
                match best {
                    None => best = Some((start, w, h)),
                    Some((best_start, _, _)) if start < best_start => best = Some((start, w, h)),
                    _ => {}
                }
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
    /// - Blends multiple children with CPU compositor (GPU compositor planned)
    fn compose(&self, frame_idx: i32, project: &super::Project) -> Option<Frame> {
        use log::debug;
        let mut source_frames: Vec<(Frame, f32, BlendMode)> = Vec::new();
        let mut earliest: Option<(i32, usize)> = None; // (start_frame, index in source_frames)
        let mut target_format: PixelFormat = PixelFormat::Rgba8;

        debug!(
            "compose() called: frame_idx={}, children.len()={}",
            frame_idx,
            self.children.len()
        );

        // Collect frames from all active children
        // IMPORTANT: Reverse iteration - last child (bottom layer) becomes base,
        // first child (top layer) composited last
        for child_uuid in self.children.iter().rev() {
            // Get child attributes
            let attrs = self.children_attrs.get(child_uuid)?;

            // Placement and bounds in parent timeline
            let child_start = attrs.get_i32("start").unwrap_or(0);
            let child_end = attrs.get_i32("end").unwrap_or(child_start);
            let duration = (child_end - child_start + 1).max(0);
            let (play_start, play_end) = self
                .child_work_area_abs(child_uuid)
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
            let Some(source_uuid) = attrs.get_str("uuid") else {
                continue;
            };

            // Resolve source from Project.media
            if let Some(source) = project.media.get(source_uuid) {
                // Visibility toggle
                if attrs.get_bool("visible").unwrap_or(true) == false {
                    continue;
                }

                // Map parent frame to source frame: anchor at source.start()
                let offset = frame_idx - child_start;
                if offset < 0 || offset >= duration {
                    continue;
                }
                let source_frame = source.start() + offset;

                // Recursively get frame from source (Clip or Comp)
                if let Some(frame) = source.get_frame(source_frame, project) {
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

        for (frame, opacity, _mode) in source_frames.iter_mut() {
            *frame = promote_frame(frame, target_format);
            *opacity = *opacity;
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
            "compose() collected {} frames, calling compositor.blend_with_dim({}, {})",
            source_frames.len(),
            dim.0,
            dim.1
        );
        project.compositor.borrow_mut().blend_with_dim(source_frames, dim)
    }

    /// Add a new child to the composition at specified start frame.
    ///
    /// Automatically determines duration from source and creates child attributes.
    /// Add child by looking up source from project
    pub fn add_child(
        &mut self,
        source_uuid: String,
        start_frame: i32,
        project: &super::Project,
    ) -> anyhow::Result<()> {
        // Get source to determine duration
        let source = project
            .media
            .get(&source_uuid)
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
        source_uuid: String,
        start_frame: i32,
        duration: i32,
        target_row: Option<usize>,
        source_dim: (usize, usize),
    ) -> anyhow::Result<()> {
        let end_frame = start_frame + duration - 1;

        // Generate unique instance UUID for this child
        let instance_uuid = uuid::Uuid::new_v4().to_string();

        // Create child attributes
        let mut attrs = Attrs::new();
        attrs.set("uuid", AttrValue::Str(source_uuid)); // Reference to source comp
        attrs.set("name", AttrValue::Str("Child".to_string()));
        attrs.set("start", AttrValue::Int(start_frame));
        attrs.set("end", AttrValue::Int(end_frame));
        // Work area defaults to full placement range in parent timeline
        attrs.set("play_start", AttrValue::Int(start_frame));
        attrs.set("play_end", AttrValue::Int(end_frame));
        attrs.set("opacity", AttrValue::Float(1.0));
        attrs.set("visible", AttrValue::Bool(true));
        attrs.set("blend_mode", AttrValue::Str("normal".to_string()));
        attrs.set("speed", AttrValue::Float(1.0));
        attrs.set("width", AttrValue::UInt(source_dim.0 as u32));
        attrs.set("height", AttrValue::UInt(source_dim.1 as u32));

        // Add to children at appropriate position for target row
        if let Some(target_row) = target_row {
            let insert_pos = self.find_insert_position_for_row(target_row);
            self.children.insert(insert_pos, instance_uuid.clone());
        } else {
            self.children.push(instance_uuid.clone());
        }
        self.children_attrs.insert(instance_uuid, attrs);

        self.rebound();
        self.update_dim_from_children();
        // Clear cache and emit event
        self.clear_cache();
        self.event_sender.emit(CompEvent::LayersChanged {
            comp_uuid: self.uuid.clone(),
        });

        Ok(())
    }

    /// Find insertion position in children array to achieve target visual row
    fn find_insert_position_for_row(&self, target_row: usize) -> usize {
        use std::collections::HashMap;

        // Compute current layout for all existing children
        let mut layer_rows: HashMap<usize, usize> = HashMap::new();
        let mut occupied_rows: HashMap<usize, Vec<(i32, i32)>> = HashMap::new();

        for (idx, child_uuid) in self.children.iter().enumerate() {
            let attrs = self.children_attrs.get(child_uuid);
            let start = attrs
                .and_then(|a| Some(a.get_i32("start").unwrap_or(0)))
                .unwrap_or(0);
            let end = attrs
                .and_then(|a| Some(a.get_i32("end").unwrap_or(0)))
                .unwrap_or(0);

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
        for (idx, _child_uuid) in self.children.iter().enumerate() {
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
        let child_uuid = self
            .children
            .get(child_idx)
            .ok_or_else(|| anyhow::anyhow!("Child {} not found", child_idx))?
            .clone();

        let attrs = self
            .children_attrs
            .get_mut(&child_uuid)
            .ok_or_else(|| anyhow::anyhow!("Child attrs not found"))?;

        let old_start = attrs.get_i32("start").unwrap_or(0);
        let old_end = attrs.get_i32("end").unwrap_or(0);
        let duration = (old_end - old_start).max(0);
        let new_end = new_start + duration;
        let delta = new_start - old_start;

        let play_start_old = attrs.get_i32("play_start").unwrap_or(old_start);
        let play_end_old = attrs.get_i32("play_end").unwrap_or(old_end);

        attrs.set("start", AttrValue::Int(new_start));
        attrs.set("end", AttrValue::Int(new_end));

        // Shift work area by the same delta, clamped to new bounds
        let shifted_start = play_start_old + delta;
        let shifted_end = play_end_old + delta;
        let (clamped_start, clamped_end) =
            Self::clamp_range_to_bounds((shifted_start, shifted_end), (new_start, new_end));
        attrs.set("play_start", AttrValue::Int(clamped_start));
        attrs.set("play_end", AttrValue::Int(clamped_end));

        self.rebound();
        self.update_dim_from_children();

        // Clear cache and emit event
        self.clear_cache();
        self.event_sender.emit(CompEvent::LayersChanged {
            comp_uuid: self.uuid.clone(),
        });

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
        let selected_uuids: Vec<String> = self.layer_selection.clone();

        // Build block of UUIDs
        let mut block: Vec<String> = Vec::new();
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
        for uuid in block.iter() {
            reordered.insert(cursor, uuid.clone());
            cursor += 1;
        }
        self.children = reordered;

        // layer_selection stores UUIDs - no need to update after reorder
        // Just restore the UUIDs that still exist in children
        self.layer_selection = selected_uuids
            .into_iter()
            .filter(|uuid| self.children.contains(uuid))
            .collect();

        // Move each by delta (preserve relative offsets)
        for uuid in block {
            if let Some(idx) = self.children.iter().position(|u| u == &uuid) {
                let current_start = self
                    .children_attrs
                    .get(&uuid)
                    .and_then(|a| Some(a.get_i32("start").unwrap_or(0)))
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

        for idx in idxs {
            if idx >= self.children.len() {
                continue;
            }
            if let Some(child_uuid) = self.children.get(idx) {
                let (bounds_start, bounds_end) = {
                    let attrs = self.children_attrs.get(child_uuid);
                    Self::child_bounds_abs(attrs.and_then(|a| a.get_i32("start")), attrs.and_then(|a| a.get_i32("end")))
                };
                if let Some(attrs) = self.children_attrs.get_mut(child_uuid) {
                    if is_start {
                        let current = attrs.get_i32("play_start").unwrap_or(bounds_start);
                        let (clamped_start, clamped_end) = Self::clamp_range_to_bounds(
                            (current + delta, attrs.get_i32("play_end").unwrap_or(bounds_end)),
                            (bounds_start, bounds_end),
                        );
                        attrs.set("play_start", AttrValue::Int(clamped_start));
                        attrs.set("play_end", AttrValue::Int(clamped_end));
                    } else {
                        let current = attrs.get_i32("play_end").unwrap_or(bounds_end);
                        let (clamped_start, clamped_end) = Self::clamp_range_to_bounds(
                            (attrs.get_i32("play_start").unwrap_or(bounds_start), current + delta),
                            (bounds_start, bounds_end),
                        );
                        attrs.set("play_start", AttrValue::Int(clamped_start));
                        attrs.set("play_end", AttrValue::Int(clamped_end));
                    }
                }
            }
        }

        // Clear cache and emit event once
        self.clear_cache();
        self.event_sender.emit(CompEvent::LayersChanged {
            comp_uuid: self.uuid.clone(),
        });
        Ok(())
    }

    /// Set child play start (adjust play_start attribute - visible start offset from child start).
    pub fn set_child_play_start(
        &mut self,
        child_idx: usize,
        new_play_start: i32,
    ) -> anyhow::Result<()> {
        let child_uuid = self
            .children
            .get(child_idx)
            .ok_or_else(|| anyhow::anyhow!("Child {} not found", child_idx))?
            .clone();

        let attrs = self
            .children_attrs
            .get_mut(&child_uuid)
            .ok_or_else(|| anyhow::anyhow!("Child attrs not found"))?;

        let (bounds_start, bounds_end) =
            Self::child_bounds_abs(attrs.get_i32("start"), attrs.get_i32("end"));
        let current_end = attrs.get_i32("play_end").unwrap_or(bounds_end);
        let (clamped_start, clamped_end) =
            Self::clamp_range_to_bounds((new_play_start, current_end), (bounds_start, bounds_end));
        attrs.set("play_start", AttrValue::Int(clamped_start));
        attrs.set("play_end", AttrValue::Int(clamped_end));

        // Clear cache and emit event
        self.clear_cache();
        self.event_sender.emit(CompEvent::LayersChanged {
            comp_uuid: self.uuid.clone(),
        });

        Ok(())
    }

    /// Set child play end (adjust play_end attribute - visible end offset from child end).
    pub fn set_child_play_end(
        &mut self,
        child_idx: usize,
        new_play_end: i32,
    ) -> anyhow::Result<()> {
        let child_uuid = self
            .children
            .get(child_idx)
            .ok_or_else(|| anyhow::anyhow!("Child {} not found", child_idx))?
            .clone();

        let attrs = self
            .children_attrs
            .get_mut(&child_uuid)
            .ok_or_else(|| anyhow::anyhow!("Child attrs not found"))?;

        let (bounds_start, bounds_end) =
            Self::child_bounds_abs(attrs.get_i32("start"), attrs.get_i32("end"));
        let current_start = attrs.get_i32("play_start").unwrap_or(bounds_start);
        let (clamped_start, clamped_end) =
            Self::clamp_range_to_bounds((current_start, new_play_end), (bounds_start, bounds_end));
        attrs.set("play_start", AttrValue::Int(clamped_start));
        attrs.set("play_end", AttrValue::Int(clamped_end));

        // Clear cache and emit event
        self.clear_cache();
        self.event_sender.emit(CompEvent::LayersChanged {
            comp_uuid: self.uuid.clone(),
        });

        Ok(())
    }

    /// Set comp play start in absolute comp frames (inclusive).
    /// Ensures `play_end` remains >= start and clamps to comp bounds.
    pub fn set_comp_play_start(&mut self, new_play_start: i32) {
        let current_end = self.play_end();
        self.set_work_area_abs(new_play_start, current_end);
    }

    /// Set comp play end in absolute comp frames (inclusive).
    /// Ensures `play_start` remains <= end and clamps to comp bounds.
    pub fn set_comp_play_end(&mut self, new_play_end: i32) {
        let current_start = self.play_start();
        self.set_work_area_abs(current_start, new_play_end);
    }

    /// Get all child edges (start and end frames) sorted by distance from given frame
    /// Returns vec of (frame_number, is_start) tuples
    pub fn get_child_edges_near(&self, _from_frame: i32) -> Vec<(i32, bool)> {
        let mut edges = Vec::new();

        for child_uuid in &self.children {
            if let Some(attrs) = self.children_attrs.get(child_uuid) {
                let start = attrs.get_i32("start").unwrap_or(0);
                let end = attrs.get_i32("end").unwrap_or(0);
                let play_start = attrs.get_i32("play_start").unwrap_or(start);
                let play_end = attrs.get_i32("play_end").unwrap_or(end);

                // Visible range accounting for play range offsets
                let visible_start = play_start;
                let visible_end = play_end;

                if visible_start <= visible_end {
                    edges.push((visible_start, true)); // Start edge
                    edges.push((visible_end, false)); // End edge
                }
            }
        }

        // Sort by frame number to allow deterministic next/previous jumps
        edges.sort_by_key(|(frame, _)| *frame);
        edges.dedup_by_key(|(frame, _)| *frame);

        edges
    }

    // ===== Parent-Child Management =====

    /// Remove child comp from this composition
    pub fn remove_child(&mut self, child_uuid: &str) {
        self.children.retain(|uuid| uuid != child_uuid);
        self.children_attrs.remove(child_uuid);
        self.rebound();
        self.update_dim_from_children();
        self.invalidate_cache();
        self.event_sender.emit(CompEvent::LayersChanged {
            comp_uuid: self.uuid.clone(),
        });
    }

    /// Recalculate comp start/end based on children (negative starts allowed).
    pub fn rebound(&mut self) {
        // File-mode comps have their own start/end; don't override them.
        if self.mode == CompMode::File {
            return;
        }
        let old_bounds = (self.start(), self.end());
        let old_work = self.play_range(true);
        if self.children.is_empty() {
            // Default span when no children: 0..100 for a visible timeline
            self.attrs.set("start", AttrValue::Int(0));
            self.attrs.set("end", AttrValue::Int(100));
            if old_work == old_bounds {
                self.attrs.set("play_start", AttrValue::Int(0));
                self.attrs.set("play_end", AttrValue::Int(100));
            }
            return;
        }

        let mut min_start = i32::MAX;
        let mut max_end = i32::MIN;

        for child_uuid in &self.children {
            if let Some((visible_start, visible_end)) = self.child_work_area_abs(child_uuid) {
                min_start = min_start.min(visible_start);
                max_end = max_end.max(visible_end);
            }
        }

        let (new_start, new_end) = if min_start == i32::MAX || max_end == i32::MIN {
            (0, 0)
        } else {
            (min_start, max_end)
        };

        self.attrs.set("start", AttrValue::Int(new_start));
        self.attrs.set("end", AttrValue::Int(new_end));

        // Keep work area in sync only if it used to match full bounds
        if old_work == old_bounds {
            self.attrs.set("play_start", AttrValue::Int(new_start));
            self.attrs.set("play_end", AttrValue::Int(new_end));
        }
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

    /// Set parent composition UUID
    pub fn set_parent(&mut self, parent_uuid: Option<String>) {
        self.parent = parent_uuid;
    }

    /// Get parent composition UUID
    pub fn get_parent(&self) -> Option<&String> {
        self.parent.as_ref()
    }

    /// Get children composition UUIDs
    pub fn get_children(&self) -> &[String] {
        &self.children
    }

    /// Check if this comp has a specific child
    pub fn has_child(&self, child_uuid: &str) -> bool {
        self.children.iter().any(|uuid| uuid == child_uuid)
    }

    /// Find all children (instance UUIDs) that reference a specific source UUID
    pub fn find_children_by_source(&self, source_uuid: &str) -> Vec<String> {
        let mut result = Vec::new();
        for child_uuid in &self.children {
            if let Some(attrs) = self.children_attrs.get(child_uuid) {
                if let Some(uuid) = attrs.get_str("uuid") {
                    if uuid == source_uuid {
                        result.push(child_uuid.clone());
                    }
                }
            }
        }
        result
    }

    /// Invalidate cache (alias for clear_cache with hash reset)
    fn invalidate_cache(&self) {
        self.cache.borrow_mut().clear();
        // comp_hash will be recalculated on next get_frame()
    }
}

// ===== GUI Trait Implementations =====

impl crate::entities::ProjectUI for Comp {
    fn project_ui(&self, ui: &mut egui::Ui) -> egui::Response {
        ui.horizontal(|ui| {
            // Icon/type indicator
            ui.label("📁");

            // Comp name
            ui.label(self.name());

            // Metadata
            ui.label(format!("{}fps", self.fps()));
            ui.label(format!("{}-{}", self.start(), self.end()));
            ui.label(format!("{} children", self.children.len()));
        })
        .response
    }
}

impl crate::entities::TimelineUI for Comp {
    fn timeline_ui(
        &self,
        ui: &mut egui::Ui,
        bar_rect: egui::Rect,
        current_frame: i32,
    ) -> egui::Response {
        let painter = ui.painter();

        // Draw bar background (different color for comp)
        let bar_color = egui::Color32::from_rgb(100, 60, 140);
        painter.rect_filled(bar_rect, 2.0, bar_color);

        // Draw border
        painter.rect_stroke(
            bar_rect,
            2.0,
            egui::Stroke::new(1.0, egui::Color32::WHITE),
            egui::epaint::StrokeKind::Middle,
        );

        // Highlight current frame if within range
        let start = self.start();
        let end = self.end();
        if current_frame >= start && current_frame <= end {
            let total_frames = (end - start + 1) as f32;
            let frame_width = bar_rect.width() / total_frames;
            let offset = (current_frame - start) as f32 * frame_width;
            let playhead_rect = egui::Rect::from_min_size(
                egui::pos2(bar_rect.min.x + offset, bar_rect.min.y),
                egui::vec2(2.0, bar_rect.height()),
            );
            painter.rect_filled(playhead_rect, 0.0, egui::Color32::RED);
        }

        // Draw label
        painter.text(
            bar_rect.left_center() + egui::vec2(5.0, 0.0),
            egui::Align2::LEFT_CENTER,
            self.name(),
            egui::FontId::default(),
            egui::Color32::WHITE,
        );

        ui.interact(
            bar_rect,
            ui.id().with(&self.uuid),
            egui::Sense::click_and_drag(),
        )
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
            if let Some(mask) = &comp.file_mask {
                unique.entry(mask.clone()).or_insert(comp);
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

    comp.file_start = Some(0);
    comp.file_end = Some(last_frame);
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
        let mut project = Project::new();

        // Leaf: file-mode comp that yields placeholder frames
        let leaf = file_comp("Leaf", 0, 9, 24.0);
        let leaf_uuid = leaf.uuid.clone();
        project.comps_order.push(leaf_uuid.clone());
        project.media.insert(leaf_uuid.clone(), leaf);

        // Middle: layer comp that references leaf
        let mut inner = Comp::new("Inner", 0, 9, 24.0);
        inner.add_child(leaf_uuid.clone(), 0, &project).unwrap();
        let inner_uuid = inner.uuid.clone();
        project.comps_order.push(inner_uuid.clone());
        project.media.insert(inner_uuid.clone(), inner);

        // Root: layer comp that references inner
        let mut root = Comp::new("Root", 0, 9, 24.0);
        root.add_child(inner_uuid.clone(), 0, &project).unwrap();
        let root_uuid = root.uuid.clone();
        project.media.insert(root_uuid.clone(), root);

        let root_ref = project.media.get(&root_uuid).unwrap();
        let frame = root_ref.get_frame(5, &project);
        assert!(
            frame.is_some(),
            "Recursive composition should resolve a frame"
        );
    }

    #[test]
    fn test_cache_invalidation_on_attr_change() {
        let mut project = Project::new();

        // Source clip placeholder
        let clip = file_comp("Clip", 0, 4, 24.0);
        let clip_uuid = clip.uuid.clone();
        project.media.insert(clip_uuid.clone(), clip);

        // Comp with single child
        let mut comp = Comp::new("Test Comp", 0, 4, 24.0);
        comp.add_child(clip_uuid.clone(), 0, &project).unwrap();
        let comp_uuid = comp.uuid.clone();
        project.media.insert(comp_uuid.clone(), comp);

        // First render populates cache
        {
            let comp_ref = project.media.get(&comp_uuid).unwrap();
            let _frame = comp_ref.get_frame(2, &project).unwrap();
            assert_eq!(comp_ref.cache.borrow().len(), 1);
        }

        // Change child opacity to alter comp hash without clearing cache
        {
            let comp_mut = project.media.get_mut(&comp_uuid).unwrap();
            let child_uuid = comp_mut.children.first().cloned().unwrap();
            if let Some(attrs) = comp_mut.children_attrs.get_mut(&child_uuid) {
                attrs.set("opacity", AttrValue::Float(0.5));
            }
        }

        // Second render should insert a new cache entry (hash changed)
        let comp_ref = project.media.get(&comp_uuid).unwrap();
        let _frame = comp_ref.get_frame(2, &project).unwrap();
        assert_eq!(
            comp_ref.cache.borrow().len(),
            2,
            "Cache should contain entries for old and new hashes"
        );
    }

    /// Test hash computation consistency
    #[test]
    fn test_comp_hash_consistency() {
        let mut comp1 = Comp::new("Comp1", 0, 10, 24.0);
        let mut comp2 = Comp::new("Comp2", 0, 10, 24.0);

        // Same children should produce same hash
        let uuid1 = "uuid1".to_string();

        let mut attrs1 = Attrs::new();
        attrs1.set("start", AttrValue::UInt(0));
        attrs1.set("end", AttrValue::UInt(10));
        attrs1.set("play_start", AttrValue::Int(0));
        attrs1.set("play_end", AttrValue::Int(0));
        attrs1.set("opacity", AttrValue::Float(1.0));

        let mut attrs2 = Attrs::new();
        attrs2.set("start", AttrValue::UInt(0));
        attrs2.set("end", AttrValue::UInt(10));
        attrs2.set("play_start", AttrValue::Int(0));
        attrs2.set("play_end", AttrValue::Int(0));
        attrs2.set("opacity", AttrValue::Float(1.0));

        comp1.children.push(uuid1.clone());
        comp1.children_attrs.insert(uuid1.clone(), attrs1);

        comp2.children.push(uuid1.clone());
        comp2.children_attrs.insert(uuid1, attrs2);

        let hash1 = comp1.compute_comp_hash();
        let hash2 = comp2.compute_comp_hash();
        assert_eq!(hash1, hash2, "Identical layers should produce same hash");

        // Different opacity should produce different hash
        if let Some(attrs) = comp2.children_attrs.get_mut(&"uuid1".to_string()) {
            attrs.set("opacity", AttrValue::Float(0.7));
        }
        let hash3 = comp2.compute_comp_hash();
        assert_ne!(
            hash1, hash3,
            "Different opacity should produce different hash"
        );
    }

    #[test]
    fn test_multi_layer_blending_placeholder_sources() {
        let mut project = Project::new();

        // Three placeholder sources
        let mut sources: Vec<String> = Vec::new();
        for i in 0..3 {
            let comp = file_comp(&format!("Src{}", i), 0, 4, 24.0);
            let uuid = comp.uuid.clone();
            project.media.insert(uuid.clone(), comp);
            sources.push(uuid);
        }

        // Parent comp blending three children with different opacities
        let mut comp = Comp::new("Blend", 0, 4, 24.0);
        for (idx, uuid) in sources.iter().enumerate() {
            comp.add_child(uuid.clone(), 0, &project).unwrap();
            // Set opacity based on order
            let child_uuid = comp.children.last().unwrap().clone();
            let opacity = match idx {
                0 => 1.0,
                1 => 0.5,
                _ => 0.3,
            };
            if let Some(attrs) = comp.children_attrs.get_mut(&child_uuid) {
                attrs.set("opacity", AttrValue::Float(opacity));
            }
        }

        let comp_uuid = comp.uuid.clone();
        project.media.insert(comp_uuid.clone(), comp);

        let comp_ref = project.media.get(&comp_uuid).unwrap();
        let frame = comp_ref.get_frame(2, &project);
        assert!(
            frame.is_some(),
            "Multi-layer composition with placeholder sources should succeed"
        );
        assert_eq!(comp_ref.cache.borrow().len(), 1);
    }
}
