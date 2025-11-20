//! Timeline widget - After Effects-style layer stack
//!
//! Vertical stack of layers with horizontal bars

mod timeline;
mod timeline_ui;
mod timeline_helpers;

pub use timeline::{
    TimelineConfig,
    TimelineState,
    GlobalDragState,
    TimelineAction,
};
pub use timeline_ui::render;
