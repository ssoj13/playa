//! NodeKind - enum wrapper for all node types.
//!
//! Provides unified interface for storing different node types
//! in Project.media HashMap.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::attrs::Attrs;
use super::camera_node::CameraNode;
use super::comp_node::CompNode;
use super::file_node::FileNode;
use super::frame::Frame;
use super::node::{ComputeContext, Node};
use super::text_node::TextNode;

/// Enum containing all possible node types.
/// Used in Project.media for unified storage.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NodeKind {
    File(FileNode),
    Comp(CompNode),
    Camera(CameraNode),
    Text(TextNode),
}

impl NodeKind {
    /// Check if this is a file node
    pub fn is_file(&self) -> bool {
        matches!(self, NodeKind::File(_))
    }
    
    /// Check if this is a comp node
    pub fn is_comp(&self) -> bool {
        matches!(self, NodeKind::Comp(_))
    }
    
    /// Check if this is a camera node
    pub fn is_camera(&self) -> bool {
        matches!(self, NodeKind::Camera(_))
    }
    
    /// Check if this is a text node
    pub fn is_text(&self) -> bool {
        matches!(self, NodeKind::Text(_))
    }
    
    /// Play range (work area)
    pub fn play_range(&self, use_work_area: bool) -> (i32, i32) {
        match self {
            NodeKind::File(n) => n.work_area_abs(),
            NodeKind::Comp(n) => n.play_range(use_work_area),
            NodeKind::Camera(_) => (0, 99), // 100 frames default duration
            NodeKind::Text(_) => (0, 99),    // 100 frames default duration
        }
    }
    
    /// Actual content bounds (for zoom-to-fit)
    /// If selection_only=true and comp has selection, only selected layers are considered
    pub fn bounds(&self, use_trim: bool, selection_only: bool) -> (i32, i32) {
        match self {
            NodeKind::File(n) => n.work_area_abs(),
            NodeKind::Comp(n) => n.bounds(use_trim, selection_only),
            NodeKind::Camera(_) => (0, 99),
            NodeKind::Text(_) => (0, 99),
        }
    }
    
    /// Frame count
    pub fn frame_count(&self) -> i32 {
        match self {
            NodeKind::File(n) => n.frame_count(),
            NodeKind::Comp(n) => n.frame_count(),
            NodeKind::Camera(_) => 100, // 100 frames default
            NodeKind::Text(_) => 100,   // 100 frames default
        }
    }
    
    /// Dimensions (width, height)
    pub fn dim(&self) -> (usize, usize) {
        match self {
            NodeKind::File(n) => n.dim(),
            NodeKind::Comp(n) => n.dim(),
            NodeKind::Camera(_) => (0, 0), // No dimensions
            NodeKind::Text(n) => {
                let w = n.width().max(1) as usize;
                let h = n.height().max(1) as usize;
                (w, h)
            }
        }
    }
    
    /// Add child layer (only works on CompNode)
    pub fn add_child_layer(
        &mut self,
        source_uuid: Uuid,
        name: &str,
        start_frame: i32,
        duration: i32,
        insert_idx: Option<usize>,
        source_dim: (usize, usize),
    ) -> anyhow::Result<Uuid> {
        match self {
            NodeKind::Comp(comp) => comp.add_child_layer(source_uuid, name, start_frame, duration, insert_idx, source_dim),
            _ => anyhow::bail!("Cannot add child to non-Comp node"),
        }
    }
    
    /// Get as FileNode reference
    pub fn as_file(&self) -> Option<&FileNode> {
        match self {
            NodeKind::File(n) => Some(n),
            _ => None,
        }
    }
    
    /// Get as FileNode mutable reference
    pub fn as_file_mut(&mut self) -> Option<&mut FileNode> {
        match self {
            NodeKind::File(n) => Some(n),
            _ => None,
        }
    }
    
    /// Get as CompNode reference
    pub fn as_comp(&self) -> Option<&CompNode> {
        match self {
            NodeKind::Comp(n) => Some(n),
            _ => None,
        }
    }
    
    /// Get as CompNode mutable reference
    pub fn as_comp_mut(&mut self) -> Option<&mut CompNode> {
        match self {
            NodeKind::Comp(n) => Some(n),
            _ => None,
        }
    }
    
    /// Get as CameraNode reference
    pub fn as_camera(&self) -> Option<&CameraNode> {
        match self {
            NodeKind::Camera(n) => Some(n),
            _ => None,
        }
    }
    
    /// Get as CameraNode mutable reference
    pub fn as_camera_mut(&mut self) -> Option<&mut CameraNode> {
        match self {
            NodeKind::Camera(n) => Some(n),
            _ => None,
        }
    }
    
    /// Get as TextNode reference
    pub fn as_text(&self) -> Option<&TextNode> {
        match self {
            NodeKind::Text(n) => Some(n),
            _ => None,
        }
    }
    
    /// Get as TextNode mutable reference
    pub fn as_text_mut(&mut self) -> Option<&mut TextNode> {
        match self {
            NodeKind::Text(n) => Some(n),
            _ => None,
        }
    }
    
    /// Check if this is a file-mode node (FileNode = true, CompNode = false)
    pub fn is_file_mode(&self) -> bool {
        matches!(self, NodeKind::File(_))
    }
    
    /// Get FPS
    pub fn fps(&self) -> f32 {
        match self {
            NodeKind::File(n) => n.fps(),
            NodeKind::Comp(n) => n.fps(),
            NodeKind::Camera(_) => 24.0, // Default
            NodeKind::Text(_) => 24.0,   // Default
        }
    }
    
    /// Get file mask (only for FileNode)
    pub fn file_mask(&self) -> Option<String> {
        match self {
            NodeKind::File(n) => n.file_mask().map(|s| s.to_string()),
            _ => None,
        }
    }
    
    /// Get start frame (_in)
    pub fn _in(&self) -> i32 {
        match self {
            NodeKind::File(n) => n._in(),
            NodeKind::Comp(n) => n._in(),
            NodeKind::Camera(_) => 0,
            NodeKind::Text(_) => 0,
        }
    }
    
    /// Get end frame (_out)
    pub fn _out(&self) -> i32 {
        match self {
            NodeKind::File(n) => n._out(),
            NodeKind::Comp(n) => n._out(),
            NodeKind::Camera(_) => 0,
            NodeKind::Text(_) => 1,
        }
    }
    
    /// Get current frame (playhead)
    pub fn frame(&self) -> i32 {
        match self {
            NodeKind::File(n) => n.frame(),
            NodeKind::Comp(n) => n.frame(),
            NodeKind::Camera(_) => 0,
            NodeKind::Text(_) => 0,
        }
    }
    
    /// Set event emitter (only affects CompNode)
    pub fn set_event_emitter(&mut self, emitter: crate::core::event_bus::CompEventEmitter) {
        if let NodeKind::Comp(n) = self {
            n.set_event_emitter(emitter);
        }
    }
}

// Implement Node trait by delegating to inner node
impl Node for NodeKind {
    fn uuid(&self) -> Uuid {
        match self {
            NodeKind::File(n) => n.uuid(),
            NodeKind::Comp(n) => n.uuid(),
            NodeKind::Camera(n) => n.uuid(),
            NodeKind::Text(n) => n.uuid(),
        }
    }
    
    fn name(&self) -> &str {
        match self {
            NodeKind::File(n) => n.name(),
            NodeKind::Comp(n) => n.name(),
            NodeKind::Camera(n) => n.name(),
            NodeKind::Text(n) => n.name(),
        }
    }
    
    fn node_type(&self) -> &'static str {
        match self {
            NodeKind::File(n) => n.node_type(),
            NodeKind::Comp(n) => n.node_type(),
            NodeKind::Camera(n) => n.node_type(),
            NodeKind::Text(n) => n.node_type(),
        }
    }
    
    fn attrs(&self) -> &Attrs {
        match self {
            NodeKind::File(n) => n.attrs(),
            NodeKind::Comp(n) => n.attrs(),
            NodeKind::Camera(n) => n.attrs(),
            NodeKind::Text(n) => n.attrs(),
        }
    }
    
    fn attrs_mut(&mut self) -> &mut Attrs {
        match self {
            NodeKind::File(n) => n.attrs_mut(),
            NodeKind::Comp(n) => n.attrs_mut(),
            NodeKind::Camera(n) => n.attrs_mut(),
            NodeKind::Text(n) => n.attrs_mut(),
        }
    }
    
    fn inputs(&self) -> Vec<Uuid> {
        match self {
            NodeKind::File(n) => n.inputs(),
            NodeKind::Comp(n) => n.inputs(),
            NodeKind::Camera(n) => n.inputs(),
            NodeKind::Text(n) => n.inputs(),
        }
    }
    
    fn compute(&self, frame: i32, ctx: &ComputeContext) -> Option<Frame> {
        match self {
            NodeKind::File(n) => n.compute(frame, ctx),
            NodeKind::Comp(n) => n.compute(frame, ctx),
            NodeKind::Camera(n) => n.compute(frame, ctx),
            NodeKind::Text(n) => n.compute(frame, ctx),
        }
    }
    
    fn is_dirty(&self) -> bool {
        match self {
            NodeKind::File(n) => n.is_dirty(),
            NodeKind::Comp(n) => n.is_dirty(),
            NodeKind::Camera(n) => n.is_dirty(),
            NodeKind::Text(n) => n.is_dirty(),
        }
    }
    
    fn mark_dirty(&self) {
        match self {
            NodeKind::File(n) => n.mark_dirty(),
            NodeKind::Comp(n) => n.mark_dirty(),
            NodeKind::Camera(n) => n.mark_dirty(),
            NodeKind::Text(n) => n.mark_dirty(),
        }
    }
    
    fn clear_dirty(&self) {
        match self {
            NodeKind::File(n) => n.clear_dirty(),
            NodeKind::Comp(n) => n.clear_dirty(),
            NodeKind::Camera(n) => n.clear_dirty(),
            NodeKind::Text(n) => n.clear_dirty(),
        }
    }
    
    fn preload(&self, center: i32, radius: i32, ctx: &ComputeContext) {
        match self {
            NodeKind::File(n) => n.preload(center, radius, ctx),
            NodeKind::Comp(n) => n.preload(center, radius, ctx),
            NodeKind::Camera(_) => {} // No preload for cameras
            NodeKind::Text(_) => {}   // No preload for static text
        }
    }
}

// Convenience From implementations
impl From<FileNode> for NodeKind {
    fn from(node: FileNode) -> Self {
        NodeKind::File(node)
    }
}

impl From<CompNode> for NodeKind {
    fn from(node: CompNode) -> Self {
        NodeKind::Comp(node)
    }
}

impl From<CameraNode> for NodeKind {
    fn from(node: CameraNode) -> Self {
        NodeKind::Camera(node)
    }
}

impl From<TextNode> for NodeKind {
    fn from(node: TextNode) -> Self {
        NodeKind::Text(node)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_node_kind_file() {
        let file = FileNode::new("test.*.exr".to_string(), 1, 100, 24.0);
        let kind: NodeKind = file.into();
        
        assert!(kind.is_file());
        assert!(!kind.is_comp());
        assert_eq!(kind.node_type(), "File");
    }
    
    #[test]
    fn test_node_kind_comp() {
        let comp = CompNode::new("Test Comp", 0, 100, 24.0);
        let kind: NodeKind = comp.into();
        
        assert!(!kind.is_file());
        assert!(kind.is_comp());
        assert_eq!(kind.node_type(), "Comp");
    }
    
    #[test]
    fn test_node_trait_delegation() {
        let file = FileNode::new("test.*.exr".to_string(), 1, 100, 24.0);
        let file_uuid = file.uuid();
        let kind: NodeKind = file.into();
        
        assert_eq!(kind.uuid(), file_uuid);
        assert!(kind.inputs().is_empty());
    }
}
