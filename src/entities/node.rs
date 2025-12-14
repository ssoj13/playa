//! Node trait - base interface for all node types in the project.
//!
//! Nodes are the building blocks of the compositing graph:
//! - FileNode: loads image sequences/video from disk
//! - CompNode: composites multiple layers
//!
//! Each node can compute a frame at given time, has attributes,
//! and participates in dirty tracking for efficient caching.

use std::sync::Arc;
use uuid::Uuid;

use super::attrs::Attrs;
use super::frame::Frame;
use crate::core::global_cache::GlobalFrameCache;
use crate::core::workers::Workers;

/// Context passed to node compute and preload functions.
/// Contains references to project resources needed for computation.
pub struct ComputeContext<'a> {
    /// Global frame cache (Arc for worker thread access in preload)
    pub cache: &'a Arc<GlobalFrameCache>,
    /// Media pool for looking up source nodes
    pub media: &'a std::collections::HashMap<Uuid, super::node_kind::NodeKind>,
    /// Media pool Arc for worker thread access in preload
    pub media_arc: Option<std::sync::Arc<std::sync::RwLock<std::collections::HashMap<Uuid, super::node_kind::NodeKind>>>>,
    /// Worker pool for background loading (None during synchronous compute)
    pub workers: Option<&'a Workers>,
    /// Current epoch for cancelling stale preload requests
    pub epoch: u64,
}

/// Base trait for all node types.
/// Provides common interface for identification, attributes, and computation.
pub trait Node: Send + Sync {
    /// Unique identifier for this node
    fn uuid(&self) -> Uuid;
    
    /// Display name of the node
    fn name(&self) -> &str;
    
    /// Type identifier string ("File", "Comp", etc.)
    fn node_type(&self) -> &'static str;
    
    /// Access to node's persistent attributes
    fn attrs(&self) -> &Attrs;
    
    /// Mutable access to node's attributes
    fn attrs_mut(&mut self) -> &mut Attrs;
    
    /// Source nodes that this node depends on (via layers).
    /// Empty for leaf nodes like FileNode.
    fn inputs(&self) -> Vec<Uuid>;
    
    /// Compute output frame at given frame index.
    /// Result should be cached in global_cache[uuid][frame].
    /// Returns None if computation fails or no frame available.
    fn compute(&self, frame: i32, ctx: &ComputeContext) -> Option<Frame>;
    
    /// Check if node needs recomputation (attrs changed)
    fn is_dirty(&self) -> bool;
    
    /// Mark node as needing recomputation
    fn mark_dirty(&self);
    
    /// Clear dirty flag after successful computation
    fn clear_dirty(&self);
    
    /// Preload frames around center position for background loading.
    /// Default implementation is no-op (for nodes without preload support).
    /// FileNode/CompNode override this to enqueue frame loading via workers.
    /// `radius` - max number of frames to preload around center
    fn preload(&self, _center: i32, _radius: i32, _ctx: &ComputeContext) {
        // Default no-op
    }
    
    // --- Convenience methods with default implementations ---
    
    /// Get attribute value by key
    fn get_attr(&self, key: &str) -> Option<&super::attrs::AttrValue> {
        self.attrs().get(key)
    }
    
    /// Set attribute value
    fn set_attr(&mut self, key: &str, value: super::attrs::AttrValue) {
        self.attrs_mut().set(key, value);
    }
    
    /// Get i32 attribute
    fn get_i32(&self, key: &str) -> Option<i32> {
        self.attrs().get_i32(key)
    }
    
    /// Get f32 attribute
    fn get_float(&self, key: &str) -> Option<f32> {
        self.attrs().get_float(key)
    }
    
    /// Get string attribute
    fn get_str(&self, key: &str) -> Option<&str> {
        self.attrs().get_str(key)
    }
    
    /// Get uuid attribute
    fn get_uuid_attr(&self, key: &str) -> Option<Uuid> {
        self.attrs().get_uuid(key)
    }
}
