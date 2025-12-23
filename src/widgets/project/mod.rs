//! Project panel widget
//!
//! Unified list of Clips & Compositions with drag-and-drop support

mod project;
pub mod project_ui;
pub mod project_events;

pub use project::ProjectActions;
pub use project_ui::render;
pub use project_events::*;
