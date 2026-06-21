//! Attribute Editor — playa glue over the reusable `egui-attr-grid` widget.
//!
//! The generic `[label | value]` grid (sorted rows, resizable splitter, typed
//! editors, mixed-value dimming) now lives in `egui-attr-grid`. This module:
//! - converts playa's [`Attrs`] / [`AttrValue`] to/from the widget's flat model,
//! - keeps the layer **Effects** stack UI ([`render_effects`]), which is
//!   app-specific (playa `Effect` / `EffectType`).
//!
//! Change tracking is unchanged: [`render`] returns `bool`, [`render_with_mixed`]
//! fills a `(key, value)` vec. The caller propagates via
//! `Comp::set_child_attrs` / `Comp::emit_attrs_changed`.

use eframe::egui::{self, ComboBox, Pos2, Stroke, TextStyle, Ui};
use egui_attr_grid as ag;
use egui_extras::{Column, TableBuilder};
use playa_engine::entities::effects::{Effect, EffectType};
use playa_engine::entities::{AttrValue, Attrs};
use std::collections::HashSet;
use uuid::Uuid;

/// Persistent UI state for the Attributes panel.
#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
pub struct AttributesState {
    /// Splitter state for the main attribute grid (egui-attr-grid).
    #[serde(default)]
    pub grid: ag::AttrGridState,
    /// Label-column width for the effects sub-tables (still hand-rolled).
    #[serde(default = "default_effect_label_width")]
    pub name_column_width: f32,
    /// Saved position of the split between Project and Attributes panels (0..1).
    #[serde(default = "default_split_position")]
    pub project_attributes_split: f32,
}

fn default_split_position() -> f32 {
    0.6
}

fn default_effect_label_width() -> f32 {
    180.0
}

/// Render the attribute editor for a single object. Returns `true` if any
/// attribute changed (caller should emit change events).
pub fn render(ui: &mut Ui, attrs: &mut Attrs, state: &mut AttributesState, display_name: &str) -> bool {
    let mut changed = Vec::new();
    render_impl(ui, attrs, state, display_name, &HashSet::new(), &mut changed);
    !changed.is_empty()
}

/// Render with multi-selection support. `mixed_keys` render dimmed; `changed_out`
/// collects `(key, value)` for every attribute the user edited.
pub fn render_with_mixed(
    ui: &mut Ui,
    attrs: &mut Attrs,
    state: &mut AttributesState,
    display_name: &str,
    mixed_keys: &HashSet<String>,
    changed_out: &mut Vec<(String, AttrValue)>,
) {
    render_impl(ui, attrs, state, display_name, mixed_keys, changed_out);
}

fn render_impl(
    ui: &mut Ui,
    attrs: &mut Attrs,
    state: &mut AttributesState,
    display_name: &str,
    mixed_keys: &HashSet<String>,
    changed_out: &mut Vec<(String, AttrValue)>,
) {
    if attrs.is_empty() {
        ui.label("(no attributes)");
        return;
    }
    ui.label(format!("{display_name}: {} attrs", attrs.len()));

    // Build the widget's flat field list from the attrs + schema (order + UI
    // hints), then let egui-attr-grid render + report the edited rows.
    let schema = attrs.schema();
    let mut fields: Vec<ag::AttrField> = attrs
        .iter()
        .map(|(key, value)| {
            let (order, ui_options) = schema
                .and_then(|s| s.get(key))
                .map(|def| {
                    (
                        def.order,
                        def.ui_options.iter().map(|o| o.to_string()).collect::<Vec<_>>(),
                    )
                })
                .unwrap_or((999.0, Vec::new()));
            // READONLY provenance attrs render as a non-editable Label (the grid
            // never reports Label edits, and `from_widget` returns None for them,
            // so there is no write-back). Editable attrs keep their typed widget,
            // preserving the absorb→edit→encode round-trip.
            let widget_value = if attrs.is_readonly(key) {
                ag::AttrValue::Label(label_str(value))
            } else {
                to_widget(value)
            };
            ag::AttrField {
                key: key.clone(),
                value: widget_value,
                ui_options,
                order,
            }
        })
        .collect();

    let changed = ag::render_grid(ui, &mut fields, &mut state.grid, mixed_keys);
    for (key, widget_value) in changed {
        if let Some(value) = from_widget(&widget_value) {
            attrs.set(key.clone(), value.clone());
            changed_out.push((key, value));
        }
    }
}

/// Stringify an [`AttrValue`] for read-only Label display (READONLY attrs).
/// Scalars print their value; aggregates print a compact summary.
fn label_str(v: &AttrValue) -> String {
    match v {
        AttrValue::Bool(b) => b.to_string(),
        AttrValue::Str(s) => s.clone(),
        AttrValue::Int8(i) => i.to_string(),
        AttrValue::Int(i) => i.to_string(),
        AttrValue::Int64(i) => i.to_string(),
        AttrValue::UInt(u) => u.to_string(),
        AttrValue::Float(f) => f.to_string(),
        AttrValue::Vec3(a) => format!("{a:?}"),
        AttrValue::Vec4(a) => format!("{a:?}"),
        AttrValue::Mat3(m) => format!("{m:?}"),
        AttrValue::Mat4(m) => format!("{m:?}"),
        AttrValue::Uuid(u) => u.to_string(),
        AttrValue::List(items) => format!("[{} items]", items.len()),
        AttrValue::Map(m) => format!("Map: {} entries", m.len()),
        AttrValue::Set(s) => format!("Set: {} items", s.len()),
        AttrValue::Json(s) => format!("JSON: {} chars", s.len()),
    }
}

/// Convert a playa [`AttrValue`] into the widget's value model. Non-editable
/// kinds (uuid / map / set / json) become a read-only [`ag::AttrValue::Label`].
fn to_widget(v: &AttrValue) -> ag::AttrValue {
    match v {
        AttrValue::Bool(b) => ag::AttrValue::Bool(*b),
        AttrValue::Str(s) => ag::AttrValue::Str(s.clone()),
        AttrValue::Int8(i) => ag::AttrValue::Int8(*i),
        AttrValue::Int(i) => ag::AttrValue::Int(*i),
        AttrValue::Int64(i) => ag::AttrValue::Int64(*i),
        AttrValue::UInt(u) => ag::AttrValue::UInt(*u),
        AttrValue::Float(f) => ag::AttrValue::Float(*f),
        AttrValue::Vec3(a) => ag::AttrValue::Vec3(*a),
        AttrValue::Vec4(a) => ag::AttrValue::Vec4(*a),
        AttrValue::Mat3(m) => ag::AttrValue::Mat3(*m),
        AttrValue::Mat4(m) => ag::AttrValue::Mat4(*m),
        AttrValue::List(items) => ag::AttrValue::List(items.iter().map(to_widget).collect()),
        AttrValue::Uuid(u) => ag::AttrValue::Label(u.to_string()),
        AttrValue::Map(m) => ag::AttrValue::Label(format!("Map: {} entries", m.len())),
        AttrValue::Set(s) => ag::AttrValue::Label(format!("Set: {} items", s.len())),
        AttrValue::Json(s) => ag::AttrValue::Label(format!("JSON: {} chars", s.len())),
    }
}

/// Convert an edited widget value back to a playa [`AttrValue`]. Returns `None`
/// for read-only [`ag::AttrValue::Label`] (those are never editable, so never
/// appear in the change list) — and for a list containing one.
fn from_widget(v: &ag::AttrValue) -> Option<AttrValue> {
    Some(match v {
        ag::AttrValue::Bool(b) => AttrValue::Bool(*b),
        ag::AttrValue::Str(s) => AttrValue::Str(s.clone()),
        ag::AttrValue::Int8(i) => AttrValue::Int8(*i),
        ag::AttrValue::Int(i) => AttrValue::Int(*i),
        ag::AttrValue::Int64(i) => AttrValue::Int64(*i),
        ag::AttrValue::UInt(u) => AttrValue::UInt(*u),
        ag::AttrValue::Float(f) => AttrValue::Float(*f),
        ag::AttrValue::Vec3(a) => AttrValue::Vec3(*a),
        ag::AttrValue::Vec4(a) => AttrValue::Vec4(*a),
        ag::AttrValue::Mat3(m) => AttrValue::Mat3(*m),
        ag::AttrValue::Mat4(m) => AttrValue::Mat4(*m),
        ag::AttrValue::List(items) => {
            let converted: Option<Vec<AttrValue>> = items.iter().map(from_widget).collect();
            AttrValue::List(converted?)
        }
        ag::AttrValue::Label(_) => return None,
    })
}

// ============================================================================
// Effects UI (app-specific — playa Effect / EffectType)
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
                        if ui
                            .selectable_label(false, effect_type.display_name())
                            .clicked()
                        {
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
                    if ui
                        .small_button("✕")
                        .on_hover_text("Remove effect")
                        .clicked()
                    {
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
                render_effect_attrs(ui, effect, state, &mut actions);
            }

            // Separator between effects
            if idx < effects_count - 1 {
                ui.add_space(2.0);
            }
        });
    }

    actions
}

/// Render editable attributes for a single effect using a compact table.
fn render_effect_attrs(
    ui: &mut Ui,
    effect: &mut Effect,
    state: &mut AttributesState,
    actions: &mut Vec<EffectAction>,
) {
    let schema = effect.effect_type.schema();

    // Get attribute keys sorted by order
    let keys: Vec<String> = {
        let mut pairs: Vec<_> = effect
            .attrs
            .iter()
            .map(|(k, _)| (k.clone(), schema.get(&k).map(|d| d.order).unwrap_or(999.0)))
            .collect();
        pairs.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        pairs.into_iter().map(|(k, _)| k).collect()
    };

    if keys.is_empty() {
        return;
    }

    let row_height = ui
        .text_style_height(&TextStyle::Body)
        .max(ui.spacing().interact_size.y);

    let available_width = ui.available_width();
    let min_label = 100.0;
    let max_label = (available_width - 120.0).max(min_label);

    let table_top = ui.cursor().min;

    TableBuilder::new(ui)
        .id_salt(format!("fx_attrs_{}", effect.uuid))
        .striped(true)
        .column(
            Column::initial(state.name_column_width)
                .range(min_label..=max_label)
                .resizable(false),
        )
        .column(Column::remainder())
        .body(|mut body| {
            for key in &keys {
                if let Some(value) = effect.attrs.get_mut(key) {
                    body.row(row_height, |mut row| {
                        row.col(|ui| {
                            ui.label(key);
                        });
                        row.col(|ui| {
                            // Get UI hints from schema
                            let (min, max, speed) = schema
                                .get(key)
                                .map(|def| {
                                    let opts = def.ui_options;
                                    let min = opts
                                        .first()
                                        .and_then(|s| s.parse::<f64>().ok())
                                        .unwrap_or(0.0);
                                    let max = opts
                                        .get(1)
                                        .and_then(|s| s.parse::<f64>().ok())
                                        .unwrap_or(100.0);
                                    let speed = opts
                                        .get(2)
                                        .and_then(|s| s.parse::<f64>().ok())
                                        .unwrap_or(0.1);
                                    (min, max, speed)
                                })
                                .unwrap_or((0.0, 100.0, 0.1));

                            match value {
                                AttrValue::Float(v) => {
                                    let mut temp = *v;
                                    if ui
                                        .add(
                                            egui::DragValue::new(&mut temp)
                                                .speed(speed)
                                                .range(min..=max),
                                        )
                                        .changed()
                                    {
                                        actions.push(EffectAction::AttrChanged(
                                            effect.uuid,
                                            key.clone(),
                                            AttrValue::Float(temp),
                                        ));
                                    }
                                }
                                AttrValue::Int(v) => {
                                    let mut temp = *v;
                                    if ui
                                        .add(
                                            egui::DragValue::new(&mut temp)
                                                .speed(speed)
                                                .range(min as i32..=max as i32),
                                        )
                                        .changed()
                                    {
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
                    });
                }
            }
        });

    // Draw splitter line (aligned with the effect label column)
    let table_bottom = ui.cursor().min;
    let x = table_top.x + state.name_column_width;
    let stroke = Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color);
    ui.painter().line_segment(
        [Pos2::new(x, table_top.y), Pos2::new(x, table_bottom.y)],
        stroke,
    );
}
