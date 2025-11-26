//! After Effects-style timeline - UI rendering
//!
//! Each layer is displayed as a row showing:
//! - Layer name / clip name
//! - Start..End range as horizontal bar
//! - Visual indication of current_frame (playhead)
//! Consumed by: `ui::render_timeline_panel`. Emits `AppEvent` through
//! dispatch closures to EventBus, driven by shared `TimelineState` from
//! `timeline.rs` and helper routines in `timeline_helpers.rs`. Data flow:
//! egui input → dispatch(AppEvent) → EventBus → Project/Comp mutations.

use super::timeline_helpers::{
    compute_all_layer_rows, detect_layer_tool, draw_drop_preview, draw_frame_ruler,
    draw_load_indicator, frame_to_screen_x, hash_color, row_to_y, screen_x_to_frame,
};
use super::{GlobalDragState, TimelineConfig, TimelineState};
use crate::entities::{Comp, frame::FrameStatus};
use crate::events::AppEvent;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Ui, Vec2};
use egui_dnd::dnd;

fn compute_layer_selection(
    current: &[String],
    anchor: Option<String>,
    clicked_uuid: String,
    clicked_idx: usize,
    modifiers: egui::Modifiers,
    all_children: &[String],
) -> (Vec<String>, Option<String>) {
    if modifiers.shift {
        let anchor_uuid = anchor.as_ref().unwrap_or(&clicked_uuid);
        let anchor_idx = all_children.iter().position(|u| u == anchor_uuid).unwrap_or(clicked_idx);
        let (lo, hi) = if anchor_idx <= clicked_idx {
            (anchor_idx, clicked_idx)
        } else {
            (clicked_idx, anchor_idx)
        };
        let selection: Vec<String> = all_children[lo..=hi].to_vec();
        (selection, Some(anchor_uuid.clone()))
    } else if modifiers.ctrl {
        let mut selection: Vec<String> = current.to_vec();
        if let Some(pos) = selection.iter().position(|v| v == &clicked_uuid) {
            selection.remove(pos);
        } else {
            selection.push(clicked_uuid.clone());
        }
        (selection, anchor)
    } else {
        (vec![clicked_uuid.clone()], Some(clicked_uuid))
    }
}

/// Render timeline toolbar (transport controls, zoom, snap)
pub fn render_toolbar(ui: &mut Ui, state: &mut TimelineState, mut dispatch: impl FnMut(AppEvent)) {
    ui.horizontal(|ui| {
        if ui.button("↞").on_hover_text("To Start").clicked() {
            dispatch(AppEvent::JumpToStart);
        }

        let play_icon = "▶"; // Placeholder - real icon controlled by playback status
        if ui.button(play_icon).on_hover_text("Play/Pause").clicked() {
            dispatch(AppEvent::TogglePlayPause);
        }

        if ui.button("■").on_hover_text("Stop").clicked() {
            dispatch(AppEvent::Stop);
        }

        if ui.button("↠").on_hover_text("To End").clicked() {
            dispatch(AppEvent::JumpToEnd);
        }

        ui.separator();

        // Zoom controls emit actions; actual zoom applies in canvas via events
        ui.label("Zoom:");
        let zoom_response = ui.add_sized(
            egui::Vec2::new(200.0, 20.0), // 2x longer slider
            egui::Slider::new(&mut state.zoom, 0.1..=20.0).fixed_decimals(2),
        );
        if zoom_response.changed() {
            dispatch(AppEvent::TimelineZoomChanged(state.zoom));
        }
        if ui
            .button("Reset")
            .on_hover_text("Reset Zoom to 1.0")
            .clicked()
        {
            state.zoom = 1.0;
            dispatch(AppEvent::TimelineZoomChanged(1.0));
        }
        if ui
            .button("Fit")
            .on_hover_text("Fit all clips to view")
            .clicked()
        {
            dispatch(AppEvent::TimelineFitAll(state.last_canvas_width));
        }

        if ui.checkbox(&mut state.snap_enabled, "Snap").changed() {
            dispatch(AppEvent::TimelineSnapChanged(state.snap_enabled));
        }
        if ui.checkbox(&mut state.lock_work_area, "Lock").changed() {
            dispatch(AppEvent::TimelineLockWorkAreaChanged(state.lock_work_area));
        }
    });
}

/// Render left outline: layer list only (no toolbar)
pub fn render_outline(
    ui: &mut Ui,
    comp_uuid: &str,
    comp: &mut Comp,
    config: &TimelineConfig,
    _state: &mut TimelineState,
    view_mode: super::TimelineViewMode,
    mut dispatch: impl FnMut(AppEvent),
) {
    let comp_id = comp_uuid.to_string();

    // Render layer list with DnD inside a ScrollArea to avoid growing the parent panel.
    let mut child_order: Vec<usize> = (0..comp.children.len()).collect();
    let dnd_response = egui::ScrollArea::vertical()
        .id_salt("timeline_layers_scroll") // share scroll with canvas
        .max_height(ui.available_height())
        .show(ui, |ui| {
            ui.vertical(|ui| {
                // Match the top padding of the timeline canvas (ruler + optional status bar + spacing)
                // Ruler: 20.0, Status strip: 6.0 if present, spacer: 4.0
                let status_bar_height = comp
                    .cache_frame_statuses()
                    .as_ref()
                    .map(|_| 6.0)
                    .unwrap_or(0.0);
                ui.add_space(20.0 + status_bar_height + 4.0);

                dnd(ui, "timeline_child_names_outline").show_vec(
                    &mut child_order,
                    |ui, child_idx, handle, _state| {
                        let idx = *child_idx;
                        let child_uuid = &comp.children[idx];
                        let attrs = comp.children_attrs.get(child_uuid);

                        // In Split mode, use full available width (outline is in separate panel)
                        let row_width = if matches!(view_mode, super::TimelineViewMode::Split) {
                            ui.available_width()
                        } else {
                            config.name_column_width
                        };
                        let (row_rect, response) = ui.allocate_exact_size(
                            Vec2::new(row_width, config.layer_height),
                            Sense::click(),
                        );
                        let mut row_ui = ui.new_child(
                            egui::UiBuilder::new()
                                .max_rect(row_rect)
                                .layout(egui::Layout::left_to_right(egui::Align::Center))
                                .id_salt(egui::Id::new("outline_row").with(idx)),
                        );
                        row_ui.spacing_mut().item_spacing = egui::vec2(6.0, 0.0);
                        row_ui.set_min_height(config.layer_height);

                        handle.ui(&mut row_ui, |ui| {
                            ui.label("≡");
                        });

                        let mut visible = attrs.and_then(|a| a.get_bool("visible")).unwrap_or(true);
                        let mut opacity = attrs.and_then(|a| a.get_float("opacity")).unwrap_or(1.0);
                        let prev_blend = attrs
                            .and_then(|a| a.get_str("blend_mode"))
                            .unwrap_or("normal")
                            .to_string();
                        let mut blend = prev_blend.clone();
                        let mut speed = attrs.and_then(|a| a.get_float("speed")).unwrap_or(1.0);
                        let mut dirty = false;

                        if row_ui.checkbox(&mut visible, "").changed() {
                            dirty = true;
                        }

                        let child_name = comp
                            .children_attrs
                            .get(child_uuid)
                            .and_then(|attrs| attrs.get_str("name"))
                            .unwrap_or(child_uuid.as_str());
                        row_ui.label(child_name);

                        if row_ui
                            .add(
                                egui::Slider::new(&mut opacity, 0.0..=1.0)
                                    .show_value(false)
                                    .smallest_positive(0.01)
                                    .text(""),
                            )
                            .changed()
                        {
                            dirty = true;
                        }

                        egui::ComboBox::from_id_salt(format!("blend_outline_{}", child_uuid))
                            .width(80.0)
                            .selected_text(blend.clone())
                            .show_ui(&mut row_ui, |ui| {
                                for mode in
                                    ["normal", "screen", "add", "subtract", "multiply", "divide"]
                                {
                                    ui.selectable_value(&mut blend, mode.to_string(), mode);
                                }
                            });
                        if blend != prev_blend {
                            dirty = true;
                        }

                        if row_ui
                            .add(
                                egui::DragValue::new(&mut speed)
                                    .speed(0.1)
                                    .range(0.01..=8.0),
                            )
                            .changed()
                        {
                            dirty = true;
                        }

                        if dirty {
                            if let Some(attrs_mut) = comp.children_attrs.get_mut(child_uuid) {
                                attrs_mut.set("visible", crate::entities::AttrValue::Bool(visible));
                                attrs_mut
                                    .set("opacity", crate::entities::AttrValue::Float(opacity));
                                attrs_mut.set("blend_mode", crate::entities::AttrValue::Str(blend));
                                attrs_mut.set("speed", crate::entities::AttrValue::Float(speed));
                                // attrs.set() automatically marks as dirty
                            }
                        }

                        if response.clicked() {
                            let modifiers = ui.input(|i| i.modifiers);
                            let clicked_uuid = child_uuid.clone();
                            let (selection, anchor) = compute_layer_selection(
                                &comp.layer_selection,
                                comp.layer_selection_anchor.clone(),
                                clicked_uuid,
                                idx,
                                modifiers,
                                &comp.children,
                            );
                            dispatch(AppEvent::CompSelectionChanged {
                                comp_uuid: comp_id.clone(),
                                selection,
                                anchor,
                            });
                        }
                    },
                )
            })
            .inner
        })
        .inner;

    if let Some(update) = dnd_response.final_update() {
        dispatch(AppEvent::ReorderLayer {
            comp_uuid: comp_id.clone(),
            from_idx: update.from,
            to_idx: update.to,
        });
    }
}

/// Render After Effects-style timeline (right canvas)
pub fn render_canvas(
    ui: &mut Ui,
    comp_uuid: &str,
    comp: &mut Comp,
    config: &TimelineConfig,
    state: &mut TimelineState,
    view_mode: super::TimelineViewMode,
    mut dispatch: impl FnMut(AppEvent),
) -> super::timeline::TimelineActions {
    // Save canvas width for Fit button calculation
    state.last_canvas_width = ui.available_width();

    let comp_id = comp_uuid.to_string();
    // Calculate dimensions - timeline should show from 0 to end (not start to end)
    // This allows negative starts and ensures ruler shows full range
    let comp_start = comp.start();
    let comp_end = comp.end();
    let total_frames = (comp_end + 1).max(100); // From frame 0 to end (inclusive), minimum 100

    log::debug!(
        "Comp '{}': start={}, end={}, total_frames={}",
        comp.name(),
        comp_start,
        comp_end,
        total_frames
    );

    // In Split mode, use full width (outline is in separate panel)
    let available_for_timeline = if matches!(view_mode, super::TimelineViewMode::Split) {
        ui.available_width()
    } else {
        ui.available_width() - config.name_column_width
    };
    let timeline_width =
        (total_frames as f32 * config.pixels_per_frame * state.zoom).max(available_for_timeline);
    // Ensure non-zero height so DnD/drop zone works even for empty comps
    let total_height = (comp.children.len().max(1) as f32) * config.layer_height;

    let ruler_width =
        (total_frames as f32 * config.pixels_per_frame * state.zoom).max(ui.available_width());

    log::debug!(
        "ruler_width={}, timeline_width={}, available_width={}",
        ruler_width,
        timeline_width,
        ui.available_width()
    );
    let status_strip = comp.cache_frame_statuses();
    let status_bar_height = status_strip.as_ref().map(|_| 6.0).unwrap_or(0.0);

    // Options + time ruler row (always visible)
    let mut ruler_rect: Option<Rect> = None;
    let mut timeline_rect_global: Option<Rect> = None;
    let ruler_height = 20.0;
    let mut timeline_hovered = false; // Track hover state for input routing
    let tab_rect = ui.max_rect(); // Full tab rect for hover detection

    // Draw ruler with proper layout sync
    ui.horizontal(|ui| {
        // Add left spacer only in non-Split modes (outline column alignment)
        if !matches!(view_mode, super::TimelineViewMode::Split) {
            ui.allocate_exact_size(
                Vec2::new(config.name_column_width, ruler_height),
                Sense::hover(),
            );
        }

        // Ruler (no ScrollArea; pan/zoom handled via state.pan_offset/state.zoom)
        let (frame_opt, rect) =
            draw_frame_ruler(ui, comp, config, state, ruler_width, total_frames);
        ruler_rect = Some(rect);
        if let Some(frame) = frame_opt {
            dispatch(AppEvent::SetFrame(frame));
        }

        // Load indicator - shows cache status for each frame
        draw_load_indicator(ui, comp, config, state, ruler_width);

        // Middle-drag pan on ruler - initialize only, processing is in main loop
        if rect.contains(ui.ctx().pointer_hover_pos().unwrap_or(Pos2::ZERO)) {
            if ui
                .ctx()
                .input(|i| i.pointer.button_down(egui::PointerButton::Middle))
                && state.drag_state.is_none()
            {
                if let Some(pos) = ui.ctx().pointer_hover_pos() {
                    state.drag_state = Some(GlobalDragState::TimelinePan {
                        drag_start_pos: pos,
                        initial_pan_offset: state.pan_offset,
                    });
                }
            }
        }
    });

    if let Some(statuses) = &status_strip {
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ruler_width, status_bar_height), Sense::hover());
        draw_status_strip(ui, rect, statuses, comp_start, total_frames);
    }

    ui.add_space(4.0);

    // Layers area with vertical scroll (but horizontal pan via state.pan_offset)
    // ScrollArea is needed here because layers can extend beyond visible area vertically
    egui::ScrollArea::vertical()
        .id_salt("timeline_layers_scroll")
        // Constrain ScrollArea to visible space so the parent panel doesn't grow
        .max_height(ui.available_height())
        .show(ui, |ui| {
        ui.push_id("timeline_layers", |ui| {
            // Create temporary child order (layers displayed in original order from comp.children)
            let child_order: Vec<usize> = (0..comp.children.len()).collect();

            // Compute layout for all layers once (single source of truth)
            let layer_rows = compute_all_layer_rows(comp, &child_order);

            // Timeline bars - horizontal pan via state.pan_offset, vertical scroll via ScrollArea.
            // Use allocate_painter so ScrollArea knows the full vertical extent (all rows) and
            // applies clipping/scrolling correctly even when many layers are added.
            let (timeline_response, painter) = ui.allocate_painter(
                Vec2::new(timeline_width, total_height),
                Sense::click_and_drag(),
            );
            let timeline_rect = timeline_response.rect;

        // Get interaction response for click/drag (ui.interact doesn't show hover highlight)
        timeline_rect_global = Some(timeline_rect);
        timeline_hovered = timeline_response.hovered();

        // If mouse is over ruler or canvas rects, mark timeline hovered (hotkeys context)
        if let Some(pos) = ui.ctx().pointer_hover_pos() {
            if ruler_rect.map(|r| r.contains(pos)).unwrap_or(false)
                || timeline_rect.contains(pos)
            {
                timeline_hovered = true;
            }
        }

        // Middle-drag pan on canvas - initialize only if not already dragging
        if timeline_response.hovered() && state.drag_state.is_none() {
            if ui.ctx().input(|i| i.pointer.button_down(egui::PointerButton::Middle)) {
                if let Some(pos) = ui.ctx().pointer_hover_pos() {
                    state.drag_state = Some(GlobalDragState::TimelinePan {
                        drag_start_pos: pos,
                        initial_pan_offset: state.pan_offset,
                    });
                }
            }
        }

        // Scroll wheel horizontal pan
        let scroll_delta = ui.ctx().input(|i| i.smooth_scroll_delta);
        if scroll_delta.x.abs() > 0.0 {
            let delta_frames = scroll_delta.x / (config.pixels_per_frame * state.zoom);
            dispatch(AppEvent::TimelinePanChanged(state.pan_offset - delta_frames));
        }

        // Draw layers (egui automatically clips to visible area inside ScrollArea)
                        // Draw child bars using precomputed layout
                        for (_display_idx, &original_idx) in child_order.iter().enumerate() {
                            let idx = original_idx;
                            let child_uuid = &comp.children[idx];

                            // Get child start/end from attrs (now supports negative values)
                            let attrs = comp.children_attrs.get(child_uuid);
                            let child_start = attrs.and_then(|a| Some(a.get_i32("start").unwrap_or(0))).unwrap_or(0);
                            let child_end = attrs.and_then(|a| Some(a.get_i32("end").unwrap_or(0))).unwrap_or(0);

                            // Get precomputed row from layout
                            let row = layer_rows.get(&idx).copied().unwrap_or(0);
                            let child_y = row_to_y(row, config, timeline_rect);

                            let child_rect = Rect::from_min_size(
                                Pos2::new(timeline_rect.min.x, child_y),
                                Vec2::new(timeline_width, config.layer_height),
                            );

                          // Child background (alternating colors)
                          let bg_color = if idx % 2 == 0 {
                              Color32::from_gray(30)
                          } else {
                              Color32::from_gray(35)
                          };
                          painter.rect_filled(child_rect, 0.0, bg_color);
                            let play_start = attrs
                                .and_then(|a| Some(a.get_i32("play_start").unwrap_or(child_start)))
                                .unwrap_or(child_start);
                            let play_end = attrs
                                .and_then(|a| Some(a.get_i32("play_end").unwrap_or(child_end)))
                                .unwrap_or(child_end);
                            let is_visible = attrs.and_then(|a| a.get_bool("visible")).unwrap_or(true);

                            // Calculate layer geometry (deduplicated)
                            let geom = super::timeline::LayerGeom::calc(
                                child_start, child_end, play_start, play_end,
                                child_y, timeline_rect, config, state
                            );

                            // Child bar color (use hash of child_uuid for stable color)
                            let base_color = if is_visible {
                                hash_color(child_uuid)
                            } else {
                                Color32::from_gray(70)
                            };
                            let is_selected = comp.layer_selection.contains(child_uuid);
                            let gray_color = if is_selected {
                                // Slightly brighter grey with a blue tint when selected
                                Color32::from_rgba_unmultiplied(110, 140, 190, 130)
                            } else {
                                Color32::from_rgba_unmultiplied(80, 80, 80, 100)
                            };

                            painter.rect_filled(geom.full_bar_rect, 4.0, gray_color);

                            // Draw visible (trimmed) area with full color on top
                            if let Some(visible_bar_rect) = geom.visible_bar_rect {
                                painter.rect_filled(visible_bar_rect, 4.0, base_color);
                            }

                              // Draw outline around full bar (thicker and colored when selected)
                              let stroke_color = if is_selected {
                                  Color32::from_rgb(180, 230, 255)
                              } else {
                                  Color32::from_gray(150)
                              };
                              let stroke_width = if is_selected { 2.0 } else { 1.0 };
                              painter.rect_stroke(
                                  geom.full_bar_rect,
                                  4.0,
                                  egui::Stroke::new(stroke_width, stroke_color),
                                  egui::epaint::StrokeKind::Middle,
                              );
                        }

                        // Handle child bar interactions using proper response system
                        // We need to do this in a second pass after drawing to ensure responses are on top
                        for (_display_idx, &original_idx) in child_order.iter().enumerate() {
                            let idx = original_idx;
                            let child_uuid = &comp.children[idx];

                            // Get child attrs (now supports negative values)
                            let attrs = comp.children_attrs.get(child_uuid);
                            let child_start = attrs.and_then(|a| Some(a.get_i32("start").unwrap_or(0))).unwrap_or(0);
                            let child_end = attrs.and_then(|a| Some(a.get_i32("end").unwrap_or(0))).unwrap_or(0);
                            let play_start = attrs
                                .and_then(|a| Some(a.get_i32("play_start").unwrap_or(child_start)))
                                .unwrap_or(child_start);
                            let play_end = attrs
                                .and_then(|a| Some(a.get_i32("play_end").unwrap_or(child_end)))
                                .unwrap_or(child_end);

                            // Get precomputed row from layout
                            let row = layer_rows.get(&idx).copied().unwrap_or(0);
                            let child_y = row_to_y(row, config, timeline_rect);

                            // Calculate layer geometry (deduplicated)
                            let geom = super::timeline::LayerGeom::calc(
                                child_start, child_end, play_start, play_end,
                                child_y, timeline_rect, config, state
                            );

                            // Manual tool detection: use current visible bar bounds for handles/move
                            let edge_threshold = 8.0;
                            if let Some(hover_pos) = ui.ctx().input(|i| i.pointer.hover_pos()) {
                                if state.drag_state.is_none() {
                                    let handle_rect = geom.visible_bar_rect.unwrap_or(geom.full_bar_rect);

                                    if handle_rect.contains(hover_pos) {
                                        if let Some(tool) =
                                            detect_layer_tool(hover_pos, handle_rect, edge_threshold)
                                        {
                                            ui.ctx().set_cursor_icon(tool.cursor());

                                            // On mouse press, create appropriate drag state
                                            if ui.ctx().input(|i| i.pointer.primary_pressed()) {
                                                log::debug!(
                                                    "[TIMELINE] Creating drag state: {:?} for layer {}",
                                                    tool, idx
                                                );
                                                // Ensure selection switches to dragged layer if it wasn't selected
                                                if let Some(clicked_uuid) = comp.children.get(idx) {
                                                    let modifiers = ui.ctx().input(|i| i.modifiers);
                                                    let multi = modifiers.ctrl || modifiers.shift || modifiers.command;
                                                    if !multi && !comp.layer_selection.contains(clicked_uuid) {
                                                        dispatch(AppEvent::CompSelectionChanged {
                                                            comp_uuid: comp_id.clone(),
                                                            selection: vec![clicked_uuid.clone()],
                                                            anchor: Some(clicked_uuid.clone()),
                                                        });
                                                    }
                                                }
                                                if let Some(child_attrs) = attrs {
                                                    state.drag_state =
                                                        Some(tool.to_drag_state(idx, child_attrs, hover_pos));
                                                    log::debug!(
                                                        "[TIMELINE] Drag state created successfully"
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // Helper: find display index for a physical layer index
                        let physical_to_display = |physical_idx: usize| -> Option<usize> {
                            child_order.iter().position(|&idx| idx == physical_idx)
                        };

                        // Process active drag operations
                        if let Some(drag) = &state.drag_state.clone() {
                            if let Some(current_pos) = ui.ctx().input(|i| i.pointer.hover_pos()) {
                                match drag {
                                    GlobalDragState::TimelinePan { drag_start_pos, initial_pan_offset } => {
                                        let delta_x = current_pos.x - drag_start_pos.x;
                                        let delta_frames = delta_x / (config.pixels_per_frame * state.zoom);
                                        let new_pan = initial_pan_offset - delta_frames;

                                        // Update state directly to avoid frame delay
                                        state.pan_offset = new_pan;
                                        dispatch(AppEvent::TimelinePanChanged(new_pan));

                                        if ui.ctx().input(|i| i.pointer.any_released()) {
                                            state.drag_state = None;
                                        }
                                    }
                                    GlobalDragState::MovingLayer { layer_idx, initial_start, drag_start_x, drag_start_y, .. } => {
                                        let delta_x = current_pos.x - drag_start_x;
                                        let delta_y = current_pos.y - drag_start_y;
                                        let delta_frames = (delta_x / (config.pixels_per_frame * state.zoom)).round() as i32;
                                        let new_start = *initial_start as i32 + delta_frames;  // Allow negative values

                                        // Determine target child index from vertical position
                                        // Calculate from display position, then convert to physical
                                        let current_display_idx = physical_to_display(*layer_idx).unwrap_or(*layer_idx);
                                        let delta_children = (delta_y / config.layer_height).round() as i32;
                                        let target_display_idx = (current_display_idx as i32 + delta_children).max(0).min(comp.children.len() as i32 - 1) as usize;
                                        let target_child = child_order.get(target_display_idx).copied().unwrap_or(*layer_idx);

                                        // Visual feedback: draw ghost bars for all selected (or just dragged) layers
                                        let dragged_uuid = comp.children.get(*layer_idx).cloned().unwrap_or_default();
                                        let selection = if comp.layer_selection.contains(&dragged_uuid) {
                                            comp.layer_selection.clone()
                                        } else {
                                            vec![dragged_uuid.clone()]
                                        };

                                        for child_uuid in selection {
                                            if let Some(attrs) = comp.children_attrs.get(&child_uuid) {
                                                let idx_sel = comp.uuid_to_idx(&child_uuid).unwrap_or(0);
                                                let current_row = layer_rows.get(&idx_sel).copied().unwrap_or(idx_sel);
                                                let target_row = (current_row as i32 + delta_children)
                                                    .clamp(0, comp.children.len().saturating_sub(1) as i32)
                                                    as usize;

                                                let ghost_child_y = row_to_y(target_row, config, timeline_rect);
                                                let duration = (attrs.get_i32("end").unwrap_or(0)
                                                    - attrs.get_i32("start").unwrap_or(0)
                                                    + 1)
                                                    .max(1);

                                                // Apply same delta to maintain relative offsets
                                                let child_start = attrs.get_i32("start").unwrap_or(0);
                                                let ghost_start = child_start + delta_frames;

                                                draw_drop_preview(
                                                    &painter,
                                                    ghost_start,
                                                    ghost_child_y,
                                                    duration,
                                                    timeline_rect,
                                                    config,
                                                    state,
                                                );
                                            }
                                        }

                                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);

                                        // On release, commit both horizontal and vertical movements
                                        if ui.ctx().input(|i| i.pointer.any_released()) {
                                            let has_horizontal_move = new_start != *initial_start;
                                            let has_vertical_move = target_child != *layer_idx;

                                            log::debug!("[TIMELINE] Layer drag released: h_move={}, v_move={}, layer_idx={}, new_start={}, new_idx={}",
                                                has_horizontal_move, has_vertical_move, layer_idx, new_start, target_child);

                                            if has_horizontal_move || has_vertical_move {
                                                dispatch(AppEvent::MoveAndReorderLayer {
                                                    comp_uuid: comp_id.clone(),
                                                    layer_idx: *layer_idx,
                                                    new_start,
                                                    new_idx: target_child,
                                                });
                                                log::debug!("[TIMELINE] Emitting MoveAndReorderLayer action");
                                            }
                                            state.drag_state = None;
                                        }
                                    }
                                    GlobalDragState::AdjustPlayStart { layer_idx, initial_play_start, drag_start_x } => {
                                        let delta_x = current_pos.x - drag_start_x;
                                        let delta_frames = (delta_x / (config.pixels_per_frame * state.zoom)).round() as i32;
                                        let new_play_start = *initial_play_start + delta_frames;

                                        // Visual feedback: draw ghost play range preview
                                        if *layer_idx < comp.children.len() {
                                            let child_uuid = &comp.children[*layer_idx];
                                            if let Some(attrs) = comp.children_attrs.get(child_uuid) {
                                                // Use actual row for Y positioning
                                                let target_row = layer_rows
                                                    .get(layer_idx)
                                                    .copied()
                                                    .unwrap_or_else(|| {
                                                        physical_to_display(*layer_idx).unwrap_or(*layer_idx)
                                                    });
                                                let layer_y = row_to_y(target_row, config, timeline_rect);
                                                let visual_start = new_play_start as f32;
                                                let layer_end = attrs.get_i32("end").unwrap_or(0) as f32;
                                                let ghost_x_start = frame_to_screen_x(visual_start, timeline_rect.min.x, config, state);
                                                let ghost_x_end = frame_to_screen_x(layer_end, timeline_rect.min.x, config, state);

                                                let ghost_rect = Rect::from_min_max(
                                                    Pos2::new(ghost_x_start, layer_y + 4.0),
                                                    Pos2::new(ghost_x_end, layer_y + config.layer_height - 4.0),
                                                );
                                                painter.rect_stroke(
                                                    ghost_rect,
                                                    4.0,
                                                    egui::Stroke::new(2.0, Color32::from_rgba_unmultiplied(100, 220, 255, 200)),
                                                    egui::epaint::StrokeKind::Middle,
                                                );
                                            }
                                        }

                                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);

                                        // On release, commit the play start adjustment
                                        if ui.ctx().input(|i| i.pointer.any_released()) {
                                            dispatch(AppEvent::SetLayerPlayStart {
                                                comp_uuid: comp_id.clone(),
                                                layer_idx: *layer_idx,
                                                new_play_start,
                                            });
                                            state.drag_state = None;
                                        }
                                    }
                                    GlobalDragState::AdjustPlayEnd { layer_idx, initial_play_end, drag_start_x } => {
                                        let delta_x = current_pos.x - drag_start_x;
                                        let delta_frames = (delta_x / (config.pixels_per_frame * state.zoom)).round() as i32;
                                        // play_end is ABSOLUTE source frame, so drag right = increase
                                        let new_play_end = *initial_play_end + delta_frames;

                                        // Visual feedback: draw ghost play range preview
                                        if *layer_idx < comp.children.len() {
                                            let child_uuid = &comp.children[*layer_idx];
                                            if let Some(attrs) = comp.children_attrs.get(child_uuid) {
                                                // Use actual row for Y positioning
                                                let target_row = layer_rows
                                                    .get(layer_idx)
                                                    .copied()
                                                    .unwrap_or_else(|| {
                                                        physical_to_display(*layer_idx).unwrap_or(*layer_idx)
                                                    });
                                                let layer_y = row_to_y(target_row, config, timeline_rect);
                                                let play_start = attrs.get_i32("play_start").unwrap_or(attrs.get_i32("start").unwrap_or(0));
                                                let visual_start = play_start as f32;
                                                let visual_end = new_play_end as f32;
                                                let ghost_x_start = frame_to_screen_x(visual_start, timeline_rect.min.x, config, state);
                                                let ghost_x_end = frame_to_screen_x(visual_end, timeline_rect.min.x, config, state);

                                                let ghost_rect = Rect::from_min_max(
                                                    Pos2::new(ghost_x_start, layer_y + 4.0),
                                                    Pos2::new(ghost_x_end, layer_y + config.layer_height - 4.0),
                                                );
                                                painter.rect_stroke(
                                                    ghost_rect,
                                                    4.0,
                                                    egui::Stroke::new(2.0, Color32::from_rgba_unmultiplied(100, 220, 255, 200)),
                                                    egui::epaint::StrokeKind::Middle,
                                                );
                                            }
                                        }

                                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);

                                        // On release, commit the play end adjustment
                                        if ui.ctx().input(|i| i.pointer.any_released()) {
                                            dispatch(AppEvent::SetLayerPlayEnd {
                                                comp_uuid: comp_id.clone(),
                                                layer_idx: *layer_idx,
                                                new_play_end,
                                            });
                                            state.drag_state = None;
                                        }
                                    }
                                    // Other drag states are handled elsewhere (ProjectItem, TimelineScrub)
                                    _ => {}
                                }
                            }

                            // Cancel drag on escape
                            if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
                                state.drag_state = None;
                            }
                        }

                        // Draw work area overlay (darken regions outside play_range)
                        let (play_start, play_end) = comp.play_range(true);
                        let comp_start = comp.start();
                        let comp_end = comp.end();

                        // Darken region before work area start
                        if play_start > comp_start {
                            let start_x = frame_to_screen_x(comp_start as f32, timeline_rect.min.x, config, state);
                            let end_x = frame_to_screen_x(play_start as f32, timeline_rect.min.x, config, state);
                            let overlay_rect = Rect::from_min_max(
                                Pos2::new(start_x, timeline_rect.min.y),
                                Pos2::new(end_x, timeline_rect.max.y),
                            );
                            painter.rect_filled(overlay_rect, 0.0, Color32::from_rgba_unmultiplied(0, 0, 0, 51));
                        }

                        // Darken region after work area end
                        if play_end < comp_end {
                            let start_x = frame_to_screen_x((play_end + 1) as f32, timeline_rect.min.x, config, state);
                            let end_x = frame_to_screen_x((comp_end + 1) as f32, timeline_rect.min.x, config, state);
                            let overlay_rect = Rect::from_min_max(
                                Pos2::new(start_x, timeline_rect.min.y),
                                Pos2::new(end_x, timeline_rect.max.y),
                            );
                            painter.rect_filled(overlay_rect, 0.0, Color32::from_rgba_unmultiplied(0, 0, 0, 51));
                        }

                        // Check for drag'n'drop from Project Window using global drag state
                        let global_drag: Option<GlobalDragState> = ui.ctx().data(|data| {
                            data.get_temp(egui::Id::new("global_drag_state"))
                        });

                        if let Some(GlobalDragState::ProjectItem { source_uuid, duration, .. }) = global_drag {
                            // Use mouse Y position directly, adjust only for time overlap
                            if let Some(hover_pos) = ui.ctx().input(|i| i.pointer.hover_pos()) {
                                if hover_pos.x >= timeline_rect.min.x && hover_pos.x <= timeline_rect.max.x {
                                    let drop_frame = screen_x_to_frame(hover_pos.x, timeline_rect.min.x, config, state).round() as i32;
                                    let dur = duration.unwrap_or(10).max(1);

                                    // Calculate mouse row (raw for visual position, clamped for overlap check)
                                    let mouse_row_raw = ((hover_pos.y - timeline_rect.min.y) / config.layer_height).floor() as i32;
                                    let mouse_row = mouse_row_raw.max(0) as usize;

                                    // Check if this visual row has time overlap with any existing layer
                                    let drop_end = drop_frame + dur;
                                    let mut _has_overlap = false;

                                    // Only check overlap if mouse is within timeline bounds
                                    if hover_pos.y >= timeline_rect.min.y {
                                        for &child_idx in child_order.iter() {
                                            if let Some(child_uuid) = comp.children.get(child_idx) {
                                                let attrs = comp.children_attrs.get(child_uuid);
                                                let child_start = attrs.and_then(|a| Some(a.get_i32("start").unwrap_or(0))).unwrap_or(0);
                                                let child_end = attrs.and_then(|a| Some(a.get_i32("end").unwrap_or(0))).unwrap_or(0);

                                                // Get precomputed row for this layer
                                                let child_row = layer_rows.get(&child_idx).copied().unwrap_or(0);

                                                // Check if this layer is on the same visual row as mouse
                                                if child_row == mouse_row {
                                                    // Check time overlap
                                                    if drop_frame <= child_end && drop_end >= child_start {
                                                        _has_overlap = true;
                                                        break;
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    // Always use mouse position for preview Y (shows insertion point)
                                    let row_y = if hover_pos.y >= timeline_rect.min.y {
                                        row_to_y(mouse_row, config, timeline_rect)
                                    } else {
                                        // Above timeline -> show at top
                                        timeline_rect.min.y
                                    };

                                    draw_drop_preview(
                                        &ui.painter(),
                                        drop_frame,
                                        row_y,
                                        dur,
                                        timeline_rect,
                                        config,
                                        state,
                                    );

                                    if ui.ctx().input(|i| i.pointer.any_released()) {
                                        // Always use mouse position for target row (insert between layers)
                                        let target_row = if hover_pos.y >= timeline_rect.min.y {
                                            Some(mouse_row)
                                        } else {
                                            Some(0) // Above timeline -> insert at top
                                        };

                                        dispatch(AppEvent::AddLayer {
                                            comp_uuid: comp_id.clone(),
                                            source_uuid: source_uuid.clone(),
                                            start_frame: drop_frame,
                                            target_row,
                                        });
                                        ui.ctx().data_mut(|data| {
                                            data.remove::<GlobalDragState>(egui::Id::new("global_drag_state"));
                                        });
                                    }
                                }
                            }
            } else if state.drag_state.is_none() && global_drag.is_none() {
                // Handle click/drag interaction only if no active drag state
                if (timeline_response.clicked() || timeline_response.dragged())
                    && !ui
                        .ctx()
                        .input(|i| i.pointer.button_down(egui::PointerButton::Middle))
                {
                    if let Some(pos) = timeline_response.interact_pointer_pos() {
                        // If click is within any layer row, select that layer;
                        // otherwise treat it as a frame scrub on empty space.
                        let mut clicked_layer: Option<usize> = None;
                        for (_display_idx, &original_idx) in child_order.iter().enumerate() {
                            // Get precomputed row from layout
                            let row = layer_rows.get(&original_idx).copied().unwrap_or(0);
                            let layer_y = row_to_y(row, config, timeline_rect);

                            let row_rect = Rect::from_min_max(
                                Pos2::new(timeline_rect.min.x, layer_y),
                                Pos2::new(timeline_rect.max.x, layer_y + config.layer_height),
                            );
                            if row_rect.contains(pos) {
                                clicked_layer = Some(original_idx);
                                break;
                            }
                        }

                        if let Some(idx) = clicked_layer {
                            let modifiers = ui.input(|i| i.modifiers);
                            if let Some(clicked_uuid) = comp.children.get(idx).cloned() {
                                let (selection, anchor) = compute_layer_selection(
                                    &comp.layer_selection,
                                    comp.layer_selection_anchor.clone(),
                                    clicked_uuid,
                                    idx,
                                    modifiers,
                                    &comp.children,
                                );
                                dispatch(AppEvent::CompSelectionChanged {
                                    comp_uuid: comp_id.clone(),
                                    selection,
                                    anchor,
                                });
                            }
                        } else {
                            // Click on empty space: scrub timeline without clearing selection
                            let frame = screen_x_to_frame(
                                pos.x,
                                timeline_rect.min.x,
                                config,
                                state,
                            )
                            .round() as i32;
                            dispatch(AppEvent::SetFrame(
                                frame.min(total_frames.saturating_sub(1)),
                            ));
                        }
                    }
                }
            }
        });
    });

    // Draw playhead once across ruler + bars
    if let (Some(ruler_rect), Some(timeline_rect)) = (ruler_rect, timeline_rect_global) {
        let painter = ui.painter();
        let x = frame_to_screen_x(comp.current_frame as f32, ruler_rect.min.x, config, state);
        painter.line_segment(
            [
                Pos2::new(x, ruler_rect.min.y),
                Pos2::new(x, timeline_rect.max.y),
            ],
            (2.0, Color32::from_rgb(255, 220, 100)),
        );
        let triangle_size = 8.0;
        let top_y = ruler_rect.min.y;
        let points = [
            Pos2::new(x, top_y),
            Pos2::new(x - triangle_size / 2.0, top_y - triangle_size),
            Pos2::new(x + triangle_size / 2.0, top_y - triangle_size),
        ];
        painter.add(egui::Shape::convex_polygon(
            points.to_vec(),
            Color32::from_rgb(255, 220, 100),
            (0.0, Color32::TRANSPARENT),
        ));
    }

    // Treat entire tab rect as timeline hover for hotkey routing
    if let Some(pointer) = ui.ctx().pointer_hover_pos() {
        if tab_rect.contains(pointer) {
            timeline_hovered = true;
        }
    }

    // Return actions with hover state
    super::timeline::TimelineActions {
        hovered: timeline_hovered,
    }
}

fn draw_status_strip(
    ui: &Ui,
    rect: Rect,
    statuses: &[FrameStatus],
    comp_start: i32,
    total_frames: i32,
) {
    if statuses.is_empty() {
        return;
    }

    let painter = ui.painter();
    // block_width based on total_frames (0 to comp_end+1), not statuses.len()
    let block_width = rect.width() / total_frames as f32;
    let mut run_start = 0usize;
    let mut current = statuses[0];

    let draw_run =
        |painter: &egui::Painter, start_idx: usize, end_idx: usize, status: FrameStatus| {
            if end_idx <= start_idx {
                return;
            }
            // Convert status indices to absolute frame numbers
            let abs_start = comp_start + start_idx as i32;
            let abs_end = comp_start + end_idx as i32;

            let x_start = rect.min.x + (abs_start as f32 * block_width);
            let x_end = rect.min.x + (abs_end as f32 * block_width);
            let run_rect =
                Rect::from_min_max(Pos2::new(x_start, rect.min.y), Pos2::new(x_end, rect.max.y));
            painter.rect_filled(run_rect, 0.0, status.color());
        };

    for (idx, status) in statuses.iter().enumerate().skip(1) {
        if *status != current {
            draw_run(painter, run_start, idx, current);
            run_start = idx;
            current = *status;
        }
    }

    draw_run(painter, run_start, statuses.len(), current);
}
