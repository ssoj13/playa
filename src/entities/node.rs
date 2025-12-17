//! Node trait - base interface for all node types in the project.
//!
//! Nodes are the building blocks of the compositing graph:
//! - FileNode: loads image sequences/video from disk
//! - CompNode: composites multiple layers
//!
//! Each node can compute a frame at given time, has attributes,
//! and participates in dirty tracking for efficient caching.
//!
//! ## Play Range Helpers
//!
//! For timeline bounds and work area, see [`NodeKind`](super::node_kind::NodeKind):
//! - `play_range(use_work_area)` → `(start, end)` frame range
//! - `bounds(use_trim, selection_only)` → content bounds
//! - `frame_count()` → total frames

use enum_dispatch::enum_dispatch;
use std::sync::Arc;
use uuid::Uuid;

use super::attrs::Attrs;
use super::frame::Frame;
use super::keys::{A_HEIGHT, A_SRC_LEN, A_WIDTH};
use crate::core::global_cache::GlobalFrameCache;
use crate::core::workers::Workers;

/// Context passed to node compute and preload functions.
/// Contains references to project resources needed for computation.
///
/// ## Why Arc<NodeKind> in media?
///
/// Workers need read access during compute (50-500ms), but UI needs write
/// access for playhead updates. Without Arc, workers block UI with read locks.
///
/// With Arc<NodeKind>:
/// - Workers take snapshot (clone HashMap of Arcs) in microseconds
/// - Lock released immediately, UI never blocked
/// - Compute uses owned snapshot, safe from concurrent mutation
pub struct ComputeContext<'a> {
    /// Global frame cache (Arc for worker thread access in preload)
    pub cache: &'a Arc<GlobalFrameCache>,
    /// Media pool for looking up source nodes.
    /// Values are Arc<NodeKind> for cheap cloning - workers snapshot this
    /// and release lock before expensive compute operations.
    pub media: &'a std::collections::HashMap<Uuid, Arc<super::node_kind::NodeKind>>,
    /// Media pool Arc for worker thread access in preload.
    /// Workers clone this, take snapshot of inner HashMap, then release lock.
    pub media_arc: Option<std::sync::Arc<std::sync::RwLock<std::collections::HashMap<Uuid, Arc<super::node_kind::NodeKind>>>>>,
    /// Worker pool for background loading (None during synchronous compute)
    pub workers: Option<&'a Workers>,
    /// Current epoch for cancelling stale preload requests
    pub epoch: u64,
}

/// Base trait for all node types.
/// Provides common interface for identification, attributes, and computation.
#[enum_dispatch]
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
    
    // --- Timeline/timing methods (for enum_dispatch unification) ---
    
    /// Play range: (start_frame, end_frame) for playback.
    /// Default uses attrs.layer_start()/layer_end() which respects in/trim/speed.
    fn play_range(&self, _use_work_area: bool) -> (i32, i32) {
        (self.attrs().layer_start(), self.attrs().layer_end())
    }
    
    /// Content bounds for zoom-to-fit. Default delegates to play_range.
    fn bounds(&self, use_trim: bool, _selection_only: bool) -> (i32, i32) {
        self.play_range(use_trim)
    }
    
    /// Total source frames (before speed/trim).
    fn frame_count(&self) -> i32 {
        self.attrs().get_i32(A_SRC_LEN).unwrap_or(100)
    }
    
    /// Dimensions (width, height). Default reads from attrs.
    fn dim(&self) -> (usize, usize) {
        let w = self.attrs().get_u32(A_WIDTH).unwrap_or(0) as usize;
        let h = self.attrs().get_u32(A_HEIGHT).unwrap_or(0) as usize;
        (w, h)
    }
}
