//! Typed events and the shared [`bus::EventBus`] — the only cross-layer
//! messaging path (widgets, dialogs, engine adapters, app shell).

#![allow(clippy::module_inception)]

pub mod bus;
pub mod comp;
pub mod layout;
pub mod node_editor;
pub mod player;
pub mod prefs;
pub mod project_media;
pub mod timeline;
pub mod viewport;
pub mod viewport_tool;

pub use bus::{BoxedEvent, CompEventEmitter, Event, EventBus, EventEmitter, downcast_event};
pub use prefs::{CompositorBackend, CompositorBackendChangedEvent, GizmoPrefs};
pub use viewport_tool::{SetToolEvent, ToolMode};
