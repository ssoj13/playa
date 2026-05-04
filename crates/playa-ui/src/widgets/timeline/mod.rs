//! Timeline widget - After Effects-style layer stack.
//! Exposes state (`timeline.rs`) and UI renderers (`timeline_ui.rs`) used by
//! `ui.rs` timeline panel. Data flow: renderers emit `TimelineAction` via
//! dispatch closures → EventBus; helpers (`timeline_helpers.rs`) keep drawing
//! primitives co-located with UI code.
//!
//! [`GlobalDragState`](crate::widgets::dnd::GlobalDragState) is defined in [`super::dnd`] and
//! re-exported here for a stable import path alongside timeline types.

mod timeline;
pub mod timeline_events;
mod timeline_helpers;
mod timeline_ui;

pub use crate::widgets::dnd::GlobalDragState;
pub use timeline::{
    ClipboardLayer, TimelineActions, TimelineConfig, TimelineState, TimelineViewMode,
};
pub use timeline_ui::{render_canvas, render_outline, render_toolbar};
