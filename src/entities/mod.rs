//! Entities module - core types with separation of business logic and GUI
//!
//! Each entity (Clip, Comp, Project) can have multiple UI representations:
//! - Project panel view (name, metadata)
//! - Timeline view (bars, handles)
//! - Attribute Editor view (all properties)

pub mod attrs;
pub mod comp;
pub mod compositor;
pub mod frame;
pub mod loader;
pub mod loader_video;
pub mod project;

pub use attrs::{Attrs, AttrValue};
pub use comp::Comp;
pub use compositor::CompositorType;
pub use frame::Frame;
pub use project::Project;

use eframe::egui::{Ui, Response, Rect};

/// Widget for project panel - shows entity in the project list
pub trait ProjectUI {
    /// Render entity in project panel
    fn project_ui(&self, ui: &mut Ui) -> Response;
}

/// Widget for timeline - shows entity as a bar/clip
pub trait TimelineUI {
    /// Render entity in timeline
    fn timeline_ui(&self, ui: &mut Ui, bar_rect: Rect, current_frame: i32) -> Response;
}

/// Widget for Attribute Editor - shows all entity properties
pub trait AttributeEditorUI {
    /// Render entity attributes in attribute editor panel
    fn ae_ui(&mut self, ui: &mut Ui);
}

/// Optional: Node editor representation (for future node-based workflow)
pub trait NodeUI {
    /// Render entity as a node in node editor
    fn node_ui(&self, ui: &mut Ui, node_rect: Rect) -> Response;
}
