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
use crate::entities::effects::{Effect, EffectType};
use eframe::egui::{self, ComboBox, Pos2, Rect, Sense, Stroke, TextStyle, Ui};
use egui_extras::{Column, TableBuilder};
use std::collections::HashSet;
use uuid::Uuid;

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

    // Get schema for UI hints and ordering
    let schema = attrs.schema();

    // Sort attributes by order field from schema (lower = higher in list)
    let keys: Vec<String> = if let Some(schema) = schema {
        let mut pairs: Vec<_> = attrs.iter()
            .map(|(k, _)| (k.clone(), schema.get(k).map(|d| d.order).unwrap_or(999.0)))
            .collect();
        pairs.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        pairs.into_iter().map(|(k, _)| k).collect()
    } else {
        let mut keys: Vec<String> = attrs.iter().map(|(k, _)| k.clone()).collect();
        keys.sort();
        keys
    };

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
                // Get UI options from schema (combobox values or slider range)
                let ui_options = schema
                    .and_then(|s| s.get(&key))
                    .map(|def| def.ui_options)
                    .unwrap_or(&[]);

                body.row(row_height, |mut row| {
                    row.col(|ui| {
                        ui.label(format!("{}:", key));
                    });
                    row.col(|ui| {
                        let is_mixed = mixed_keys.contains(&key);
                        if collect_changes {
                            let before = value.clone();
                            let changed = render_value_editor(ui, &key, value, is_mixed, ui_options);
                            if changed && &before != value {
                                changed_out.push((key.clone(), value.clone()));
                            }
                        } else {
                            let _ = render_value_editor(ui, &key, value, is_mixed, ui_options);
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

/// Render value editor widget based on type and UI options from schema.
/// - String with ui_options -> combobox
/// - Float with ui_options ["min", "max", "step"] -> slider
/// - Otherwise -> default widget for type
fn render_value_editor(
    ui: &mut Ui,
    key: &str,
    value: &mut AttrValue,
    mixed: bool,
    ui_options: &[&str],
) -> bool {
    let mut changed = false;
    let weak = ui.visuals().weak_text_color();
    let mut scope_changed = false;

    // egui DragValue has built-in Shift support (slow mode)
    // No custom speed_mult needed - egui handles it internally

    ui.scope(|ui| {
        if mixed {
            ui.visuals_mut().override_text_color = Some(weak);
        }

        match value {
            // String with options -> combobox
            AttrValue::Str(current) if !ui_options.is_empty() => {
                let mut selected = current.clone();
                ComboBox::from_id_salt(format!("attr_{}", key))
                    .selected_text(&selected)
                    .show_ui(ui, |ui| {
                        for opt in ui_options {
                            ui.selectable_value(&mut selected, opt.to_string(), *opt);
                        }
                    });
                if &selected != current {
                    *current = selected;
                    scope_changed = true;
                }
            }

            // Float with options -> slider with range [min, max, step]
            AttrValue::Float(v) if ui_options.len() >= 2 => {
                let min: f32 = ui_options[0].parse().unwrap_or(0.0);
                let max: f32 = ui_options[1].parse().unwrap_or(1.0);
                let step: f64 = ui_options.get(2)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.01);
                scope_changed |= ui.add(egui::Slider::new(v, min..=max).step_by(step)).changed();
            }

            // Default widgets by type
            AttrValue::Bool(v) => {
                scope_changed |= ui.checkbox(v, "").changed();
            }
            AttrValue::Str(s) => {
                scope_changed |= ui.text_edit_singleline(s).changed();
            }
            AttrValue::Int(v) => {
                scope_changed |= ui.add(egui::DragValue::new(v).speed(1.0)).changed();
            }
            AttrValue::UInt(v) => {
                let mut temp = *v as i32;
                if ui.add(egui::DragValue::new(&mut temp).speed(1.0).range(0..=i32::MAX)).changed() {
                    *v = temp.max(0) as u32;
                    scope_changed = true;
                }
            }
            AttrValue::Float(v) => {
                scope_changed |= ui.add(egui::DragValue::new(v).speed(0.1)).changed();
            }
            AttrValue::Vec3(arr) => {
                ui.horizontal(|ui| {
                    ui.label("X:");
                    scope_changed |= ui.add(egui::DragValue::new(&mut arr[0]).speed(0.1)).changed();
                    ui.label("Y:");
                    scope_changed |= ui.add(egui::DragValue::new(&mut arr[1]).speed(0.1)).changed();
                    ui.label("Z:");
                    scope_changed |= ui.add(egui::DragValue::new(&mut arr[2]).speed(0.1)).changed();
                });
            }
            AttrValue::Vec4(arr) => {
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
            AttrValue::Mat3(_) => {
                ui.label("(3x3 matrix - not editable)");
            }
            AttrValue::Mat4(_) => {
                ui.label("(4x4 matrix - not editable)");
            }
            AttrValue::Json(s) => {
                ui.label(format!("JSON: {} chars", s.len()));
            }
            AttrValue::Int8(v) => {
                let mut temp = *v as i32;
                if ui.add(egui::DragValue::new(&mut temp).speed(1.0).range(-128..=127)).changed() {
                    *v = temp.clamp(-128, 127) as i8;
                    scope_changed = true;
                }
            }
            AttrValue::Uuid(u) => {
                ui.label(format!("{}", u));
            }
            AttrValue::List(items) => {
                ui.label(format!("List: {} items", items.len()));
            }
            AttrValue::Map(entries) => {
                ui.label(format!("Map: {} entries", entries.len()));
            }
            AttrValue::Set(items) => {
                ui.label(format!("Set: {} items", items.len()));
            }
        }
    });
    changed |= scope_changed;
    changed
}

// ============================================================================
// Effects UI
// ============================================================================

/// Actions that can be performed on effects (returned from render_effects).
#[derive(Debug, Clone)]
pub enum EffectAction {
    /// Add new effect of given type
    Add(EffectType),
    /// Remove effect by UUID
    Remove(Uuid),
    /// Toggle effect enabled state
    ToggleEnabled(Uuid),
    /// Toggle effect collapsed state
    ToggleCollapsed(Uuid),
    /// Effect attribute changed (effect_uuid, key, value)
    AttrChanged(Uuid, String, AttrValue),
    /// Move effect up in stack (lower index = applied first)
    MoveUp(Uuid),
    /// Move effect down in stack
    MoveDown(Uuid),
}

/// Render effects section for a layer.
///
/// Returns list of actions to apply (add, remove, toggle, attr change).
/// Caller should handle these actions and update the layer's effects Vec.
pub fn render_effects(
    ui: &mut Ui,
    effects: &mut Vec<Effect>,
    state: &mut AttributesState,
) -> Vec<EffectAction> {
    let mut actions: Vec<EffectAction> = Vec::new();
    
    ui.add_space(8.0);
    ui.separator();
    
    // Header with Add button
    ui.horizontal(|ui| {
        ui.strong("Effects");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Add Effect dropdown
            let mut selected_type: Option<EffectType> = None;
            ComboBox::from_id_salt("add_effect")
                .selected_text("+")
                .width(100.0)
                .show_ui(ui, |ui| {
                    for effect_type in EffectType::all() {
                        if ui.selectable_label(false, effect_type.display_name()).clicked() {
                            selected_type = Some(effect_type.clone());
                        }
                    }
                });
            if let Some(etype) = selected_type {
                actions.push(EffectAction::Add(etype));
            }
        });
    });
    
    if effects.is_empty() {
        ui.label("No effects");
        return actions;
    }
    
    // Render each effect
    let effects_count = effects.len();
    for (idx, effect) in effects.iter_mut().enumerate() {
        ui.push_id(effect.uuid, |ui| {
            ui.horizontal(|ui| {
                // Collapse toggle
                let collapse_icon = if effect.collapsed { "▸" } else { "▾" };
                if ui.small_button(collapse_icon).clicked() {
                    actions.push(EffectAction::ToggleCollapsed(effect.uuid));
                }
                
                // Enable checkbox
                let mut enabled = effect.enabled;
                if ui.checkbox(&mut enabled, "").changed() {
                    actions.push(EffectAction::ToggleEnabled(effect.uuid));
                }
                
                // Effect name (dimmed if disabled)
                let name_text = egui::RichText::new(effect.name());
                let name_text = if effect.enabled {
                    name_text
                } else {
                    name_text.weak()
                };
                ui.label(name_text);
                
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Delete button
                    if ui.small_button("✕").on_hover_text("Remove effect").clicked() {
                        actions.push(EffectAction::Remove(effect.uuid));
                    }
                    
                    // Reorder buttons
                    ui.add_enabled_ui(idx < effects_count - 1, |ui| {
                        if ui.small_button("▼").on_hover_text("Move down").clicked() {
                            actions.push(EffectAction::MoveDown(effect.uuid));
                        }
                    });
                    ui.add_enabled_ui(idx > 0, |ui| {
                        if ui.small_button("▲").on_hover_text("Move up").clicked() {
                            actions.push(EffectAction::MoveUp(effect.uuid));
                        }
                    });
                });
            });
            
            // Effect parameters (if not collapsed)
            if !effect.collapsed {
                ui.indent(effect.uuid, |ui| {
                    render_effect_attrs(ui, effect, state, &mut actions);
                });
            }
            
            // Separator between effects
            if idx < effects_count - 1 {
                ui.add_space(2.0);
            }
        });
    }
    
    actions
}

/// Render editable attributes for a single effect.
fn render_effect_attrs(
    ui: &mut Ui,
    effect: &mut Effect,
    _state: &mut AttributesState,
    actions: &mut Vec<EffectAction>,
) {
    let schema = effect.effect_type.schema();
    
    // Get attribute keys sorted by order
    let keys: Vec<String> = {
        let mut pairs: Vec<_> = effect.attrs.iter()
            .map(|(k, _)| (k.clone(), schema.get(&k).map(|d| d.order).unwrap_or(999.0)))
            .collect();
        pairs.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        pairs.into_iter().map(|(k, _)| k).collect()
    };
    
    for key in keys {
        if let Some(value) = effect.attrs.get_mut(&key) {
            ui.horizontal(|ui| {
                ui.label(&key);
                
                // Get UI hints from schema (ui_options: ["min", "max", "step"])
                let (min, max, speed) = schema.get(&key)
                    .map(|def| {
                        let opts = def.ui_options;
                        let min = opts.get(0).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
                        let max = opts.get(1).and_then(|s| s.parse::<f64>().ok()).unwrap_or(100.0);
                        let speed = opts.get(2).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.1);
                        (min, max, speed)
                    })
                    .unwrap_or((0.0, 100.0, 0.1));
                
                match value {
                    AttrValue::Float(v) => {
                        let mut temp = *v;
                        if ui.add(
                            egui::DragValue::new(&mut temp)
                                .speed(speed)
                                .range(min..=max)
                        ).changed() {
                            actions.push(EffectAction::AttrChanged(
                                effect.uuid,
                                key.clone(),
                                AttrValue::Float(temp),
                            ));
                        }
                    }
                    AttrValue::Int(v) => {
                        let mut temp = *v;
                        if ui.add(
                            egui::DragValue::new(&mut temp)
                                .speed(speed)
                                .range(min as i32..=max as i32)
                        ).changed() {
                            actions.push(EffectAction::AttrChanged(
                                effect.uuid,
                                key.clone(),
                                AttrValue::Int(temp),
                            ));
                        }
                    }
                    _ => {
                        ui.label(format!("{:?}", value));
                    }
                }
            });
        }
    }
}
