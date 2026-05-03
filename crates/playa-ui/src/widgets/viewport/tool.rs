//! Viewport tool modes — delegated to [`playa_events::viewport_tool`].

pub use playa_events::viewport_tool::{SetToolEvent, ToolMode};

use playa_engine::entities::Project;

/// Current tool stored on project prefs.
pub fn current_tool(project: &Project) -> ToolMode {
    ToolMode::from_str(&project.tool())
}
