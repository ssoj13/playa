//! Composition-level types (timeline unit for playback/encoding).
//!
//! `Comp` is now a unified entity that can work in two modes:
//! - Layer mode: composes children comps
//! - File mode: loads image sequence from disk (ex-Clip functionality)

use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};
use eframe::egui;

use super::{Attrs, AttrValue};
use crate::events::{CompEvent, CompEventSender};
use super::frame::Frame;

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
    pub file_start: Option<usize>,

    /// Last frame number in sequence
    /// Only used in File mode
    #[serde(default)]
    pub file_end: Option<usize>,

    // ===== Common Fields =====
    /// Currently selected layer/child index (if any)
    #[serde(default)]
    pub selected_layer: Option<usize>,

    /// Current playback position within this comp (persisted)
    #[serde(default)]
    pub current_frame: usize,

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
    pub fn new(name: impl Into<String>, start: usize, end: usize, fps: f32) -> Self {
        let mut attrs = Attrs::new();
        attrs.set("name", AttrValue::Str(name.into()));
        attrs.set("start", AttrValue::UInt(start as u32));
        attrs.set("end", AttrValue::UInt(end as u32));
        attrs.set("fps", AttrValue::Float(fps));
        attrs.set("play_start", AttrValue::Int(0)); // Full range by default
        attrs.set("play_end", AttrValue::Int(0));   // Full range by default

        // Transform defaults
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
    pub fn new_file_comp(
        pattern: impl Into<String>,
        start: usize,
        end: usize,
        fps: f32,
    ) -> Self {
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

    pub fn start(&self) -> usize {
        self.attrs.get_u32("start").unwrap_or(0) as usize
    }

    pub fn end(&self) -> usize {
        self.attrs.get_u32("end").unwrap_or(100) as usize
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

    pub fn set_start(&mut self, start: usize) {
        self.attrs.set("start", AttrValue::UInt(start as u32));
    }

    pub fn set_end(&mut self, end: usize) {
        self.attrs.set("end", AttrValue::UInt(end as u32));
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
    pub fn play_range(&self) -> (usize, usize) {
        let visible_start = self.start() + self.play_start().max(0) as usize;
        let visible_end = self.end().saturating_sub(self.play_end().max(0) as usize);
        (visible_start, visible_end)
    }

    /// Number of frames in full composition (not limited by play_area)
    pub fn frame_count(&self) -> usize {
        let start = self.start();
        let end = self.end();
        if end >= start {
            end - start + 1
        } else {
            0
        }
    }

    /// Number of frames in play range (work area)
    pub fn play_frame_count(&self) -> usize {
        let (visible_start, visible_end) = self.play_range();
        if visible_end >= visible_start {
            visible_end - visible_start + 1
        } else {
            0
        }
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
                        let opacity_bits = attrs.get_float("opacity").unwrap_or(1.0).to_bits();
                        opacity_bits.hash(&mut hasher);
                    }
                }
            }
        }

        // Hash transform attributes
        let transparency_bits = self.attrs.get_float("transparency").unwrap_or(1.0).to_bits();
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
    pub fn set_current_frame(&mut self, new_frame: usize) {
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
    /// Recursively resolves layer sources from Project.media and composes them.
    /// Uses hash-based cache that invalidates when layers configuration changes.
    /// Only frames within play_area (work area) are composed - frames outside return None.
    pub fn get_frame(&self, frame_idx: usize, project: &super::Project) -> Option<Frame> {
        // Check if frame is within play area (work area)
        let (play_start, play_end) = self.play_range();
        if frame_idx < play_start || frame_idx > play_end {
            return None; // Frame outside work area - don't compose
        }

        // Compute composition hash for cache key
        let comp_hash = self.compute_comp_hash();
        let cache_key = (comp_hash, frame_idx);

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

    /// Compose frame at given global frame index.
    ///
    /// Recursively resolves all active children:
    /// - Converts global comp frame to local source frame
    /// - Resolves Comp from Project.media by UUID
    /// - Recursively gets frames (supports nested Comps)
    /// - Blends multiple children with CPU compositor (GPU compositor planned)
    fn compose(&self, frame_idx: usize, project: &super::Project) -> Option<Frame> {
        let mut source_frames: Vec<(Frame, f32)> = Vec::new();

        // Collect frames from all active children
        for child_uuid in &self.children {
            // Get child attributes
            let attrs = self.children_attrs.get(child_uuid)?;

            // Get child range from attrs
            let child_start = attrs.get_u32("start").unwrap_or(0) as usize;
            let child_end = attrs.get_u32("end").unwrap_or(0) as usize;

            // Check if child is active at this frame
            if frame_idx < child_start || frame_idx > child_end {
                continue; // Child not active
            }

            // Convert comp frame to local source frame
            let play_start = attrs.get_i32("play_start").unwrap_or(0);
            let local_frame = (frame_idx - child_start) as i32 + play_start;
            if local_frame < 0 {
                continue;
            }

            // Resolve source from Project.media
            if let Some(source) = project.media.get(child_uuid) {
                // Recursively get frame from source (Clip or Comp)
                if let Some(frame) = source.get_frame(local_frame as usize, project) {
                    let opacity = attrs.get_float("opacity").unwrap_or(1.0);
                    source_frames.push((frame, opacity));
                }
            }
        }

        // Blend all children with project compositor (CPU or GPU)
        project.compositor.blend(source_frames)
    }

    /// Add a new child to the composition at specified start frame.
    ///
    /// Automatically determines duration from source and creates child attributes.
    /// Add child by looking up source from project
    pub fn add_child(
        &mut self,
        source_uuid: String,
        start_frame: usize,
        project: &super::Project,
    ) -> anyhow::Result<()> {
        // Get source to determine duration
        let source = project
            .media
            .get(&source_uuid)
            .ok_or_else(|| anyhow::anyhow!("Source {} not found", source_uuid))?;

        let duration = source.frame_count();
        self.add_child_with_duration(source_uuid, start_frame, duration)
    }

    /// Add child with explicit duration (avoids borrow checker issues)
    pub fn add_child_with_duration(
        &mut self,
        source_uuid: String,
        start_frame: usize,
        duration: usize,
    ) -> anyhow::Result<()> {
        let end_frame = start_frame + duration - 1;

        // Create child attributes
        let mut attrs = Attrs::new();
        attrs.set("name", AttrValue::Str("Child".to_string()));
        attrs.set("start", AttrValue::UInt(start_frame as u32));
        attrs.set("end", AttrValue::UInt(end_frame as u32));
        attrs.set("play_start", AttrValue::Int(0));
        attrs.set("play_end", AttrValue::Int(0));
        attrs.set("opacity", AttrValue::Float(1.0));

        // Add to children (top)
        self.children.push(source_uuid.clone());
        self.children_attrs.insert(source_uuid, attrs);

        // Clear cache and emit event
        self.clear_cache();
        self.event_sender.emit(CompEvent::LayersChanged {
            comp_uuid: self.uuid.clone(),
        });

        Ok(())
    }

    /// Move a child to a new start position, preserving duration.
    pub fn move_child(&mut self, child_idx: usize, new_start: usize) -> anyhow::Result<()> {
        let child_uuid = self
            .children
            .get(child_idx)
            .ok_or_else(|| anyhow::anyhow!("Child {} not found", child_idx))?
            .clone();

        let attrs = self
            .children_attrs
            .get_mut(&child_uuid)
            .ok_or_else(|| anyhow::anyhow!("Child attrs not found"))?;

        let old_start = attrs.get_u32("start").unwrap_or(0) as usize;
        let old_end = attrs.get_u32("end").unwrap_or(0) as usize;
        let duration = if old_end >= old_start {
            old_end - old_start
        } else {
            0
        };

        attrs.set("start", AttrValue::UInt(new_start as u32));
        attrs.set("end", AttrValue::UInt((new_start + duration) as u32));

        // Clear cache and emit event
        self.clear_cache();
        self.event_sender.emit(CompEvent::LayersChanged {
            comp_uuid: self.uuid.clone(),
        });

        Ok(())
    }

    /// Set child play start (adjust play_start attribute - visible start offset from child start).
    pub fn set_child_play_start(&mut self, child_idx: usize, new_play_start: i32) -> anyhow::Result<()> {
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
    pub fn set_child_play_end(&mut self, child_idx: usize, new_play_end: i32) -> anyhow::Result<()> {
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
    pub fn get_child_edges_near(&self, from_frame: usize) -> Vec<(usize, bool)> {
        let mut edges = Vec::new();

        for child_uuid in &self.children {
            if let Some(attrs) = self.children_attrs.get(child_uuid) {
                let start = attrs.get_u32("start").unwrap_or(0) as usize;
                let end = attrs.get_u32("end").unwrap_or(0) as usize;
                let play_start = attrs.get_i32("play_start").unwrap_or(0);
                let play_end = attrs.get_i32("play_end").unwrap_or(0);

                // Visible range accounting for play range
                let visible_start = start + play_start as usize;
                let visible_end = end.saturating_sub(play_end as usize);

                if visible_start < visible_end {
                    edges.push((visible_start, true));   // Start edge
                    edges.push((visible_end, false));    // End edge
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
        self.invalidate_cache();
        self.event_sender.emit(CompEvent::LayersChanged {
            comp_uuid: self.uuid.clone(),
        });
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
        current_frame: usize,
    ) -> egui::Response {
        let painter = ui.painter();

        // Draw bar background (different color for comp)
        let bar_color = egui::Color32::from_rgb(100, 60, 140);
        painter.rect_filled(bar_rect, 2.0, bar_color);

        // Draw border
        painter.rect_stroke(bar_rect, 2.0, egui::Stroke::new(1.0, egui::Color32::WHITE), egui::epaint::StrokeKind::Middle);

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

        ui.interact(bar_rect, ui.id().with(&self.uuid), egui::Sense::click_and_drag())
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use super::super::Clip;
    use super::super::Project;

    /// Helper: create dummy frame
    fn dummy_frame(_value: u8) -> Frame {
        // Create 2x2 F32 frame with test pattern
        Frame::new(2, 2, crate::frame::PixelDepth::F32)
    }

    /// Test recursive composition: Comp A contains Comp B
    #[test]
    fn test_recursive_composition() {
        let mut project = Project::new();

        // Create Clip with 10 frames
        let frames: Vec<Frame> = (0..10).map(|i| dummy_frame(i * 10)).collect();
        let clip = Clip::from_frames(frames, "test_clip".to_string(), 2, 2);
        let clip_uuid = clip.uuid.clone();
        project.media.insert(clip_uuid.clone(), todo!("Convert Clip to Comp with File mode"));
        project.clips_order.push(clip_uuid.clone());

        // Create Comp B with clip as child
        let mut comp_b = Comp::new("Comp B", 0, 9, 24.0);
        comp_b.add_child(clip_uuid.clone(), 0, &project).unwrap();
        let comp_b_uuid = comp_b.uuid.clone();
        project.media.insert(comp_b_uuid.clone(), comp_b);
        project.comps_order.push(comp_b_uuid.clone());

        // Create Comp A with Comp B as child
        let mut comp_a = Comp::new("Comp A", 0, 9, 24.0);
        comp_a.add_child(comp_b_uuid.clone(), 0, &project).unwrap();
        let comp_a_uuid = comp_a.uuid.clone();
        project.media.insert(comp_a_uuid.clone(), comp_a);
        project.comps_order.push(comp_a_uuid.clone());

        // Test: Get frame from Comp A (should recursively resolve through Comp B to Clip)
        let comp_a = project.media.get(&comp_a_uuid).unwrap();
        let frame = comp_a.get_frame(5, &project);
        assert!(frame.is_some(), "Frame should be resolved recursively");

        // Verify frame was composed (detailed data verification would require frame comparison helper)
        let _frame = frame.unwrap();
        // Success if we got a frame - recursive composition worked
    }

    /// Test hash-based cache invalidation
    #[test]
    fn test_cache_invalidation() {
        let mut project = Project::new();

        // Create Clip with 5 frames
        let frames: Vec<Frame> = (0..5).map(|i| dummy_frame(i * 20)).collect();
        let clip = Clip::from_frames(frames, "test_clip".to_string(), 2, 2);
        let clip_uuid = clip.uuid.clone();
        project.media.insert(clip_uuid.clone(), todo!("Convert Clip to Comp with File mode"));

        // Create Comp with clip as child
        let mut comp = Comp::new("Test Comp", 0, 4, 24.0);
        comp.add_child(clip_uuid.clone(), 0, &project).unwrap();
        let comp_uuid = comp.uuid.clone();
        project.media.insert(comp_uuid.clone(), comp);

        // Get frame 2 - should cache it
        let comp = project.media.get(&comp_uuid).unwrap();
        let _frame1 = comp.get_frame(2, &project).unwrap();
        let cache_size_before = comp.cache.borrow().len();
        assert_eq!(cache_size_before, 1, "Cache should have 1 entry");

        // Get same frame - should hit cache
        let _frame2 = comp.get_frame(2, &project).unwrap();
        // Frames should be clones (cache hit)
        assert_eq!(comp.cache.borrow().len(), 1, "Cache size should stay the same");

        // Modify layer (change opacity) - should invalidate cache
        {
            let comp_mut = project.media.get_mut(&comp_uuid).unwrap();
            // TODO: Update test to work with children API instead of layers
            // comp_mut.children[0].attrs...
            comp_mut.clear_cache();
        } // Release mutable borrow

        // Get frame again - cache should add new entry with different hash
        let comp = project.media.get(&comp_uuid).unwrap();
        let _frame3 = comp.get_frame(2, &project).unwrap();
        assert_eq!(comp.cache.borrow().len(), 2, "Cache should have both old and new entries (different hashes)");
        // Success - cache uses hash-based keys, old entry with old hash remains, new entry with new hash added
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
        comp2.layers[0].attrs.set("opacity", crate::attrs::AttrValue::Float(0.7));
        let hash3 = comp2.compute_comp_hash();
        assert_ne!(hash1, hash3, "Different opacity should produce different hash");
    }

    /// Test multi-layer blending with compositor
    #[test]
    fn test_multi_layer_blending() {
        let mut project = Project::new();

        // Create 3 clips with different frames
        for i in 0..3 {
            let frames: Vec<Frame> = (0..5).map(|_| dummy_frame(i * 30)).collect();
            let clip = Clip::from_frames(frames, format!("clip_{}", i), 2, 2);
            let clip_uuid = clip.uuid.clone();
            project.media.insert(clip_uuid.clone(), todo!("Convert Clip to Comp with File mode"));
            project.clips_order.push(clip_uuid.clone());
        }

        // Create Comp with all 3 clips as layers
        let mut comp = Comp::new("Multi-layer Comp", 0, 4, 24.0);

        // Child 0: clip 0, full opacity
        let uuid0 = project.clips_order[0].clone();
        comp.children.push(uuid0.clone());
        let mut attrs0 = Attrs::new();
        attrs0.set("start", AttrValue::UInt(0));
        attrs0.set("end", AttrValue::UInt(4));
        attrs0.set("play_start", AttrValue::Int(0));
        attrs0.set("play_end", AttrValue::Int(0));
        attrs0.set("opacity", AttrValue::Float(1.0));
        comp.children_attrs.insert(uuid0, attrs0);

        // Child 1: clip 1, 50% opacity
        let uuid1 = project.clips_order[1].clone();
        comp.children.push(uuid1.clone());
        let mut attrs1 = Attrs::new();
        attrs1.set("start", AttrValue::UInt(0));
        attrs1.set("end", AttrValue::UInt(4));
        attrs1.set("play_start", AttrValue::Int(0));
        attrs1.set("play_end", AttrValue::Int(0));
        attrs1.set("opacity", AttrValue::Float(0.5));
        comp.children_attrs.insert(uuid1, attrs1);

        // Child 2: clip 2, 30% opacity
        let uuid2 = project.clips_order[2].clone();
        comp.children.push(uuid2.clone());
        let mut attrs2 = Attrs::new();
        attrs2.set("start", AttrValue::UInt(0));
        attrs2.set("end", AttrValue::UInt(4));
        attrs2.set("play_start", AttrValue::Int(0));
        attrs2.set("play_end", AttrValue::Int(0));
        attrs2.set("opacity", AttrValue::Float(0.3));
        comp.children_attrs.insert(uuid2, attrs2);

        let comp_uuid = comp.uuid.clone();
        project.media.insert(comp_uuid.clone(), comp);

        // Get frame - should blend all 3 layers
        let comp = project.media.get(&comp_uuid).unwrap();
        let frame = comp.get_frame(2, &project);

        assert!(frame.is_some(), "Multi-layer composition should succeed");

        // Verify cache contains the blended result
        assert_eq!(comp.cache.borrow().len(), 1, "Cache should contain blended frame");
    }
}
