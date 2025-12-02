//! Timeline widget - After Effects-style layer stack.
//! Exposes state (`timeline.rs`) and UI renderers (`timeline_ui.rs`) used by
//! `ui.rs` timeline panel. Data flow: renderers emit `TimelineAction` via
//! dispatch closures → EventBus; helpers (`timeline_helpers.rs`) keep drawing
//! primitives co-located with UI code.

mod timeline;
mod timeline_helpers;
mod timeline_ui;
pub mod timeline_events;

pub use timeline::{
    GlobalDragState, TimelineActions, TimelineConfig, TimelineState, TimelineViewMode,
};
pub use timeline_ui::{render_canvas, render_outline, render_toolbar};
