//! NodeKind - enum wrapper for all node types.
//!
//! Provides unified interface for storing different node types
//! in Project.media HashMap.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::attrs::Attrs;
use super::comp_node::CompNode;
use super::file_node::FileNode;
use super::frame::Frame;
use super::node::{ComputeContext, Node};

/// Enum containing all possible node types.
/// Used in Project.media for unified storage.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NodeKind {
    File(FileNode),
    Comp(CompNode),
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
}

// Implement Node trait by delegating to inner node
impl Node for NodeKind {
    fn uuid(&self) -> Uuid {
        match self {
            NodeKind::File(n) => n.uuid(),
            NodeKind::Comp(n) => n.uuid(),
        }
    }
    
    fn name(&self) -> &str {
        match self {
            NodeKind::File(n) => n.name(),
            NodeKind::Comp(n) => n.name(),
        }
    }
    
    fn node_type(&self) -> &'static str {
        match self {
            NodeKind::File(n) => n.node_type(),
            NodeKind::Comp(n) => n.node_type(),
        }
    }
    
    fn attrs(&self) -> &Attrs {
        match self {
            NodeKind::File(n) => n.attrs(),
            NodeKind::Comp(n) => n.attrs(),
        }
    }
    
    fn attrs_mut(&mut self) -> &mut Attrs {
        match self {
            NodeKind::File(n) => n.attrs_mut(),
            NodeKind::Comp(n) => n.attrs_mut(),
        }
    }
    
    fn inputs(&self) -> Vec<Uuid> {
        match self {
            NodeKind::File(n) => n.inputs(),
            NodeKind::Comp(n) => n.inputs(),
        }
    }
    
    fn compute(&self, frame: i32, ctx: &ComputeContext) -> Option<Frame> {
        match self {
            NodeKind::File(n) => n.compute(frame, ctx),
            NodeKind::Comp(n) => n.compute(frame, ctx),
        }
    }
    
    fn is_dirty(&self) -> bool {
        match self {
            NodeKind::File(n) => n.is_dirty(),
            NodeKind::Comp(n) => n.is_dirty(),
        }
    }
    
    fn mark_dirty(&self) {
        match self {
            NodeKind::File(n) => n.mark_dirty(),
            NodeKind::Comp(n) => n.mark_dirty(),
        }
    }
    
    fn clear_dirty(&self) {
        match self {
            NodeKind::File(n) => n.clear_dirty(),
            NodeKind::Comp(n) => n.clear_dirty(),
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
