//! Attribute Editor widget - UI rendering

use crate::entities::{AttrValue, Attrs};
use eframe::egui::{self, Ui};

/// Render generic attributes editor
///
/// Displays all attributes with appropriate UI widgets for editing.
/// Supports: Str, Int, UInt, Float, Vec3, Vec4, Mat3, Mat4
pub fn render(ui: &mut Ui, attrs: &mut Attrs) {
    if attrs.is_empty() {
        ui.label("(no attributes)");
        return;
    }

    let attr_count = attrs.iter().count();
    let attr_len = attrs.len();
    debug_assert_eq!(attr_count, attr_len);
    ui.label(format!("Attributes: {}", attr_len));

    for (key, value) in attrs.iter_mut() {
        ui.horizontal(|ui| {
            // Attribute name (read-only label)
            ui.label(format!("{}:", key));

            // Attribute value editor (type-specific widget)
            match value {
                AttrValue::Bool(v) => {
                    ui.checkbox(v, "");
                }
                AttrValue::Str(s) => {
                    ui.text_edit_singleline(s);
                }
                AttrValue::Int(v) => {
                    ui.add(egui::DragValue::new(v).speed(1.0));
                }
                AttrValue::UInt(v) => {
                    let mut temp = *v as i32;
                    if ui
                        .add(
                            egui::DragValue::new(&mut temp)
                                .speed(1.0)
                                .range(0..=i32::MAX),
                        )
                        .changed()
                    {
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
        });
    }
}
