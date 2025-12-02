//! Node trait and NodeCore - base for all node types.
//!
//! NodeCore holds persistent (attrs) and transient (data) storage.
//! Node trait provides uniform access for composition pipeline.

use super::attrs::Attrs;
use super::frame::Frame;

/// Compute context passed to nodes during composition
pub struct ComputeContext<'a> {
    /// Current frame being computed
    pub frame: i32,
    /// Parent node's data (if any)
    pub parent_data: Option<&'a Attrs>,
}

impl<'a> ComputeContext<'a> {
    pub fn new(frame: i32) -> Self {
        Self {
            frame,
            parent_data: None,
        }
    }

    pub fn with_parent(frame: i32, parent_data: &'a Attrs) -> Self {
        Self {
            frame,
            parent_data: Some(parent_data),
        }
    }
}

/// Core storage for all node types.
/// - attrs: persistent attributes (serialized)
/// - data: transient runtime data (not serialized)
#[derive(Debug, Clone, Default)]
pub struct NodeCore {
    /// Persistent attributes (saved to project)
    pub attrs: Attrs,
    /// Transient runtime data (computed values, cache refs)
    #[allow(dead_code)]
    pub data: Attrs,
}

impl NodeCore {
    pub fn new() -> Self {
        Self {
            attrs: Attrs::new(),
            data: Attrs::new(),
        }
    }

    pub fn with_attrs(attrs: Attrs) -> Self {
        Self {
            attrs,
            data: Attrs::new(),
        }
    }
}

/// Base trait for all node types in the composition graph.
pub trait Node {
    /// Access persistent attributes
    fn attrs(&self) -> &Attrs;
    /// Access persistent attributes mutably
    fn attrs_mut(&mut self) -> &mut Attrs;
    /// Access transient runtime data
    fn data(&self) -> &Attrs;
    /// Access transient runtime data mutably
    fn data_mut(&mut self) -> &mut Attrs;
    /// Compute output frame for given context
    fn compute(&self, ctx: &ComputeContext) -> Option<Frame>;
}
