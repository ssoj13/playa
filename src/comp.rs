//! Composition-level types (timeline unit for playback/encoding).
//!
//! `Comp` references Layers, Clips (via Layers), and owns
//! a simple per-comp cache for composed frames.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::attrs::{Attrs, AttrValue};
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

    /// Per-comp frame cache: global frame index -> composed Frame (runtime-only)
    #[serde(skip)]
    #[serde(default)]
    cache: HashMap<usize, Frame>,
}

fn gen_comp_uuid(name: &str, start: usize, end: usize) -> String {
    format!("comp:{}:{}:{}", name, start, end)
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
            uuid: gen_comp_uuid(&name_str, start, end),
            name: name_str,
            start,
            end,
            fps,
            attrs,
            layers: Vec::new(),
            current_frame: start, // Start at beginning of comp
            cache: HashMap::new(),
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
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    /// Get composed frame at given global frame index.
    ///
    /// If frame is not in cache, composes it via `compose()`,
    /// stores in cache, and returns the result.
    pub fn get_frame(&mut self, frame_idx: usize) -> Option<Frame> {
        if let Some(frame) = self.cache.get(&frame_idx) {
            return Some(frame.clone());
        }

        let composed = self.compose(frame_idx)?;
        self.cache.insert(frame_idx, composed.clone());
        Some(composed)
    }

    /// Compose frame at given global frame index.
    ///
    /// For now, this is a minimal implementation:
    /// - If there is at least one Layer
    /// - Delegates to Layer::get_frame (single-layer case)
    ///
    /// Later это будет заменено на полноценный мульти-layer compositing.
    pub fn compose(&self, frame_idx: usize) -> Option<Frame> {
        let layer = self.layers.first()?;
        layer.get_frame(frame_idx)
    }
}
