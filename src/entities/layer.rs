//! Layer: simple (uuid, attrs) tuple for referencing MediaSource in Comp.
//!
//! Layer is just a reference - actual source is resolved from Project.media at runtime.

use serde::{Deserialize, Serialize};

use crate::attrs::{Attrs, AttrValue};

/// Layer reference - just UUID + attributes, nothing more
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Layer {
    /// UUID of referenced MediaSource in Project.media
    pub source_uuid: String,

    /// Layer attributes (start, end, trims, transforms, blend mode, etc.)
    pub attrs: Attrs,
}

impl Layer {
    /// Create new layer with default attributes
    pub fn new(source_uuid: String, source_start: usize, source_end: usize) -> Self {
        let mut attrs = Attrs::new();

        attrs.set("name", AttrValue::Str("Layer".to_string()));
        attrs.set("start", AttrValue::UInt(source_start as u32));
        attrs.set("end", AttrValue::UInt(source_end as u32));
        attrs.set("play_start", AttrValue::Int(0));
        attrs.set("play_end", AttrValue::Int(0));
        attrs.set("opacity", AttrValue::Float(1.0));

        Self { source_uuid, attrs }
    }

    /// Get layer start frame
    pub fn start(&self) -> usize {
        self.attrs.get_u32("start").unwrap_or(0) as usize
    }

    /// Get layer end frame
    pub fn end(&self) -> usize {
        self.attrs.get_u32("end").unwrap_or(0) as usize
    }

    /// Get layer name
    pub fn name(&self) -> &str {
        self.attrs.get_str("name").unwrap_or("Layer")
    }

    /// Get layer opacity
    pub fn opacity(&self) -> f32 {
        self.attrs.get_float("opacity").unwrap_or(1.0)
    }
}
