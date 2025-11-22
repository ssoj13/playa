//! Composition-level types (timeline unit for playback/encoding).
//!
//! `Comp` is now a unified entity that can work in two modes:
//! - Layer mode: composes children comps
//! - File mode: loads image sequence from disk (ex-Clip functionality)
//! Used by: timeline rendering (`widgets::timeline`), encoding (`dialogs::encode`),
//! playback (`player.rs`), and project serialization. Data flow: UI emits events →
//! `Comp` mutates attrs/children → cached frames/computed hashes drive compositor
//! work and encoding output.

use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use eframe::egui;
use serde::{Deserialize, Serialize};

use super::frame::{CropAlign, Frame, FrameStatus, PixelDepth};
use super::{AttrValue, Attrs};
use crate::events::{CompEvent, CompEventSender};

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
/// - "play_start" (Int): Work area start offset
/// - "play_end" (Int): Work area end offset
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
    /// Currently selected layer/child index (if any)
    #[serde(default)]
    pub selected_layer: Option<usize>,

    /// Current playback position within this comp (persisted)
    #[serde(default)]
    pub current_frame: i32,

    /// Event sender for emitting comp events (runtime-only, rebuilt after deserialization)
    #[serde(skip)]
    #[serde(default)]
    event_sender: CompEventSender,

    /// Per-comp frame cache: (comp_hash, frame_idx) -> composed Frame (runtime-only)
    /// Uses RefCell for interior mutability to allow caching with &self
    /// Hash invalidates cache when composition changes
    #[serde(skip)]
    #[serde(default)]
    cache: RefCell<HashMap<(u64, usize), Frame>>,
}

impl Comp {
    /// Create new composition in Layer mode (default)
    pub fn new(name: impl Into<String>, start: i32, end: i32, fps: f32) -> Self {
        let mut attrs = Attrs::new();
        attrs.set("name", AttrValue::Str(name.into()));
        attrs.set("start", AttrValue::Int(start));
        attrs.set("end", AttrValue::Int(end));
        attrs.set("fps", AttrValue::Float(fps));
        attrs.set("play_start", AttrValue::Int(0)); // Full range by default
        attrs.set("play_end", AttrValue::Int(0)); // Full range by default

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
            selected_layer: None,
            event_sender: CompEventSender::dummy(),
            cache: RefCell::new(HashMap::new()),
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
        self.attrs.get_i32("play_start").unwrap_or(0)
    }

    pub fn play_end(&self) -> i32 {
        self.attrs.get_i32("play_end").unwrap_or(0)
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

    pub fn set_fps(&mut self, fps: f32) {
        self.attrs.set("fps", AttrValue::Float(fps));
    }

    pub fn set_play_start(&mut self, play_start: i32) {
        self.attrs.set("play_start", AttrValue::Int(play_start));
    }

    pub fn set_play_end(&mut self, play_end: i32) {
        self.attrs.set("play_end", AttrValue::Int(play_end));
    }

    /// Inclusive play range (work area) used for rendering/encoding
    /// Returns the visible portion considering play_start/play_end offsets
    pub fn play_range(&self) -> (i32, i32) {
        let visible_start = self.start() + self.play_start().max(0);
        let visible_end = self.end() - self.play_end().max(0);
        (visible_start, visible_end)
    }

    /// Number of frames in full composition (not limited by play_area)
    pub fn frame_count(&self) -> i32 {
        let start = self.start();
        let end = self.end();
        if end >= start { end - start + 1 } else { 0 }
    }

    /// Number of frames in play range (work area)
    pub fn play_frame_count(&self) -> i32 {
        let (visible_start, visible_end) = self.play_range();
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
            return None;
        }

        let duration = self.frame_count();
        if duration <= 0 {
            return None;
        }

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

    /// Set selected layer index.
    pub fn set_selected_layer(&mut self, layer: Option<usize>) {
        self.selected_layer = layer;
    }

    /// Clear per-comp frame cache.
    pub fn clear_cache(&self) {
        self.cache.borrow_mut().clear();
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
                // Hash children UUIDs (order matters)
                self.children.len().hash(&mut hasher);
                for child_uuid in &self.children {
                    child_uuid.hash(&mut hasher);

                    // Hash child attributes if present
                    if let Some(attrs) = self.children_attrs.get(child_uuid) {
                        attrs.get_u32("start").unwrap_or(0).hash(&mut hasher);
                        attrs.get_u32("end").unwrap_or(0).hash(&mut hasher);
                        attrs.get_i32("play_start").unwrap_or(0).hash(&mut hasher);
                        attrs.get_i32("play_end").unwrap_or(0).hash(&mut hasher);
                        attrs.get_bool("visible").unwrap_or(true).hash(&mut hasher);
                        let opacity_bits = attrs.get_float("opacity").unwrap_or(1.0).to_bits();
                        opacity_bits.hash(&mut hasher);
                        if let Some(blend) = attrs.get_str("blend_mode") {
                            blend.hash(&mut hasher);
                        }
                        let speed_bits = attrs.get_float("speed").unwrap_or(1.0).to_bits();
                        speed_bits.hash(&mut hasher);
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

        // Work area in local (0-based) clip space
        let work_start = self.play_start().max(0);
        let work_end = (duration - 1 - self.play_end().max(0)).max(work_start);

        // Outside work area -> placeholder, no load
        if frame_idx < work_start || frame_idx > work_end {
            return Some(self.placeholder_frame());
        }

        // Map local frame_idx to absolute sequence number (preserve original numbering)
        let seq_start = self.file_start.unwrap_or(self.start());
        let seq_end = self.file_end.unwrap_or(self.end());
        let seq_frame = seq_start.saturating_add(frame_idx);
        if seq_frame < seq_start || seq_frame > seq_end {
            return Some(self.placeholder_frame());
        }

        // Cache key uses sequence frame number to avoid collisions when start shifts
        let cache_key = (self.compute_comp_hash(), seq_frame.max(0) as usize);
        if let Some(frame) = self.cache.borrow().get(&cache_key) {
            return Some(frame.clone());
        }

        let frame_path = self.resolve_frame_path(seq_frame).unwrap_or_default();
        if frame_path.as_os_str().is_empty() {
            return Some(self.placeholder_frame());
        }

        let frame = self.frame_from_path(frame_path);
        self.cache.borrow_mut().insert(cache_key, frame.clone());
        Some(frame)
    }

    fn get_layer_frame(&self, frame_idx: i32, project: &super::Project) -> Option<Frame> {
        // Check if frame is within play area (work area)
        let (play_start, play_end) = self.play_range();
        if frame_idx < play_start || frame_idx > play_end {
            return None; // Frame outside work area - don't compose
        }

        // Compute composition hash for cache key
        let comp_hash = self.compute_comp_hash();
        let cache_key = (comp_hash, frame_idx.max(0) as usize); // Cache key uses positive values

        // Check cache
        if let Some(frame) = self.cache.borrow().get(&cache_key) {
            return Some(frame.clone());
        }

        // Compose frame recursively
        let composed = self.compose(frame_idx, project)?;

        // Cache result with hash-based key
        self.cache.borrow_mut().insert(cache_key, composed.clone());
        Some(composed)
    }

    fn resolve_frame_path(&self, frame_number: i32) -> Option<PathBuf> {
        let mask = self.file_mask.as_ref()?;
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

    fn frame_dimensions(&self) -> (usize, usize) {
        let w = self.attrs.get_u32("width").unwrap_or(64) as usize;
        let h = self.attrs.get_u32("height").unwrap_or(64) as usize;
        (w.max(1), h.max(1))
    }

    fn placeholder_frame(&self) -> Frame {
        let (w, h) = self.frame_dimensions();
        Frame::new(w, h, PixelDepth::U8)
    }

    fn frame_from_path(&self, path: PathBuf) -> Frame {
        let (w, h) = self.frame_dimensions();
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
        let mut source_frames: Vec<(Frame, f32)> = Vec::new();

        debug!("compose() called: frame_idx={}, children.len()={}", frame_idx, self.children.len());

        // Collect frames from all active children
        // IMPORTANT: Reverse iteration - last child (bottom layer) becomes base,
        // first child (top layer) composited last
        for child_uuid in self.children.iter().rev() {
            // Get child attributes
            let attrs = self.children_attrs.get(child_uuid)?;

            // Get child range from attrs (supports negative values)
            let child_start = attrs.get_i32("start").unwrap_or(0);
            let child_end = attrs.get_i32("end").unwrap_or(0);

            // Check if child is active at this frame
            let frame_idx_i32 = frame_idx as i32;
            if frame_idx_i32 < child_start || frame_idx_i32 > child_end {
                debug!("  child {} SKIPPED: frame {} not in range [{}, {}]",
                    child_uuid, frame_idx_i32, child_start, child_end);
                continue; // Child not active
            }
            debug!("  child {} ACTIVE at frame {}, range=[{}, {}]",
                child_uuid, frame_idx_i32, child_start, child_end);

            // Convert comp frame to local source frame
            let play_start = attrs.get_i32("play_start").unwrap_or(0);
            let local_frame = (frame_idx_i32 - child_start) + play_start;
            if local_frame < 0 {
                continue;
            }

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
                // Recursively get frame from source (Clip or Comp)
                if let Some(frame) = source.get_frame(local_frame, project) {
                    let opacity = attrs.get_float("opacity").unwrap_or(1.0);
                    source_frames.push((frame, opacity));
                }
            }
        }

        // Blend all children with project compositor (CPU or GPU)
        debug!("compose() collected {} frames, calling compositor.blend()", source_frames.len());
        project.compositor.blend(source_frames)
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

        let duration = source.frame_count();
        self.add_child_with_duration(source_uuid, start_frame, duration, None)
    }

    /// Add child with explicit duration and optional target row
    pub fn add_child_with_duration(
        &mut self,
        source_uuid: String,
        start_frame: i32,
        duration: i32,
        target_row: Option<usize>,
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
        attrs.set("play_start", AttrValue::Int(0));
        attrs.set("play_end", AttrValue::Int(0));
        attrs.set("opacity", AttrValue::Float(1.0));
        attrs.set("visible", AttrValue::Bool(true));
        attrs.set("blend_mode", AttrValue::Str("normal".to_string()));
        attrs.set("speed", AttrValue::Float(1.0));

        // Add to children at appropriate position for target row
        if let Some(target_row) = target_row {
            let insert_pos = self.find_insert_position_for_row(target_row);
            self.children.insert(insert_pos, instance_uuid.clone());
        } else {
            self.children.push(instance_uuid.clone());
        }
        self.children_attrs.insert(instance_uuid, attrs);

        self.rebound();
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

        attrs.set("start", AttrValue::Int(new_start));
        attrs.set("end", AttrValue::Int(new_end));

        self.rebound();

        // Clear cache and emit event
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

        attrs.set("play_start", AttrValue::Int(new_play_start));

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

        attrs.set("play_end", AttrValue::Int(new_play_end));

        // Clear cache and emit event
        self.clear_cache();
        self.event_sender.emit(CompEvent::LayersChanged {
            comp_uuid: self.uuid.clone(),
        });

        Ok(())
    }

    /// Set comp play start (work area start offset from comp start).
    /// This limits the active work area for playback and rendering.
    pub fn set_comp_play_start(&mut self, new_play_start: i32) {
        self.set_play_start(new_play_start);
        self.clear_cache();
        self.event_sender.emit(CompEvent::LayersChanged {
            comp_uuid: self.uuid.clone(),
        });
    }

    /// Set comp play end (work area end offset from comp end).
    /// This limits the active work area for playback and rendering.
    pub fn set_comp_play_end(&mut self, new_play_end: i32) {
        self.set_play_end(new_play_end);
        self.clear_cache();
        self.event_sender.emit(CompEvent::LayersChanged {
            comp_uuid: self.uuid.clone(),
        });
    }

    /// Get all child edges (start and end frames) sorted by distance from given frame
    /// Returns vec of (frame_number, is_start) tuples
    pub fn get_child_edges_near(&self, from_frame: i32) -> Vec<(i32, bool)> {
        let mut edges = Vec::new();

        for child_uuid in &self.children {
            if let Some(attrs) = self.children_attrs.get(child_uuid) {
                let start = attrs.get_i32("start").unwrap_or(0);
                let end = attrs.get_i32("end").unwrap_or(0);
                let play_start = attrs.get_i32("play_start").unwrap_or(0);
                let play_end = attrs.get_i32("play_end").unwrap_or(0);

                // Visible range accounting for play range
                let visible_start = start + play_start;
                let visible_end = end - play_end;

                if visible_start < visible_end {
                    edges.push((visible_start, true)); // Start edge
                    edges.push((visible_end, false)); // End edge
                }
            }
        }

        // Sort by distance from from_frame
        edges.sort_by_key(|(frame, _)| {
            let dist = if *frame > from_frame {
                *frame - from_frame
            } else {
                from_frame - *frame
            };
            dist
        });

        // Remove duplicates while preserving order
        edges.dedup_by_key(|(frame, _)| *frame);

        edges
    }

    // ===== Parent-Child Management =====

    /// Remove child comp from this composition
    pub fn remove_child(&mut self, child_uuid: &str) {
        self.children.retain(|uuid| uuid != child_uuid);
        self.rebound();
        self.invalidate_cache();
        self.event_sender.emit(CompEvent::LayersChanged {
            comp_uuid: self.uuid.clone(),
        });
    }

    /// Recalculate comp start/end based on children (negative starts allowed).
    pub fn rebound(&mut self) {
        if self.children.is_empty() {
            // Default span when no children: 0..100 for a visible timeline
            self.attrs.set("start", AttrValue::Int(0));
            self.attrs.set("end", AttrValue::Int(100));
            return;
        }

        let mut min_start = i32::MAX;
        let mut max_end = i32::MIN;

        for child_uuid in &self.children {
            if let Some(attrs) = self.children_attrs.get(child_uuid) {
                let s = attrs.get_i32("start").unwrap_or(0);
                let e = attrs.get_i32("end").unwrap_or(0);
                min_start = min_start.min(s);
                max_end = max_end.max(e);
            }
        }

        if min_start == i32::MAX || max_end == i32::MIN {
            self.attrs.set("start", AttrValue::Int(0));
            self.attrs.set("end", AttrValue::Int(0));
        } else {
            self.attrs.set("start", AttrValue::Int(min_start));
            self.attrs.set("end", AttrValue::Int(max_end));
        }
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
