//! NodeKind - enum wrapper for all node types.
//!
//! Provides unified interface for storing different node types
//! in Project.media HashMap.

use enum_dispatch::enum_dispatch;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use super::attrs::Attrs;
use super::camera_node::CameraNode;
use super::comp_node::CompNode;
use super::file_node::FileNode;
use super::frame::Frame;
use super::node::{ComputeContext, Node};
use super::ref_node::RefNode;
use super::text_node::TextNode;

/// Enum containing all possible node types.
/// Used in Project.media for unified storage.
#[enum_dispatch(Node)]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NodeKind {
    File(FileNode),
    Comp(CompNode),
    Camera(CameraNode),
    Text(TextNode),
    Ref(RefNode),
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

    /// Check if this is a reference node (utility — points at another
    /// node + channel selector).
    pub fn is_ref(&self) -> bool {
        matches!(self, NodeKind::Ref(_))
    }

    /// Check if this node type can be rendered as a layer.
    /// Returns false for control nodes (camera, light, null, audio, ref).
    pub fn is_renderable(&self) -> bool {
        match self {
            NodeKind::Camera(_) => false,
            NodeKind::Ref(_) => false,
            // Future: Light, Transform (null), Audio -> false
            _ => true,
        }
    }

    /// Check if listed in Project UI (false = hidden preview comp)
    pub fn is_listed(&self) -> bool {
        self.attrs().get_bool("listed").unwrap_or(true)
    }

    /// Add child layer (only works on CompNode)
    ///
    /// # initial_position parameter
    ///
    /// Optional starting position for the layer. Used for cameras which need
    /// Z=-1000 to be pulled back from the scene (at Z=0 they'd see nothing).
    /// Regular layers use None → default [0,0,0].
    pub fn add_child_layer(
        &mut self,
        source_uuid: Uuid,
        name: &str,
        start_frame: i32,
        duration: i32,
        insert_idx: Option<usize>,
        source_dim: (usize, usize),
        renderable: bool,
        initial_position: Option<[f32; 3]>,
    ) -> anyhow::Result<Uuid> {
        match self {
            NodeKind::Comp(comp) => comp.add_child_layer(
                source_uuid,
                name,
                start_frame,
                duration,
                insert_idx,
                source_dim,
                renderable,
                initial_position,
            ),
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

    /// Get as `RefNode` reference.
    pub fn as_ref_node(&self) -> Option<&RefNode> {
        match self {
            NodeKind::Ref(n) => Some(n),
            _ => None,
        }
    }

    /// Get as mutable `RefNode` reference.
    pub fn as_ref_node_mut(&mut self) -> Option<&mut RefNode> {
        match self {
            NodeKind::Ref(n) => Some(n),
            _ => None,
        }
    }

    /// Get file mask (only for FileNode)
    pub fn file_mask(&self) -> Option<String> {
        match self {
            NodeKind::File(n) => n.file_mask().map(|s| s.to_string()),
            _ => None,
        }
    }

    /// Set event emitter (only affects CompNode)
    pub fn set_event_emitter(&mut self, emitter: crate::core::event_bus::CompEventEmitter) {
        if let NodeKind::Comp(n) = self {
            n.set_event_emitter(emitter);
        }
    }
}
// Node trait impl and From<T> for NodeKind are auto-generated by enum_dispatch

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
