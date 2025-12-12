//! FileNode - loads image sequences and video files from disk.
//!
//! Replaces the COMP_FILE mode from Comp. This node type has no inputs
//! and produces frames by loading them from disk based on file_mask pattern.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::attrs::{AttrValue, Attrs};
use super::frame::{CropAlign, Frame, PixelDepth};
use super::keys::*;
use super::node::{ComputeContext, Node};
use crate::utils::media;

/// Node that loads frames from image sequences or video files.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileNode {
    /// Persistent attributes: uuid, name, file_mask, file_start, file_end, fps, width, height
    pub attrs: Attrs,
}

impl FileNode {
    /// Create new file node from file mask pattern.
    ///
    /// # Arguments
    /// * `file_mask` - Path pattern with * for frame numbers (e.g., "frame.*.exr")
    /// * `start` - First frame number in sequence
    /// * `end` - Last frame number in sequence
    /// * `fps` - Frames per second
    pub fn new(file_mask: String, start: i32, end: i32, fps: f32) -> Self {
        let mut attrs = Attrs::new();
        let uuid = Uuid::new_v4();
        
        attrs.set_uuid(A_UUID, uuid);
        attrs.set(A_NAME, AttrValue::Str(file_mask.clone()));
        attrs.set(A_FILE_MASK, AttrValue::Str(file_mask));
        attrs.set(A_FILE_START, AttrValue::Int(start));
        attrs.set(A_FILE_END, AttrValue::Int(end));
        attrs.set(A_IN, AttrValue::Int(start));
        attrs.set(A_OUT, AttrValue::Int(end));
        attrs.set(A_TRIM_IN, AttrValue::Int(0));
        attrs.set(A_TRIM_OUT, AttrValue::Int(0));
        attrs.set(A_FPS, AttrValue::Float(fps));
        attrs.set(A_FRAME, AttrValue::Int(start));
        attrs.set(A_WIDTH, AttrValue::UInt(64));
        attrs.set(A_HEIGHT, AttrValue::UInt(64));
        
        Self { attrs }
    }
    
    /// Create with specified UUID (for deserialization)
    pub fn with_uuid(mut self, uuid: Uuid) -> Self {
        self.attrs.set_uuid(A_UUID, uuid);
        self
    }
    
    // --- Getters ---
    
    pub fn file_mask(&self) -> Option<String> {
        self.attrs.get_str(A_FILE_MASK).map(|s| s.to_string())
    }
    
    pub fn file_start(&self) -> Option<i32> {
        self.attrs.get_i32(A_FILE_START)
    }
    
    pub fn file_end(&self) -> Option<i32> {
        self.attrs.get_i32(A_FILE_END)
    }
    
    pub fn _in(&self) -> i32 {
        self.attrs.get_i32(A_IN).unwrap_or(0)
    }
    
    pub fn _out(&self) -> i32 {
        self.attrs.get_i32(A_OUT).unwrap_or(0)
    }
    
    pub fn fps(&self) -> f32 {
        self.attrs.get_float(A_FPS).unwrap_or(24.0)
    }
    
    pub fn dim(&self) -> (usize, usize) {
        let w = self.attrs.get_u32(A_WIDTH).unwrap_or(64) as usize;
        let h = self.attrs.get_u32(A_HEIGHT).unwrap_or(64) as usize;
        (w.max(1), h.max(1))
    }
    
    pub fn frame_count(&self) -> i32 {
        (self._out() - self._in() + 1).max(0)
    }
    
    /// Work area (trimmed range) in absolute frames
    pub fn work_area_abs(&self) -> (i32, i32) {
        let trim_in = self.attrs.get_i32(A_TRIM_IN).unwrap_or(0);
        let trim_out = self.attrs.get_i32(A_TRIM_OUT).unwrap_or(0);
        (self._in() + trim_in, self._out() - trim_out)
    }
    
    // --- Internal ---
    
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
}

impl Node for FileNode {
    fn uuid(&self) -> Uuid {
        self.attrs.get_uuid(A_UUID).unwrap_or_else(Uuid::nil)
    }
    
    fn name(&self) -> &str {
        self.attrs.get_str(A_NAME).unwrap_or("Untitled")
    }
    
    fn node_type(&self) -> &'static str {
        "File"
    }
    
    fn attrs(&self) -> &Attrs {
        &self.attrs
    }
    
    fn attrs_mut(&mut self) -> &mut Attrs {
        &mut self.attrs
    }
    
    fn inputs(&self) -> Vec<Uuid> {
        vec![] // FileNode has no inputs
    }
    
    fn compute(&self, frame_idx: i32, ctx: &ComputeContext) -> Option<Frame> {
        let duration = self.frame_count();
        if duration <= 0 {
            return None;
        }

        let (work_start, work_end) = self.work_area_abs();
        if work_end < work_start {
            return Some(self.placeholder_frame());
        }

        // Outside work area -> placeholder
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

        // Map local frame_idx to absolute sequence number
        let seq_start = self.file_start().unwrap_or(self._in());
        let seq_end = self.file_end().unwrap_or(self._out());
        let seq_frame = seq_start.saturating_add(local_idx);
        if seq_frame < seq_start || seq_frame > seq_end {
            return Some(self.placeholder_frame());
        }

        // Check cache
        let my_uuid = self.uuid();
        if let Some(frame) = ctx.cache.get(my_uuid, frame_idx) {
            return Some(frame);
        }

        // Cache miss: load frame from disk
        let frame_path = self.resolve_frame_path(seq_frame).unwrap_or_default();
        if frame_path.as_os_str().is_empty() {
            return Some(self.placeholder_frame());
        }

        let frame = self.frame_from_path(frame_path);

        // Insert into cache
        ctx.cache.insert(my_uuid, frame_idx, frame.clone());

        Some(frame)
    }
    
    fn is_dirty(&self) -> bool {
        self.attrs.is_dirty()
    }
    
    fn mark_dirty(&self) {
        self.attrs.mark_dirty()
    }
    
    fn clear_dirty(&self) {
        self.attrs.clear_dirty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_file_node_creation() {
        let node = FileNode::new("test.*.exr".to_string(), 1, 100, 24.0);
        assert_eq!(node.file_mask(), Some("test.*.exr".to_string()));
        assert_eq!(node.file_start(), Some(1));
        assert_eq!(node.file_end(), Some(100));
        assert_eq!(node.fps(), 24.0);
        assert_eq!(node.frame_count(), 100);
    }
    
    #[test]
    fn test_file_node_trait() {
        let node = FileNode::new("test.*.exr".to_string(), 1, 100, 24.0);
        assert_eq!(node.node_type(), "File");
        assert!(node.inputs().is_empty());
    }
}
