//! Node trait for uniform access to composable entities.
//!
//! Comp implements Node for polymorphic access to attrs/data.
//! Direct field access (comp.attrs) is preferred for simplicity.

#![allow(unused)] // Infrastructure for future polymorphic use

use super::attrs::Attrs;
use super::frame::Frame;

/// Compute context passed to nodes during composition
pub struct ComputeContext<'a> {
    pub frame: i32,
    pub parent_data: Option<&'a Attrs>,
}

impl<'a> ComputeContext<'a> {
    pub fn new(frame: i32) -> Self {
        Self { frame, parent_data: None }
    }
}

/// Base trait for composable entities.
pub trait Node {
    fn attrs(&self) -> &Attrs;
    fn attrs_mut(&mut self) -> &mut Attrs;
    fn data(&self) -> &Attrs;
    fn data_mut(&mut self) -> &mut Attrs;
    fn compute(&self, ctx: &ComputeContext) -> Option<Frame>;
}
