//! Layer: references a single Clip within a Comp timeline.
//!
//! Holds a reference to `Clip` plus editable attributes
//! (name, trims, visibility, transforms, etc.).

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::attrs::{Attrs, AttrValue};
use crate::clip::Clip;
use crate::frame::Frame;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Layer {
    /// UUID of referenced clip (serialized)
    pub clip_uuid: Option<String>,

    /// Runtime clip reference (rebuilt from clip_uuid)
    #[serde(skip)]
    pub clip: Option<Arc<Clip>>,

    /// Layer attributes (name, trims, transforms, mode, etc.)
    pub attrs: Attrs,
}

impl Layer {
    pub fn new(clip: Arc<Clip>) -> Self {
        let uuid = clip.uuid.clone();
        let mut layer = Self {
            clip_uuid: Some(uuid.clone()),
            clip: Some(clip.clone()),
            attrs: Attrs::new(),
        };

        // Basic defaults
        layer.attrs.set("name", AttrValue::Str("Layer".to_string()));
        layer.attrs.set("clip_uuid", AttrValue::Str(uuid));

        // Resolution defaults (x, y, depth)
        let (xres, yres) = clip.resolution();
        layer
            .attrs
            .set("resolution_x", AttrValue::UInt(xres as u32));
        layer
            .attrs
            .set("resolution_y", AttrValue::UInt(yres as u32));
        layer
            .attrs
            .set("resolution_depth", AttrValue::UInt(4)); // RGBA

        // Clip range (0-based indices into Clip frames)
        let clip_start = 0_i32;
        let clip_end = clip.len().saturating_sub(1) as i32;
        layer.set_i32("clip_start", clip_start);
        layer.set_i32("clip_end", clip_end);

        // Trims
        layer.set_i32("trim_start", 0);
        layer.set_i32("trim_end", 0);

        // Computed layer start/end on comp timeline
        let start = clip_start + layer.trim_start();
        let end = clip_end + layer.trim_end();
        layer.set_i32("start", start);
        layer.set_i32("end", end);

        layer
    }

    pub fn clear_clip(&mut self) {
        self.clip = None;
        self.clip_uuid = None;
        self.attrs.set("clip_uuid", AttrValue::Str(String::new()));
    }

    pub fn set_clip(&mut self, clip: Arc<Clip>) {
        let uuid = clip.uuid.clone();
        self.clip_uuid = Some(uuid.clone());
        self.clip = Some(clip);
        self.attrs.set("clip_uuid", AttrValue::Str(uuid));
    }

    pub fn clip(&self) -> Option<&Arc<Clip>> {
        self.clip.as_ref()
    }

    /// Get layer name from attrs (fallback: "Layer")
    pub fn name(&self) -> String {
        self.attrs
            .get_str("name")
            .unwrap_or("Layer")
            .to_string()
    }

    fn get_i32(&self, key: &str, default: i32) -> i32 {
        match self.attrs.get(key) {
            Some(AttrValue::Int(v)) => *v,
            Some(AttrValue::UInt(v)) => *v as i32,
            _ => default,
        }
    }

    fn set_i32(&mut self, key: &str, value: i32) {
        self.attrs.set(key, AttrValue::Int(value));
    }

    pub fn clip_start(&self) -> i32 {
        self.get_i32("clip_start", 0)
    }

    pub fn clip_end(&self) -> i32 {
        self.get_i32("clip_end", 0)
    }

    pub fn trim_start(&self) -> i32 {
        self.get_i32("trim_start", 0)
    }

    pub fn trim_end(&self) -> i32 {
        self.get_i32("trim_end", 0)
    }

    pub fn start(&self) -> i32 {
        self.get_i32("start", self.clip_start() + self.trim_start())
    }

    pub fn end(&self) -> i32 {
        self.get_i32("end", self.clip_end() + self.trim_end())
    }

    /// Get composed frame for given comp-global frame index.
    ///
    /// - If frame is outside layer start/end: None (layer inactive).
    /// - If frame is within layer but outside clip range due to trims:
    ///   extend first/last clip frame.
    pub fn get_frame(&self, global_frame: usize) -> Option<Frame> {
        let g = global_frame as i64;
        let start = self.start() as i64;
        let end = self.end() as i64;

        if g < start || g > end {
            return None;
        }

        let clip_start = self.clip_start() as i64;
        let clip_end = self.clip_end() as i64;

        // Offset from layer start into clip timeline (before clamping)
        let mut clip_idx = clip_start + (g - start);

        // Extend first/last frame when outside clip range
        if clip_idx < clip_start {
            clip_idx = clip_start;
        } else if clip_idx > clip_end {
            clip_idx = clip_end;
        }

        if clip_idx < 0 {
            return None;
        }

        let clip_idx_usize = clip_idx as usize;
        let clip = self.clip.as_ref()?;
        clip.get_frame(clip_idx_usize).cloned()
    }
}
