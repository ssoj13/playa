//! Timeline widget - After Effects-style layer stack
//!
//! Vertical stack of layers with horizontal bars

mod timeline;
mod timeline_helpers;
mod timeline_ui;

pub use timeline::{GlobalDragState, TimelineAction, TimelineConfig, TimelineState};
pub use timeline_ui::render;
