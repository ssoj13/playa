//! MediaSource - unified enum for Clip and Comp
//!
//! Provides common interface for frame sources

use serde::{Deserialize, Serialize};
use crate::entities::{Attrs, Clip, Comp};
use crate::frame::Frame;

/// Unified media source - can be either Clip or Comp
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MediaSource {
    Clip(Clip),
    Comp(Comp),
}

impl MediaSource {
    /// Get unique identifier
    pub fn uuid(&self) -> &str {
        match self {
            MediaSource::Clip(c) => &c.uuid,
            MediaSource::Comp(c) => &c.uuid,
        }
    }

    /// Get frame at given index (returns owned Frame)
    /// For Comp sources, requires reference to Project for recursive resolution
    pub fn get_frame(&self, frame: usize, project: &crate::entities::Project) -> Option<Frame> {
        match self {
            MediaSource::Clip(c) => c.get_frame(frame).cloned(),
            MediaSource::Comp(c) => c.get_frame(frame, project),
        }
    }

    /// Total number of frames (for Comp returns play_frame_count - active work area)
    pub fn total_frames(&self) -> usize {
        match self {
            MediaSource::Clip(c) => c.total_frames(),
            MediaSource::Comp(c) => c.play_frame_count(),
        }
    }

    /// Get frame range (start, end)
    pub fn frame_range(&self) -> (usize, usize) {
        match self {
            MediaSource::Clip(c) => (c.start(), c.end()),
            MediaSource::Comp(c) => (c.start(), c.end()),
        }
    }

    /// Get framerate
    pub fn fps(&self) -> f32 {
        match self {
            MediaSource::Clip(c) => c.fps(),
            MediaSource::Comp(c) => c.fps(),
        }
    }

    /// Get attributes
    pub fn attrs(&self) -> &Attrs {
        match self {
            MediaSource::Clip(c) => &c.attrs,
            MediaSource::Comp(c) => &c.attrs,
        }
    }

    /// Get mutable attributes
    pub fn attrs_mut(&mut self) -> &mut Attrs {
        match self {
            MediaSource::Clip(c) => &mut c.attrs,
            MediaSource::Comp(c) => &mut c.attrs,
        }
    }

    /// Get name/pattern for display
    pub fn name(&self) -> &str {
        match self {
            MediaSource::Clip(c) => c.pattern(),
            MediaSource::Comp(c) => c.name(),
        }
    }

    /// Check if this is a Clip
    pub fn is_clip(&self) -> bool {
        matches!(self, MediaSource::Clip(_))
    }

    /// Check if this is a Comp
    pub fn is_comp(&self) -> bool {
        matches!(self, MediaSource::Comp(_))
    }

    /// Get reference to Clip if this is a Clip
    pub fn as_clip(&self) -> Option<&Clip> {
        match self {
            MediaSource::Clip(c) => Some(c),
            MediaSource::Comp(_) => None,
        }
    }

    /// Get reference to Comp if this is a Comp
    pub fn as_comp(&self) -> Option<&Comp> {
        match self {
            MediaSource::Clip(_) => None,
            MediaSource::Comp(c) => Some(c),
        }
    }

    /// Get mutable reference to Comp if this is a Comp
    pub fn as_comp_mut(&mut self) -> Option<&mut Comp> {
        match self {
            MediaSource::Clip(_) => None,
            MediaSource::Comp(c) => Some(c),
        }
    }
}
