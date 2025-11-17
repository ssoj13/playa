//! Composition-level types (timeline unit for playback/encoding).
//!
//! `Comp` references Layers, Clips (via Layers), and owns
//! a simple per-comp cache for composed frames.

use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

use crate::attrs::{Attrs, AttrValue};
use crate::events::{CompEvent, CompEventSender};
use crate::frame::Frame;
use crate::layer::Layer;

/// Lightweight composition descriptor with per-comp cache.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Comp {
    /// Stable identifier inside Project
    pub uuid: String,

    /// Human-readable name (e.g., "Main", "Shot_010")
    pub name: String,

    /// Global start frame (inclusive)
    pub start: usize,

    /// Global end frame (inclusive)
    pub end: usize,

    /// Timeline framerate (frames per second)
    pub fps: f32,

    /// Arbitrary attributes (resolution, fps overrides, tags, etc.)
    pub attrs: Attrs,

    /// Layers that belong to this composition
    pub layers: Vec<Layer>,

    /// Current playback position within this comp (persisted)
    #[serde(default)]
    pub current_frame: usize,

    /// Event sender for emitting comp events (runtime-only, rebuilt after deserialization)
    #[serde(skip)]
    #[serde(default)]
    event_sender: CompEventSender,

    /// Per-comp frame cache: (layers_hash, frame_idx) -> composed Frame (runtime-only)
    /// Uses RefCell for interior mutability to allow caching with &self
    /// Hash invalidates cache when layers change
    #[serde(skip)]
    #[serde(default)]
    cache: RefCell<HashMap<(u64, usize), Frame>>,
}

impl Comp {
    pub fn new(name: impl Into<String>, start: usize, end: usize, fps: f32) -> Self {
        let name_str = name.into();
        let mut attrs = Attrs::new();
        attrs.set("name", AttrValue::Str(name_str.clone()));
        attrs.set("start", AttrValue::UInt(start as u32));
        attrs.set("end", AttrValue::UInt(end as u32));
        attrs.set("fps", AttrValue::Float(fps));

        Self {
            uuid: uuid::Uuid::new_v4().to_string(),
            name: name_str,
            start,
            end,
            fps,
            attrs,
            layers: Vec::new(),
            current_frame: start, // Start at beginning of comp
            event_sender: CompEventSender::dummy(),
            cache: RefCell::new(HashMap::new()),
        }
    }

    /// Inclusive play range used for rendering/encoding
    pub fn play_range(&self) -> (usize, usize) {
        (self.start, self.end)
    }

    /// Number of frames in play range
    pub fn total_frames(&self) -> usize {
        if self.end >= self.start {
            self.end - self.start + 1
        } else {
            0
        }
    }

    /// Set play range (inclusive) in comp-local frame indices.
    pub fn set_play_range(&mut self, start: usize, end: usize) {
        if end < start {
            self.start = 0;
            self.end = 0;
        } else {
            self.start = start;
            self.end = end;
        }
    }

    /// Reset play range to full length based on current layers.
    pub fn reset_play_range(&mut self) {
        // For now, assume full range is [0, total_frames-1] as stored.
        if self.end < self.start {
            self.start = 0;
            self.end = 0;
        }
    }

    /// Clear per-comp frame cache.
    pub fn clear_cache(&self) {
        self.cache.borrow_mut().clear();
    }

    /// Compute hash of layers configuration for cache invalidation.
    /// Hash changes when layers, their UUIDs, or attrs change.
    fn compute_layers_hash(&self) -> u64 {
        let mut hasher = DefaultHasher::new();

        // Hash number of layers
        self.layers.len().hash(&mut hasher);

        // Hash each layer's source_uuid and attrs
        for layer in &self.layers {
            layer.source_uuid.hash(&mut hasher);

            // Hash layer attrs (start, end, trim_start, trim_end, opacity)
            layer.attrs.get_u32("start").unwrap_or(0).hash(&mut hasher);
            layer.attrs.get_u32("end").unwrap_or(0).hash(&mut hasher);
            layer.attrs.get_i32("trim_start").unwrap_or(0).hash(&mut hasher);
            layer.attrs.get_i32("trim_end").unwrap_or(0).hash(&mut hasher);

            // Hash opacity as bits to avoid float comparison issues
            let opacity_bits = layer.attrs.get_float("opacity").unwrap_or(1.0).to_bits();
            opacity_bits.hash(&mut hasher);
        }

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
    pub fn get_frame(&self, frame_idx: usize, project: &crate::project::Project) -> Option<Frame> {
        // Compute layers hash for cache key
        let layers_hash = self.compute_layers_hash();
        let cache_key = (layers_hash, frame_idx);

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
    /// Recursively resolves all active layers:
    /// - Converts global comp frame to local source frame via LayerRef.global_to_local()
    /// - Resolves MediaSource from Project.media by UUID
    /// - Recursively gets frames (supports nested Comps)
    /// - Blends multiple layers with CPU compositor (GPU compositor planned)
    fn compose(&self, frame_idx: usize, project: &crate::project::Project) -> Option<Frame> {
        let mut source_frames: Vec<(Frame, f32)> = Vec::new();

        // Collect frames from all active layers
        for layer in &self.layers {
            // Get layer range from attrs
            let layer_start = layer.attrs.get_u32("start").unwrap_or(0) as usize;
            let layer_end = layer.attrs.get_u32("end").unwrap_or(0) as usize;

            // Check if layer is active at this frame
            if frame_idx < layer_start || frame_idx > layer_end {
                continue; // Layer not active
            }

            // Convert comp frame to local source frame
            let trim_start = layer.attrs.get_i32("trim_start").unwrap_or(0);
            let local_frame = (frame_idx - layer_start) as i32 + trim_start;
            if local_frame < 0 {
                continue;
            }

            // Resolve source from Project.media
            if let Some(source) = project.media.get(&layer.source_uuid) {
                // Recursively get frame from source (Clip or Comp)
                if let Some(frame) = source.get_frame(local_frame as usize, project) {
                    let opacity = layer.attrs.get_float("opacity").unwrap_or(1.0);
                    source_frames.push((frame, opacity));
                }
            }
        }

        // Blend all layers with project compositor (CPU or GPU)
        project.compositor.blend(source_frames)
    }

    /// Add a new layer to the composition at specified start frame.
    ///
    /// Automatically determines layer duration from source and creates layer with proper attributes.
    pub fn add_layer(
        &mut self,
        source_uuid: String,
        start_frame: usize,
        project: &crate::project::Project,
    ) -> anyhow::Result<()> {
        // Get source to determine duration
        let source = project
            .media
            .get(&source_uuid)
            .ok_or_else(|| anyhow::anyhow!("Source {} not found", source_uuid))?;

        let duration = source.total_frames();
        let end_frame = start_frame + duration - 1;

        // Create new layer with proper signature
        let layer = Layer::new(source_uuid, start_frame, end_frame);

        // Add to layers (top)
        self.layers.push(layer);

        // Clear cache and emit event
        self.clear_cache();
        self.event_sender.emit(CompEvent::LayersChanged {
            comp_uuid: self.uuid.clone(),
        });

        Ok(())
    }

    /// Move a layer to a new start position, preserving duration.
    pub fn move_layer(&mut self, layer_idx: usize, new_start: usize) -> anyhow::Result<()> {
        let layer = self
            .layers
            .get_mut(layer_idx)
            .ok_or_else(|| anyhow::anyhow!("Layer {} not found", layer_idx))?;

        let old_start = layer.attrs.get_u32("start").unwrap_or(0) as usize;
        let old_end = layer.attrs.get_u32("end").unwrap_or(0) as usize;
        let duration = if old_end >= old_start {
            old_end - old_start
        } else {
            0
        };

        layer.attrs.set("start", AttrValue::UInt(new_start as u32));
        layer
            .attrs
            .set("end", AttrValue::UInt((new_start + duration) as u32));

        // Clear cache and emit event
        self.clear_cache();
        self.event_sender.emit(CompEvent::LayersChanged {
            comp_uuid: self.uuid.clone(),
        });

        Ok(())
    }

    /// Trim layer start (adjust trim_start attribute).
    pub fn trim_layer_start(&mut self, layer_idx: usize, new_trim: i32) -> anyhow::Result<()> {
        let layer = self
            .layers
            .get_mut(layer_idx)
            .ok_or_else(|| anyhow::anyhow!("Layer {} not found", layer_idx))?;

        layer.attrs.set("trim_start", AttrValue::Int(new_trim));

        // Clear cache and emit event
        self.clear_cache();
        self.event_sender.emit(CompEvent::LayersChanged {
            comp_uuid: self.uuid.clone(),
        });

        Ok(())
    }

    /// Trim layer end (adjust trim_end attribute).
    pub fn trim_layer_end(&mut self, layer_idx: usize, new_trim: i32) -> anyhow::Result<()> {
        let layer = self
            .layers
            .get_mut(layer_idx)
            .ok_or_else(|| anyhow::anyhow!("Layer {} not found", layer_idx))?;

        layer.attrs.set("trim_end", AttrValue::Int(new_trim));

        // Clear cache and emit event
        self.clear_cache();
        self.event_sender.emit(CompEvent::LayersChanged {
            comp_uuid: self.uuid.clone(),
        });

        Ok(())
    }

    /// Get all layer edges (start and end frames) sorted by distance from given frame
    /// Returns vec of (frame_number, is_start) tuples
    pub fn get_layer_edges_near(&self, from_frame: usize) -> Vec<(usize, bool)> {
        let mut edges = Vec::new();

        for layer in &self.layers {
            let start = layer.attrs.get_u32("start").unwrap_or(0) as usize;
            let end = layer.attrs.get_u32("end").unwrap_or(0) as usize;
            let trim_start = layer.attrs.get_i32("trim_start").unwrap_or(0);
            let trim_end = layer.attrs.get_i32("trim_end").unwrap_or(0);

            // Visible range accounting for trim
            let visible_start = start + trim_start as usize;
            let visible_end = end.saturating_sub(trim_end as usize);

            if visible_start < visible_end {
                edges.push((visible_start, true));   // Start edge
                edges.push((visible_end, false));    // End edge
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clip::Clip;
    use crate::media::MediaSource;
    use crate::project::Project;

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
        project.media.insert(clip_uuid.clone(), MediaSource::Clip(clip));
        project.clips_order.push(clip_uuid.clone());

        // Create Comp B with clip as layer
        let mut comp_b = Comp::new("Comp B", 0, 9, 24.0);
        let layer_b = Layer::new(clip_uuid.clone(), 0, 9);
        comp_b.layers.push(layer_b);
        let comp_b_uuid = comp_b.uuid.clone();
        project.media.insert(comp_b_uuid.clone(), MediaSource::Comp(comp_b));
        project.comps_order.push(comp_b_uuid.clone());

        // Create Comp A with Comp B as layer
        let mut comp_a = Comp::new("Comp A", 0, 9, 24.0);
        let layer_a = Layer::new(comp_b_uuid.clone(), 0, 9);
        comp_a.layers.push(layer_a);
        let comp_a_uuid = comp_a.uuid.clone();
        project.media.insert(comp_a_uuid.clone(), MediaSource::Comp(comp_a));
        project.comps_order.push(comp_a_uuid.clone());

        // Test: Get frame from Comp A (should recursively resolve through Comp B to Clip)
        let comp_a = project.media.get(&comp_a_uuid).unwrap().as_comp().unwrap();
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
        project.media.insert(clip_uuid.clone(), MediaSource::Clip(clip));

        // Create Comp with clip as layer
        let mut comp = Comp::new("Test Comp", 0, 4, 24.0);
        let layer = Layer::new(clip_uuid.clone(), 0, 4);
        comp.layers.push(layer);
        let comp_uuid = comp.uuid.clone();
        project.media.insert(comp_uuid.clone(), MediaSource::Comp(comp));

        // Get frame 2 - should cache it
        let comp = project.media.get(&comp_uuid).unwrap().as_comp().unwrap();
        let _frame1 = comp.get_frame(2, &project).unwrap();
        let cache_size_before = comp.cache.borrow().len();
        assert_eq!(cache_size_before, 1, "Cache should have 1 entry");

        // Get same frame - should hit cache
        let _frame2 = comp.get_frame(2, &project).unwrap();
        // Frames should be clones (cache hit)
        assert_eq!(comp.cache.borrow().len(), 1, "Cache size should stay the same");

        // Modify layer (change opacity) - should invalidate cache
        {
            let comp_mut = project.media.get_mut(&comp_uuid).unwrap().as_comp_mut().unwrap();
            comp_mut.layers[0].attrs.set("opacity", crate::attrs::AttrValue::Float(0.5));
        } // Release mutable borrow

        // Get frame again - cache should add new entry with different hash
        let comp = project.media.get(&comp_uuid).unwrap().as_comp().unwrap();
        let _frame3 = comp.get_frame(2, &project).unwrap();
        assert_eq!(comp.cache.borrow().len(), 2, "Cache should have both old and new entries (different hashes)");
        // Success - cache uses hash-based keys, old entry with old hash remains, new entry with new hash added
    }

    /// Test hash computation consistency
    #[test]
    fn test_layers_hash_consistency() {
        let mut comp1 = Comp::new("Comp1", 0, 10, 24.0);
        let mut comp2 = Comp::new("Comp2", 0, 10, 24.0);

        // Same layers should produce same hash
        let layer1 = Layer::new("uuid1".to_string(), 0, 10);
        let layer2 = Layer::new("uuid1".to_string(), 0, 10);

        comp1.layers.push(layer1);
        comp2.layers.push(layer2);

        let hash1 = comp1.compute_layers_hash();
        let hash2 = comp2.compute_layers_hash();
        assert_eq!(hash1, hash2, "Identical layers should produce same hash");

        // Different opacity should produce different hash
        comp2.layers[0].attrs.set("opacity", crate::attrs::AttrValue::Float(0.7));
        let hash3 = comp2.compute_layers_hash();
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
            project.media.insert(clip_uuid.clone(), MediaSource::Clip(clip));
            project.clips_order.push(clip_uuid.clone());
        }

        // Create Comp with all 3 clips as layers
        let mut comp = Comp::new("Multi-layer Comp", 0, 4, 24.0);

        // Layer 0: clip 0, full opacity
        let layer0 = Layer::new(project.clips_order[0].clone(), 0, 4);
        comp.layers.push(layer0);

        // Layer 1: clip 1, 50% opacity
        let mut layer1 = Layer::new(project.clips_order[1].clone(), 0, 4);
        layer1.attrs.set("opacity", crate::attrs::AttrValue::Float(0.5));
        comp.layers.push(layer1);

        // Layer 2: clip 2, 30% opacity
        let mut layer2 = Layer::new(project.clips_order[2].clone(), 0, 4);
        layer2.attrs.set("opacity", crate::attrs::AttrValue::Float(0.3));
        comp.layers.push(layer2);

        let comp_uuid = comp.uuid.clone();
        project.media.insert(comp_uuid.clone(), MediaSource::Comp(comp));

        // Get frame - should blend all 3 layers
        let comp = project.media.get(&comp_uuid).unwrap().as_comp().unwrap();
        let frame = comp.get_frame(2, &project);

        assert!(frame.is_some(), "Multi-layer composition should succeed");

        // Verify cache contains the blended result
        assert_eq!(comp.cache.borrow().len(), 1, "Cache should contain blended frame");
    }
}
