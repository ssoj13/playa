//! Attribute Editor widget - UI rendering
//!
//! Provides a generic property editor for [`Attrs`] objects. Used in:
//! - Attribute Editor panel (right dock) for layer and comp attributes
//! - Potentially other places that need to edit key-value attribute sets
//!
//! # Change Tracking
//! The editor tracks which attributes were modified during the frame:
//! - [`render`] returns `bool` - true if any attribute changed
//! - [`render_with_mixed`] populates `changed_out` vec with (key, value) pairs
//!
//! The caller is responsible for propagating changes via [`Comp::set_child_attrs`]
//! or [`Comp::emit_attrs_changed`] to trigger cache invalidation.
//!
//! # Usage in main.rs
//! ```ignore
//! // Single layer: render_with_mixed tracks changes, apply via set_child_attrs
//! let mut changed = Vec::new();
//! render_with_mixed(ui, &mut temp_attrs, state, name, &HashSet::new(), &mut changed);
//! if !changed.is_empty() {
//!     comp.set_child_attrs(&layer_uuid, &values);  // auto-emits event
//! }
//!
//! // Comp attrs: render returns bool, emit manually if changed
//! if render(ui, &mut comp.attrs, state, name) {
//!     comp.emit_attrs_changed();
//! }
//! ```

use crate::entities::{AttrValue, Attrs};
use eframe::egui::{self, ComboBox, Pos2, Rect, Sense, Stroke, TextStyle, Ui};
use egui_extras::{Column, TableBuilder};
use std::collections::HashSet;

/// Persistent UI state for the Attributes panel.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct AttributesState {
    pub name_column_width: f32,
    /// Saved position of the split between Project and Attributes panels (0.0-1.0)
    #[serde(default = "default_split_position")]
    pub project_attributes_split: f32,
}

fn default_split_position() -> f32 {
    0.6
}

impl Default for AttributesState {
    fn default() -> Self {
        Self {
            name_column_width: 180.0,
            project_attributes_split: 0.6,
        }
    }
}

/// Render generic attributes editor for a single object.
///
/// Displays all attributes with appropriate UI widgets for editing.
/// Supports: Str, Int, UInt, Float, Vec3, Vec4, Mat3, Mat4
///
/// Returns `true` if any attribute was modified by user interaction.
/// The caller should emit change events when this returns true.
pub fn render(ui: &mut Ui, attrs: &mut Attrs, state: &mut AttributesState, display_name: &str) -> bool {
    let mut changed = Vec::new();
    render_impl(ui, attrs, state, display_name, &HashSet::new(), &mut changed, true);
    !changed.is_empty()
}

/// Render attribute editor with support for mixed values (multi-selection).
///
/// # Arguments
/// - `mixed_keys` - attribute keys that have differing values across selected objects
///   (rendered with dimmed values to indicate mixed state)
/// - `changed_out` - populated with `(key, value)` pairs for attributes modified by user
///
/// Use this for multi-layer selection where you need to know exactly which
/// attributes changed to apply them to all selected layers.
pub fn render_with_mixed(
    ui: &mut Ui,
    attrs: &mut Attrs,
    state: &mut AttributesState,
    display_name: &str,
    mixed_keys: &HashSet<String>,
    changed_out: &mut Vec<(String, AttrValue)>,
) {
    render_impl(ui, attrs, state, display_name, mixed_keys, changed_out, true);
}

fn render_impl(
    ui: &mut Ui,
    attrs: &mut Attrs,
    state: &mut AttributesState,
    display_name: &str,
    mixed_keys: &HashSet<String>,
    changed_out: &mut Vec<(String, AttrValue)>,
    collect_changes: bool,
) {
    if attrs.is_empty() {
        ui.label("(no attributes)");
        return;
    }

    let attr_count = attrs.iter().count();
    let attr_len = attrs.len();
    debug_assert_eq!(attr_count, attr_len);
    ui.label(format!("{display_name}: {attr_len} attrs"));

    let row_height = ui
        .text_style_height(&TextStyle::Body)
        .max(ui.spacing().interact_size.y);

    // Clamp width bounds
    let available_width = ui.available_width();
    let min_label = 100.0;
    let max_label = (available_width - 120.0).max(min_label);
    state.name_column_width = state.name_column_width.clamp(min_label, max_label);

    // Track top to draw splitter across table height later
    let table_top = ui.cursor().min;

    // Sort attributes by name for stable order
    let mut keys: Vec<String> = attrs.iter().map(|(k, _)| k.clone()).collect();
    keys.sort();

    TableBuilder::new(ui)
        .id_salt("attrs_table")
        .striped(true)
        .column(
            Column::initial(state.name_column_width)
                .range(min_label..=max_label)
                .resizable(false),
        )
        .column(Column::remainder())
        .header(row_height, |mut header| {
            header.col(|ui| {
                ui.strong("Attribute");
            });
            header.col(|ui| {
                ui.strong("Value");
            });
        })
        .body(|mut body| {
            for key in keys {
                let Some(value) = attrs.get_mut(&key) else {
                    continue;
                };
                body.row(row_height, |mut row| {
                    row.col(|ui| {
                        ui.label(format!("{}:", key));
                    });
                    row.col(|ui| {
                        let is_mixed = mixed_keys.contains(&key);
                        if collect_changes {
                            let before = value.clone();
                            let changed = render_value_editor(ui, &key, value, is_mixed);
                            if changed && &before != value {
                                changed_out.push((key.clone(), value.clone()));
                            }
                        } else {
                            let _ = render_value_editor(ui, &key, value, is_mixed);
                        }
                    });
                });
            }
        });

    // Interactive splitter spanning header + body
    let table_bottom = ui.cursor().min;
    let x = table_top.x + state.name_column_width;
    let splitter_rect = Rect::from_min_max(
        Pos2::new(x - 4.0, table_top.y),
        Pos2::new(x + 4.0, table_bottom.y),
    );
    let splitter_id = ui.make_persistent_id("attrs_splitter_drag");
    let response = ui.interact(splitter_rect, splitter_id, Sense::click_and_drag());
    if response.dragged() {
        state.name_column_width =
            (state.name_column_width + response.drag_delta().x).clamp(min_label, max_label);
    }
    let stroke = if response.hovered() || response.dragged() {
        Stroke::new(2.0, ui.visuals().strong_text_color())
    } else {
        Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color)
    };
    ui.painter().line_segment(
        [Pos2::new(x, table_top.y), Pos2::new(x, table_bottom.y)],
        stroke,
    );
}

fn render_value_editor(ui: &mut Ui, key: &str, value: &mut AttrValue, mixed: bool) -> bool {
    let mut changed = false;
    let weak = ui.visuals().weak_text_color();
    let mut scope_changed = false;
    ui.scope(|ui| {
        if mixed {
            ui.visuals_mut().override_text_color = Some(weak);
        }
    match (key, value) {
        // Known enum-like string attributes rendered as dropdowns
        ("blend_mode" | "layer_mode", AttrValue::Str(current)) => {
            let mut selected = current.clone();
            ComboBox::from_id_salt(format!("attr_enum_{}", key))
                .selected_text(&selected)
                .show_ui(ui, |ui| {
                    for mode in ["normal", "screen", "add", "subtract", "multiply", "divide", "difference"] {
                        ui.selectable_value(&mut selected, mode.to_string(), mode);
                    }
                });
            if &selected != current {
                *current = selected;
                scope_changed = true;
            }
        }

        // Fallbacks
        (_, AttrValue::Bool(v)) => {
            scope_changed |= ui.checkbox(v, "").changed();
        }
        (_, AttrValue::Str(s)) => {
            scope_changed |= ui.text_edit_singleline(s).changed();
        }
        (_, AttrValue::Int(v)) => {
            scope_changed |= ui.add(egui::DragValue::new(v).speed(1.0)).changed();
        }
        (_, AttrValue::UInt(v)) => {
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
                scope_changed = true;
            }
        }
        (_, AttrValue::Float(v)) => {
            scope_changed |= ui.add(egui::DragValue::new(v).speed(0.1)).changed();
        }
        (_, AttrValue::Vec3(arr)) => {
            ui.horizontal(|ui| {
                ui.label("X:");
                scope_changed |= ui.add(egui::DragValue::new(&mut arr[0]).speed(0.1)).changed();
                ui.label("Y:");
                scope_changed |= ui.add(egui::DragValue::new(&mut arr[1]).speed(0.1)).changed();
                ui.label("Z:");
                scope_changed |= ui.add(egui::DragValue::new(&mut arr[2]).speed(0.1)).changed();
            });
        }
        (_, AttrValue::Vec4(arr)) => {
            ui.horizontal(|ui| {
                ui.label("X:");
                scope_changed |= ui.add(egui::DragValue::new(&mut arr[0]).speed(0.1)).changed();
                ui.label("Y:");
                scope_changed |= ui.add(egui::DragValue::new(&mut arr[1]).speed(0.1)).changed();
                ui.label("Z:");
                scope_changed |= ui.add(egui::DragValue::new(&mut arr[2]).speed(0.1)).changed();
                ui.label("W:");
                scope_changed |= ui.add(egui::DragValue::new(&mut arr[3]).speed(0.1)).changed();
            });
        }
        (_, AttrValue::Mat3(_)) => {
            ui.label("(3x3 matrix - not editable)");
        }
        (_, AttrValue::Mat4(_)) => {
            ui.label("(4x4 matrix - not editable)");
        }
        (_, AttrValue::Json(s)) => {
            ui.label(format!("JSON: {} chars", s.len()));
        }
        (_, AttrValue::Int8(v)) => {
            let mut temp = *v as i32;
            if ui.add(egui::DragValue::new(&mut temp).speed(1.0).range(-128..=127)).changed() {
                *v = temp.clamp(-128, 127) as i8;
                scope_changed = true;
            }
        }
        (_, AttrValue::Uuid(u)) => {
            ui.label(format!("{}", u));
        }
        (_, AttrValue::List(items)) => {
            ui.label(format!("List: {} items", items.len()));
        }
    }
    });
    changed |= scope_changed;
    changed
}
