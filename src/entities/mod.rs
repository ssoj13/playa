//! Entities module - core types with separation of business logic and GUI
//!
//! Each entity (Clip, Comp, Project) can have multiple UI representations:
//! - Project panel view (name, metadata)
//! - Timeline view (bars, handles)
//! - Attribute Editor view (all properties)

pub mod attrs;
pub mod clip;
pub mod comp;
pub mod frame;
pub mod loader;
pub mod loader_exr;
pub mod loader_video;
pub mod project;

pub use attrs::{Attrs, AttrValue};
pub use clip::Clip;
pub use comp::Comp;
pub use frame::Frame;
pub use loader::Loader;
pub use loader_video::VideoMetadata;
pub use project::Project;

use eframe::egui::{self, Ui, Response, Rect};

/// Render generic attributes editor
///
/// Displays all attributes with appropriate UI widgets for editing.
/// Supports: Str, Int, UInt, Float, Vec3, Vec4, Mat3, Mat4
pub fn render_attrs_editor(ui: &mut Ui, attrs: &mut Attrs) {
    if attrs.is_empty() {
        ui.label("(no attributes)");
        return;
    }

    // Collect keys to avoid borrow issues
    let keys: Vec<String> = attrs.iter().map(|(k, _)| k.clone()).collect();

    for key in keys {
        ui.horizontal(|ui| {
            // Attribute name (read-only label)
            ui.label(format!("{}:", key));

            // Attribute value editor (type-specific widget)
            if let Some(value) = attrs.get_mut(&key) {
                match value {
                    AttrValue::Str(s) => {
                        ui.text_edit_singleline(s);
                    }
                    AttrValue::Int(v) => {
                        ui.add(egui::DragValue::new(v).speed(1.0));
                    }
                    AttrValue::UInt(v) => {
                        let mut temp = *v as i32;
                        if ui.add(egui::DragValue::new(&mut temp).speed(1.0).range(0..=i32::MAX)).changed() {
                            *v = temp.max(0) as u32;
                        }
                    }
                    AttrValue::Float(v) => {
                        ui.add(egui::DragValue::new(v).speed(0.1));
                    }
                    AttrValue::Vec3(arr) => {
                        ui.label("X:");
                        ui.add(egui::DragValue::new(&mut arr[0]).speed(0.1));
                        ui.label("Y:");
                        ui.add(egui::DragValue::new(&mut arr[1]).speed(0.1));
                        ui.label("Z:");
                        ui.add(egui::DragValue::new(&mut arr[2]).speed(0.1));
                    }
                    AttrValue::Vec4(arr) => {
                        ui.label("X:");
                        ui.add(egui::DragValue::new(&mut arr[0]).speed(0.1));
                        ui.label("Y:");
                        ui.add(egui::DragValue::new(&mut arr[1]).speed(0.1));
                        ui.label("Z:");
                        ui.add(egui::DragValue::new(&mut arr[2]).speed(0.1));
                        ui.label("W:");
                        ui.add(egui::DragValue::new(&mut arr[3]).speed(0.1));
                    }
                    AttrValue::Mat3(_) => {
                        ui.label("(3x3 matrix - not editable)");
                    }
                    AttrValue::Mat4(_) => {
                        ui.label("(4x4 matrix - not editable)");
                    }
                }
            }
        });
    }
}

/// Widget for project panel - shows entity in the project list
///
/// Displays:
/// - Name/identifier
/// - Metadata (resolution, frame count, fps)
/// - Icon/thumbnail (optional)
pub trait ProjectUI {
    /// Render entity in project panel
    ///
    /// Returns Response for interaction handling (clicks, drag-and-drop)
    fn project_ui(&self, ui: &mut Ui) -> Response;
}

/// Widget for timeline - shows entity as a bar/clip
///
/// Displays:
/// - Horizontal bar representing time range
/// - Handles for trimming/moving
/// - Visual state (selected, muted, etc.)
pub trait TimelineUI {
    /// Render entity in timeline
    ///
    /// # Arguments
    /// * `ui` - egui UI context
    /// * `bar_rect` - Rectangle where the bar should be drawn
    /// * `current_frame` - Current playhead position (for highlighting)
    ///
    /// Returns Response for interaction handling
    fn timeline_ui(&self, ui: &mut Ui, bar_rect: Rect, current_frame: usize) -> Response;
}

/// Widget for Attribute Editor - shows all entity properties
///
/// Similar to Maya's Attribute Editor or After Effects' Effect Controls.
/// Displays:
/// - All editable attributes
/// - Grouped by category
/// - Interactive widgets (sliders, text fields, checkboxes)
pub trait AttributeEditorUI {
    /// Render entity attributes in attribute editor panel
    ///
    /// This is a mutable method because editing attributes modifies the entity.
    fn ae_ui(&mut self, ui: &mut Ui);
}

/// Optional: Node editor representation (for future node-based workflow)
///
/// Displays entity as a node in a graph editor (like Houdini or Nuke).
/// This is for future expansion - not required now.
pub trait NodeUI {
    /// Render entity as a node in node editor
    ///
    /// # Arguments
    /// * `ui` - egui UI context
    /// * `node_rect` - Rectangle where the node should be drawn
    ///
    /// Returns Response for interaction handling
    fn node_ui(&self, ui: &mut Ui, node_rect: Rect) -> Response;
}
