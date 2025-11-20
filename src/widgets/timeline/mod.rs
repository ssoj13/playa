//! Timeline widget - After Effects-style layer stack
//!
//! Vertical stack of layers with horizontal bars

mod timeline;
mod timeline_ui;

pub use timeline::{
    TimelineConfig,
    TimelineState,
    GlobalDragState, // deprecated
    TimelineAction,
};
pub use timeline_ui::render_timeline;
