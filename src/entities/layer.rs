//! Layer and Track structures for composition timeline.
//!
//! # Architecture
//!
//! This module provides typed wrappers around the flexible `Attrs` storage:
//! - `Layer` - single clip placement on timeline with timing/visual properties
//! - `Track` - collection of non-overlapping layers (like DAW audio tracks)
//!
//! # Why Layer wraps Attrs?
//!
//! Instead of storing timing/visual properties as separate struct fields,
//! we keep them in `Attrs` (HashMap<String, AttrValue>). This allows:
//! - Unified dirty tracking (Attrs has built-in dirty flag)
//! - Easy serialization (Attrs is serde-friendly)
//! - Forward compatibility (new attrs don't break old saves)
//! - Consistent API with Comp which also uses Attrs
//!
//! Layer provides typed accessors (`in_frame()`, `opacity()`, etc.) on top.
//!
//! # Coordinate Systems
//!
//! - `in_frame` - where layer starts in PARENT timeline (absolute position)
//! - `src_len` - total source duration in frames (before speed)
//! - `trim_in/trim_out` - frames cut from start/end of source
//! - `speed` - playback rate (2.0 = 2x faster, 0.5 = half speed)
//!
//! Computed values:
//! - `start()` = `in_frame` (full bar start, ignores trim)
//! - `end()` = `in_frame + src_len/speed - 1` (full bar end)
//! - `play_start()` = visible start after trim applied
//! - `play_end()` = visible end after trim applied
//!
//! # Dependencies
//!
//! - `Attrs` from `super::attrs` - attribute storage with dirty tracking
//! - `A_*` constants from `super::keys` - standardized attribute names
//! - Used by: `Comp` (timeline), `timeline_ui` (rendering), `compose()` (blending)

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::attrs::Attrs;
use super::keys::*;

/// Single layer (clip) on the timeline.
///
/// Each layer represents one placement of a source Comp in a parent Comp.
/// The same source can be placed multiple times with different instance_uuids.
///
/// # Instance vs Source UUID
///
/// - `instance_uuid` - unique ID for THIS placement (generated on add)
/// - `source_uuid()` - the Comp being referenced (stored in attrs["uuid"])
///
/// Example: Same clip placed 3 times = 3 Layers with same source_uuid,
/// different instance_uuids, different timing.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Layer {
    /// Unique ID for this specific placement.
    /// Used for selection, deletion, reordering operations.
    pub instance_uuid: Uuid,

    /// All layer attributes stored in flexible HashMap.
    /// Key attributes: "uuid" (source), "in", "src_len", "trim_in/out",
    /// "speed", "opacity", "blend_mode", "visible", "mute".
    pub attrs: Attrs,
}

impl Layer {
    pub fn new(instance_uuid: Uuid, attrs: Attrs) -> Self {
        Self { instance_uuid, attrs }
    }

    /// Get source comp UUID
    pub fn source_uuid(&self) -> Option<Uuid> {
        self.attrs
            .get_str(A_UUID)
            .and_then(|s| Uuid::parse_str(s).ok())
    }

    /// Get layer name
    pub fn name(&self) -> &str {
        self.attrs.get_str(A_NAME).unwrap_or("Untitled")
    }

    /// Get in-frame (start position in parent timeline)
    pub fn in_frame(&self) -> i32 {
        self.attrs.get_i32_or_zero(A_IN)
    }

    /// Get source length
    pub fn src_len(&self) -> i32 {
        self.attrs.get_i32_or_zero("src_len")
    }

    /// Get trim in
    pub fn trim_in(&self) -> i32 {
        self.attrs.get_i32_or_zero(A_TRIM_IN)
    }

    /// Get trim out
    pub fn trim_out(&self) -> i32 {
        self.attrs.get_i32_or_zero(A_TRIM_OUT)
    }

    /// Get playback speed
    pub fn speed(&self) -> f32 {
        self.attrs.get_float_or(A_SPEED, 1.0).clamp(0.1, 4.0)
    }

    /// Visible start in parent coords (full_bar_start)
    pub fn start(&self) -> i32 {
        self.in_frame()
    }

    /// Visible end in parent coords (full_bar_end)
    pub fn end(&self) -> i32 {
        let in_val = self.in_frame();
        let src_len = self.src_len();
        let speed = self.speed();
        in_val + (src_len as f32 / speed).ceil() as i32 - 1
    }

    /// Play range start (with trim)
    pub fn play_start(&self) -> i32 {
        let in_val = self.in_frame();
        let trim_in = self.trim_in();
        let speed = self.speed();
        in_val + (trim_in as f32 / speed).round() as i32
    }

    /// Play range end (with trim)
    pub fn play_end(&self) -> i32 {
        let in_val = self.in_frame();
        let src_len = self.src_len();
        let trim_in = self.trim_in();
        let trim_out = self.trim_out();
        let speed = self.speed();
        let visible_src = (src_len - trim_in - trim_out).max(1);
        let visible_timeline = (visible_src as f32 / speed).round() as i32;
        in_val + (trim_in as f32 / speed).round() as i32 + visible_timeline - 1
    }

    /// Check if layer is visible
    pub fn visible(&self) -> bool {
        self.attrs.get_bool(A_VISIBLE).unwrap_or(true)
    }

    /// Check if layer is muted
    pub fn muted(&self) -> bool {
        self.attrs.get_bool(A_MUTE).unwrap_or(false)
    }

    /// Get opacity
    pub fn opacity(&self) -> f32 {
        self.attrs.get_float_or(A_OPACITY, 1.0)
    }

    /// Get blend mode
    pub fn blend_mode(&self) -> &str {
        self.attrs.get_str(A_BLEND_MODE).unwrap_or("normal")
    }
}

/// Track (row) on the timeline containing non-overlapping layers.
///
/// # Design Rationale
///
/// Tracks group layers that shouldn't overlap temporally (like audio tracks in DAW).
/// The current Comp uses greedy auto-layout (`compute_layer_rows()`) which assigns
/// layers to rows dynamically. Track struct enables future explicit track management.
///
/// # Invariants
///
/// - Layers within a track MUST NOT overlap (enforced by `can_place()`)
/// - Layers are kept sorted by `start()` for efficient lookup
/// - Empty tracks are allowed (user may want to reserve space)
///
/// # Future Use
///
/// When Comp migrates from `Vec<(Uuid, Attrs)>` to `Vec<Track>`:
/// - User can name/color tracks
/// - Lock tracks to prevent edits
/// - Explicit control over clip placement
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Track {
    /// User-visible track name (e.g., "Background", "FX", "Audio")
    pub name: String,

    /// Locked tracks reject edits (move, delete, add layers)
    pub locked: bool,

    /// Optional UI color as [R, G, B, A]. Used for track header in timeline.
    #[serde(default)]
    pub color: Option<[u8; 4]>,

    /// Layers sorted by start time. Non-overlapping invariant enforced by `can_place()`.
    pub layers: Vec<Layer>,
}

impl Track {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            locked: false,
            color: None,
            layers: Vec::new(),
        }
    }

    /// Check if layer can be placed without overlap
    pub fn can_place(&self, start: i32, end: i32) -> bool {
        !self.layers.iter().any(|layer| {
            let layer_start = layer.start();
            let layer_end = layer.end();
            // Overlap check
            start <= layer_end && end >= layer_start
        })
    }

    /// Add layer to track (maintains sorted order by start)
    pub fn add_layer(&mut self, layer: Layer) {
        let start = layer.start();
        let pos = self
            .layers
            .iter()
            .position(|l| l.start() > start)
            .unwrap_or(self.layers.len());
        self.layers.insert(pos, layer);
    }

    /// Remove layer by instance UUID
    pub fn remove_layer(&mut self, instance_uuid: Uuid) -> Option<Layer> {
        if let Some(pos) = self.layers.iter().position(|l| l.instance_uuid == instance_uuid) {
            Some(self.layers.remove(pos))
        } else {
            None
        }
    }

    /// Get layer at specific frame
    pub fn layer_at_frame(&self, frame: i32) -> Option<&Layer> {
        self.layers
            .iter()
            .find(|l| frame >= l.start() && frame <= l.end())
    }

    /// Get layer at specific frame (mutable)
    pub fn layer_at_frame_mut(&mut self, frame: i32) -> Option<&mut Layer> {
        self.layers
            .iter_mut()
            .find(|l| frame >= l.start() && frame <= l.end())
    }

    /// Find layer by instance UUID
    pub fn find_layer(&self, instance_uuid: Uuid) -> Option<&Layer> {
        self.layers.iter().find(|l| l.instance_uuid == instance_uuid)
    }

    /// Find layer by instance UUID (mutable)
    pub fn find_layer_mut(&mut self, instance_uuid: Uuid) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.instance_uuid == instance_uuid)
    }

    /// Check if track is empty
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    /// Get number of layers
    pub fn len(&self) -> usize {
        self.layers.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::AttrValue;

    fn make_layer(instance_uuid: Uuid, source_uuid: Uuid, in_frame: i32, src_len: i32) -> Layer {
        let mut attrs = Attrs::new();
        attrs.set(A_UUID, AttrValue::Str(source_uuid.to_string()));
        attrs.set(A_NAME, AttrValue::Str("Test".to_string()));
        attrs.set(A_IN, AttrValue::Int(in_frame));
        attrs.set(A_SRC_LEN, AttrValue::Int(src_len));
        attrs.set(A_TRIM_IN, AttrValue::Int(0));
        attrs.set(A_TRIM_OUT, AttrValue::Int(0));
        attrs.set(A_SPEED, AttrValue::Float(1.0));
        Layer::new(instance_uuid, attrs)
    }

    #[test]
    fn test_layer_timing() {
        let layer = make_layer(Uuid::new_v4(), Uuid::new_v4(), 10, 100);
        assert_eq!(layer.start(), 10);
        assert_eq!(layer.end(), 109); // 10 + 100 - 1
    }

    #[test]
    fn test_track_can_place() {
        let mut track = Track::new("Track 1");
        let layer = make_layer(Uuid::new_v4(), Uuid::new_v4(), 0, 50);
        track.add_layer(layer);

        // Should not be able to place overlapping layer
        assert!(!track.can_place(25, 75));
        // Should be able to place after
        assert!(track.can_place(50, 100));
        // Should be able to place before
        assert!(track.can_place(-50, -1));
    }

    #[test]
    fn test_track_sorted_insert() {
        let mut track = Track::new("Track 1");

        // Add in non-sorted order
        track.add_layer(make_layer(Uuid::new_v4(), Uuid::new_v4(), 100, 10));
        track.add_layer(make_layer(Uuid::new_v4(), Uuid::new_v4(), 0, 10));
        track.add_layer(make_layer(Uuid::new_v4(), Uuid::new_v4(), 50, 10));

        // Should be sorted by start
        assert_eq!(track.layers[0].start(), 0);
        assert_eq!(track.layers[1].start(), 50);
        assert_eq!(track.layers[2].start(), 100);
    }
}
