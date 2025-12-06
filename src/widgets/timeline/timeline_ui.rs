//! After Effects-style timeline - UI rendering
//!
//! Each layer is displayed as a row showing:
//! - Layer name / clip name
//! - Start..End range as horizontal bar
//! - Visual indication of current_frame (playhead)
//!
//! # Interactions
//!
//! - **Click**: Select layer (with Shift/Ctrl for multi-select)
//! - **Double-click**: Dive into source comp (activates the layer's source)
//! - **Drag**: Move layer position or reorder
//! - **Edge drag**: Trim in/out points
//!
//! Consumed by: `ui::render_timeline_panel`. Emits events through
//! dispatch closures to EventBus, driven by shared `TimelineState` from
//! `timeline.rs` and helper routines in `timeline_helpers.rs`. Data flow:
//! egui input → dispatch(BoxedEvent) → EventBus → Project/Comp mutations.

use super::timeline_helpers::{
    compute_all_layer_rows, detect_layer_tool, draw_drop_preview, draw_frame_ruler,
    frame_to_screen_x, hash_color_str, row_to_y, screen_x_to_frame,
};
use super::{GlobalDragState, TimelineConfig, TimelineState};
use crate::entities::{Comp, frame::FrameStatus};
use crate::core::event_bus::BoxedEvent;
use crate::core::player_events::{JumpToStartEvent, JumpToEndEvent, TogglePlayPauseEvent, StopEvent, SetFrameEvent};
use crate::core::project_events::ProjectActiveChangedEvent;
use crate::entities::comp_events::{
    AddLayerEvent, CompSelectionChangedEvent, LayerAttributesChangedEvent,
    MoveAndReorderLayerEvent, ReorderLayerEvent, SetLayerPlayEndEvent, SetLayerPlayStartEvent,
};
use super::timeline_events::{
    TimelineFitAllEvent, TimelineLockWorkAreaChangedEvent, TimelinePanChangedEvent,
    TimelineSnapChangedEvent, TimelineZoomChangedEvent,
};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Ui, Vec2};
use egui_dnd::dnd;
use uuid::Uuid;

fn compute_layer_selection(
    current: &[Uuid],
    anchor: Option<Uuid>,
    clicked_uuid: Uuid,
    clicked_idx: usize,
    modifiers: egui::Modifiers,
    all_children: &[Uuid],
) -> (Vec<Uuid>, Option<Uuid>) {
    if modifiers.shift {
        let anchor_uuid = anchor.unwrap_or(clicked_uuid);
        let anchor_idx = all_children.iter().position(|u| *u == anchor_uuid).unwrap_or(clicked_idx);
        let (lo, hi) = if anchor_idx <= clicked_idx {
            (anchor_idx, clicked_idx)
        } else {
            (clicked_idx, anchor_idx)
        };
        let selection: Vec<Uuid> = all_children[lo..=hi].to_vec();
        (selection, Some(anchor_uuid))
    } else if modifiers.ctrl {
        let mut selection: Vec<Uuid> = current.to_vec();
        if let Some(pos) = selection.iter().position(|v| *v == clicked_uuid) {
            selection.remove(pos);
        } else {
            selection.push(clicked_uuid);
        }
        (selection, anchor)
    } else {
        (vec![clicked_uuid], Some(clicked_uuid))
    }
}

/// Render timeline toolbar (transport controls, zoom, snap)
pub fn render_toolbar(ui: &mut Ui, state: &mut TimelineState, mut dispatch: impl FnMut(BoxedEvent)) {
    ui.horizontal(|ui| {
        if ui.button("↞").on_hover_text("To Start").clicked() {
            dispatch(Box::new(JumpToStartEvent));
        }

        let play_icon = "▶"; // Placeholder - real icon controlled by playback status
        if ui.button(play_icon).on_hover_text("Play/Pause").clicked() {
            dispatch(Box::new(TogglePlayPauseEvent));
        }

        if ui.button("■").on_hover_text("Stop").clicked() {
            dispatch(Box::new(StopEvent));
        }

        if ui.button("↠").on_hover_text("To End").clicked() {
            dispatch(Box::new(JumpToEndEvent));
        }

        ui.separator();

        // Zoom controls emit actions; actual zoom applies in canvas via events
        ui.label("Zoom:");
        let zoom_response = ui.add_sized(
            egui::Vec2::new(200.0, 20.0), // 2x longer slider
            egui::Slider::new(&mut state.zoom, 0.1..=20.0).fixed_decimals(2),
        );
        if zoom_response.changed() {
            dispatch(Box::new(TimelineZoomChangedEvent(state.zoom)));
        }
        if ui
            .button("Reset")
            .on_hover_text("Reset Zoom to 1.0")
            .clicked()
        {
            state.zoom = 1.0;
            dispatch(Box::new(TimelineZoomChangedEvent(1.0)));
        }
        if ui
            .button("Fit")
            .on_hover_text("Fit all clips to view")
            .clicked()
        {
            dispatch(Box::new(TimelineFitAllEvent(state.last_canvas_width)));
        }

        if ui.checkbox(&mut state.snap_enabled, "Snap").changed() {
            dispatch(Box::new(TimelineSnapChangedEvent(state.snap_enabled)));
        }
        if ui.checkbox(&mut state.lock_work_area, "Lock").changed() {
            dispatch(Box::new(TimelineLockWorkAreaChangedEvent(state.lock_work_area)));
        }
    });
}

/// Render left outline: layer list only (no toolbar)
pub fn render_outline(
    ui: &mut Ui,
    comp_uuid: Uuid,
    comp: &Comp,
    config: &TimelineConfig,
    _state: &mut TimelineState,
    view_mode: super::TimelineViewMode,
    mut dispatch: impl FnMut(BoxedEvent),
) {
    let comp_id = comp_uuid;

    // Render layer list with DnD inside a ScrollArea to avoid growing the parent panel.
    let mut child_order: Vec<usize> = (0..comp.children.len()).collect();
    let dnd_response = egui::ScrollArea::vertical()
        .id_salt("timeline_layers_scroll") // share scroll with canvas
        .max_height(ui.available_height())
        .show(ui, |ui| {
            ui.vertical(|ui| {
                // Match the top padding of the timeline canvas (ruler + optional status bar + spacing)
                // Ruler: 20.0, Status strip: 2.0 if present, spacer: 4.0
                let status_bar_height = comp
                    .cache_frame_statuses()
                    .as_ref()
                    .map(|_| 2.0)
                    .unwrap_or(0.0);
                ui.add_space(20.0 + status_bar_height + 4.0);

                dnd(ui, "timeline_child_names_outline").show_vec(
                    &mut child_order,
                    |ui, child_idx, handle, _state| {
                        let idx = *child_idx;
                        let (child_uuid, attrs) = &comp.children[idx];

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

                        let mut visible = attrs.get_bool("visible").unwrap_or(true);
                        let mut opacity = attrs.get_float("opacity").unwrap_or(1.0);
                        let prev_blend = attrs
                            .get_str("blend_mode")
                            .unwrap_or("normal")
                            .to_string();
                        let mut blend = prev_blend.clone();
                        let mut speed = attrs.get_float("speed").unwrap_or(1.0);
                        let mut dirty = false;

                        if row_ui.checkbox(&mut visible, "").changed() {
                            dirty = true;
                        }

                        let child_name = attrs
                            .get_str("name")
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| child_uuid.to_string());
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
                            dispatch(Box::new(LayerAttributesChangedEvent {
                                comp_uuid: comp_id,
                                layer_uuid: *child_uuid,
                                visible,
                                opacity,
                                blend_mode: blend,
                                speed,
                            }));
                        }

                        if response.clicked() {
                            let modifiers = ui.input(|i| i.modifiers);
                            let clicked_uuid = *child_uuid;
                            let children_uuids = comp.children_uuids_vec();
                            let (selection, anchor) = compute_layer_selection(
                                &comp.layer_selection,
                                comp.layer_selection_anchor,
                                clicked_uuid,
                                idx,
                                modifiers,
                                &children_uuids,
                            );
                            dispatch(Box::new(CompSelectionChangedEvent {
                                comp_uuid: comp_id,
                                selection,
                                anchor,
                            }));
                        }

                        // Double-click: dive into source comp
                        if response.double_clicked() {
                            if let Some(source_uuid_str) = attrs.get_str("uuid") {
                                if let Ok(source_uuid) = Uuid::parse_str(source_uuid_str) {
                                    dispatch(Box::new(ProjectActiveChangedEvent(source_uuid)));
                                }
                            }
                        }
                    },
                )
            })
            .inner
        })
        .inner;

    if let Some(update) = dnd_response.final_update() {
        dispatch(Box::new(ReorderLayerEvent {
            comp_uuid: comp_id,
            from_idx: update.from,
            to_idx: update.to,
        }));
    }
}

/// Render After Effects-style timeline (right canvas)
pub fn render_canvas(
    ui: &mut Ui,
    comp_uuid: Uuid,
    comp: &Comp,
    project: &crate::entities::Project,
    config: &TimelineConfig,
    state: &mut TimelineState,
    view_mode: super::TimelineViewMode,
    mut dispatch: impl FnMut(BoxedEvent),
) -> super::timeline::TimelineActions {
    // Save canvas width for Fit button calculation
    state.last_canvas_width = ui.available_width();

    let comp_id = comp_uuid;
    // Calculate dimensions - timeline should show from 0 to end (not start to end)
    // This allows negative starts and ensures ruler shows full range
    let comp_start = comp._in();
    let comp_end = comp._out();
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
    let status_bar_height = status_strip.as_ref().map(|_| 2.0).unwrap_or(0.0);

    // Options + time ruler row (always visible)
    let mut ruler_rect: Option<Rect> = None;
    let mut timeline_rect_global: Option<Rect> = None;
    let ruler_height = 20.0;
    let mut timeline_hovered = false; // Track hover state for input routing
    let tab_rect = ui.max_rect(); // Full tab rect for hover detection

    // Draw ruler with proper layout sync
    ui.horizontal(|ui| {
        // Add left spacer only in OutlineOnly mode (to align ruler with outline column)
        // In CanvasOnly mode there's no outline, in Split mode outline is separate panel
        if matches!(view_mode, super::TimelineViewMode::OutlineOnly) {
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
            dispatch(Box::new(SetFrameEvent(frame)));
        }

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

    // Status strip (if present) - draw inside horizontal layout to align with ruler
    if let Some(statuses) = &status_strip {
        if let Some(ruler) = ruler_rect {
            ui.horizontal(|ui| {
                // Add left spacer only in OutlineOnly mode (same as ruler)
                if matches!(view_mode, super::TimelineViewMode::OutlineOnly) {
                    ui.allocate_exact_size(
                        Vec2::new(config.name_column_width, status_bar_height),
                        Sense::hover(),
                    );
                }

                // Allocate status strip with same width as ruler
                let (status_rect, _) = ui.allocate_exact_size(
                    Vec2::new(ruler.width(), status_bar_height),
                    Sense::hover(),
                );
                // Pass ruler_rect to ensure alignment
                draw_status_strip(ui, status_rect, statuses, comp_start, total_frames, ruler, config, state);
            });
        }
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
            dispatch(Box::new(TimelinePanChangedEvent(state.pan_offset - delta_frames)));
        }

        // Draw layers (egui automatically clips to visible area inside ScrollArea)
                        // Cache LayerGeom results to avoid recalculating in interaction pass
                        let mut geom_cache: std::collections::HashMap<usize, super::timeline::LayerGeom> =
                            std::collections::HashMap::with_capacity(child_order.len());

                        // Draw child bars using precomputed layout
                        for (_display_idx, &original_idx) in child_order.iter().enumerate() {
                            let idx = original_idx;
                            let (child_uuid, attrs) = &comp.children[idx];

                            // Get child start/end from attrs (now supports negative values)
                            let child_start = attrs.get_i32("in").unwrap_or(0);
                            let child_end = attrs.get_i32("out").unwrap_or(0);

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
                            let play_start = attrs.get_i32("trim_in").unwrap_or(child_start);
                            let play_end = attrs.get_i32("trim_out").unwrap_or(child_end);
                            let is_visible = attrs.get_bool("visible").unwrap_or(true);

                            // Calculate layer geometry and cache for interaction pass
                            let geom = super::timeline::LayerGeom::calc(
                                child_start, child_end, play_start, play_end,
                                child_y, timeline_rect, config, state
                            );
                            geom_cache.insert(idx, geom);

                            // Child bar color (use hash of name for stable color per clip)
                            let child_name = attrs.get_str("name").unwrap_or("?");
                            let base_color = if is_visible {
                                hash_color_str(child_name)
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

                                // Draw diagonal hatch pattern for file comps (over the color)
                                let is_source_file = attrs.get_str("uuid")
                                    .and_then(|s| Uuid::parse_str(s).ok())
                                    .and_then(|source_uuid| project.get_comp(source_uuid))
                                    .map(|source| source.is_file_mode())
                                    .unwrap_or(false);

                                if is_source_file {
                                    let hatch_id = state.get_hatch_texture(ui.ctx());
                                    // Calculate UV to tile the pattern across the bar
                                    let uv_scale = 16.0; // Pattern size in pixels
                                    let uv = Rect::from_min_max(
                                        Pos2::new(visible_bar_rect.min.x / uv_scale, visible_bar_rect.min.y / uv_scale),
                                        Pos2::new(visible_bar_rect.max.x / uv_scale, visible_bar_rect.max.y / uv_scale),
                                    );
                                    painter.image(hatch_id, visible_bar_rect, uv, Color32::WHITE);
                                }

                                // Draw layer name centered on visible bar
                                let text_color = Color32::WHITE;
                                let font_id = egui::FontId::proportional(11.0);
                                let galley = painter.layout_no_wrap(
                                    child_name.to_string(),
                                    font_id,
                                    text_color,
                                );
                                // Only draw if bar is wide enough for text
                                if visible_bar_rect.width() > galley.size().x + 8.0 {
                                    let text_pos = visible_bar_rect.center() - galley.size() / 2.0;
                                    painter.galley(text_pos, galley, text_color);
                                }
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
                            let (child_uuid, attrs) = &comp.children[idx];

                            // Use cached geometry from draw pass
                            let Some(&geom) = geom_cache.get(&idx) else { continue };

                            // Manual tool detection: use current visible bar bounds for handles/move
                            let edge_threshold = 8.0;
                            if let Some(hover_pos) = ui.ctx().input(|i| i.pointer.hover_pos()) {
                                let handle_rect = geom.visible_bar_rect.unwrap_or(geom.full_bar_rect);

                                // Double-click: dive into source comp (check independently of drag state)
                                if handle_rect.contains(hover_pos) {
                                    if ui.ctx().input(|i| i.pointer.button_double_clicked(egui::PointerButton::Primary)) {
                                        if let Some(source_uuid_str) = attrs.get_str("uuid") {
                                            if let Ok(source_uuid) = Uuid::parse_str(source_uuid_str) {
                                                dispatch(Box::new(ProjectActiveChangedEvent(source_uuid)));
                                            }
                                        }
                                    }
                                }

                                if state.drag_state.is_none() && handle_rect.contains(hover_pos) {
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
                                                {
                                                    let modifiers = ui.ctx().input(|i| i.modifiers);
                                                    let multi = modifiers.ctrl || modifiers.shift || modifiers.command;
                                                    if !multi && !comp.layer_selection.contains(child_uuid) {
                                                        dispatch(Box::new(CompSelectionChangedEvent {
                                                            comp_uuid: comp_id,
                                                            selection: vec![*child_uuid],
                                                            anchor: Some(*child_uuid),
                                                        }));
                                                    }
                                                }
                                                {
                                                    state.drag_state =
                                                        Some(tool.to_drag_state(idx, attrs, hover_pos));
                                                    log::debug!(
                                                        "[TIMELINE] Drag state created successfully"
                                                    );
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
                        // Use latest_pos() instead of hover_pos() to track cursor even outside window
                        if let Some(drag) = &state.drag_state.clone() {
                            if let Some(current_pos) = ui.ctx().input(|i| i.pointer.latest_pos()) {
                                match drag {
                                    GlobalDragState::TimelinePan { drag_start_pos, initial_pan_offset } => {
                                        let delta_x = current_pos.x - drag_start_pos.x;
                                        let delta_frames = delta_x / (config.pixels_per_frame * state.zoom);
                                        let new_pan = initial_pan_offset - delta_frames;

                                        // Update state directly to avoid frame delay
                                        state.pan_offset = new_pan;
                                        dispatch(Box::new(TimelinePanChangedEvent(new_pan)));

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
                                        let dragged_uuid = comp.children.get(*layer_idx).map(|(u, _)| *u).unwrap_or_default();
                                        let selection = if comp.layer_selection.contains(&dragged_uuid) {
                                            comp.layer_selection.clone()
                                        } else {
                                            vec![dragged_uuid]
                                        };

                                        for child_uuid in selection {
                                            if let Some(attrs) = comp.children_attrs_get(&child_uuid) {
                                                let idx_sel = comp.uuid_to_idx(child_uuid).unwrap_or(0);
                                                let current_row = layer_rows.get(&idx_sel).copied().unwrap_or(idx_sel);
                                                let target_row = (current_row as i32 + delta_children)
                                                    .clamp(0, comp.children.len().saturating_sub(1) as i32)
                                                    as usize;

                                                let ghost_child_y = row_to_y(target_row, config, timeline_rect);
                                                let duration = (attrs.get_i32("out").unwrap_or(0)
                                                    - attrs.get_i32("in").unwrap_or(0)
                                                    + 1)
                                                    .max(1);

                                                // Apply same delta to maintain relative offsets
                                                let child_start = attrs.get_i32("in").unwrap_or(0);
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
                                                dispatch(Box::new(MoveAndReorderLayerEvent {
                                                    comp_uuid: comp_id,
                                                    layer_idx: *layer_idx,
                                                    new_start,
                                                    new_idx: target_child,
                                                }));
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
                                        if let Some((_child_uuid, attrs)) = comp.children.get(*layer_idx) {
                                            // Use actual row for Y positioning
                                            let target_row = layer_rows
                                                .get(layer_idx)
                                                .copied()
                                                .unwrap_or_else(|| {
                                                    physical_to_display(*layer_idx).unwrap_or(*layer_idx)
                                                });
                                            let layer_y = row_to_y(target_row, config, timeline_rect);
                                            let visual_start = new_play_start as f32;
                                            let layer_end = attrs.get_i32("out").unwrap_or(0) as f32;
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

                                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);

                                        // On release, commit the play start adjustment
                                        if ui.ctx().input(|i| i.pointer.any_released()) {
                                            dispatch(Box::new(SetLayerPlayStartEvent {
                                                comp_uuid: comp_id,
                                                layer_idx: *layer_idx,
                                                new_play_start,
                                            }));
                                            state.drag_state = None;
                                        }
                                    }
                                    GlobalDragState::AdjustPlayEnd { layer_idx, initial_play_end, drag_start_x } => {
                                        let delta_x = current_pos.x - drag_start_x;
                                        let delta_frames = (delta_x / (config.pixels_per_frame * state.zoom)).round() as i32;
                                        // play_end is ABSOLUTE source frame, so drag right = increase
                                        let new_play_end = *initial_play_end + delta_frames;

                                        // Visual feedback: draw ghost play range preview
                                        if let Some((_child_uuid, attrs)) = comp.children.get(*layer_idx) {
                                            // Use actual row for Y positioning
                                            let target_row = layer_rows
                                                .get(layer_idx)
                                                .copied()
                                                .unwrap_or_else(|| {
                                                    physical_to_display(*layer_idx).unwrap_or(*layer_idx)
                                                });
                                            let layer_y = row_to_y(target_row, config, timeline_rect);
                                            let play_start = attrs.get_i32("trim_in").unwrap_or(attrs.get_i32("in").unwrap_or(0));
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

                                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);

                                        // On release, commit the play end adjustment
                                        if ui.ctx().input(|i| i.pointer.any_released()) {
                                            dispatch(Box::new(SetLayerPlayEndEvent {
                                                comp_uuid: comp_id,
                                                layer_idx: *layer_idx,
                                                new_play_end,
                                            }));
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
                        let comp_start = comp._in();
                        let comp_end = comp._out();

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

                                    // Calculate mouse row for visual positioning
                                    let mouse_row_raw = ((hover_pos.y - timeline_rect.min.y) / config.layer_height).floor() as i32;
                                    let mouse_row = mouse_row_raw.max(0) as usize;

                                    // Use mouse position for preview Y (shows insertion point)
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

                                        dispatch(Box::new(AddLayerEvent {
                                            comp_uuid: comp_id,
                                            source_uuid, // Uuid is Copy
                                            start_frame: drop_frame,
                                            target_row,
                                        }));
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
                            if let Some((clicked_uuid, _attrs)) = comp.children.get(idx) {
                                let children_uuids = comp.children_uuids_vec();
                                let (selection, anchor) = compute_layer_selection(
                                    &comp.layer_selection,
                                    comp.layer_selection_anchor.clone(),
                                    *clicked_uuid,
                                    idx,
                                    modifiers,
                                    &children_uuids,
                                );
                                dispatch(Box::new(CompSelectionChangedEvent {
                                    comp_uuid: comp_id,
                                    selection,
                                    anchor,
                                }));
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
                            dispatch(Box::new(SetFrameEvent(
                                frame.min(total_frames.saturating_sub(1)),
                            )));
                        }
                    }
                }
            }
        });
    });

    // Draw playhead once across ruler + bars
    if let (Some(ruler_rect), Some(timeline_rect)) = (ruler_rect, timeline_rect_global) {
        let painter = ui.painter();
        let x = frame_to_screen_x(comp.frame() as f32, ruler_rect.min.x, config, state);
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
    _total_frames: i32,
    ruler_rect: Rect,
    config: &super::TimelineConfig,
    state: &super::TimelineState,
) {
    if statuses.is_empty() {
        return;
    }

    let painter = ui.painter();
    // Use ruler's base_x to ensure alignment with ruler ticks and indicator
    let base_x = ruler_rect.min.x;
    
    // Calculate visible frame range using the VISIBLE width (ruler_rect.width()),
    // matching how the ruler and indicator calculate visible range
    let effective_ppf = config.pixels_per_frame * state.zoom;
    let visible_start_frame = state.pan_offset.max(comp_start as f32) as i32;
    let visible_end_frame = (state.pan_offset + (ruler_rect.width() / effective_ppf))
        .min((comp_start + statuses.len() as i32) as f32) as i32;

    let mut run_start_frame: Option<i32> = None;
    let mut current_status: Option<FrameStatus> = None;

    // Draw status runs for visible frames only
    for frame in visible_start_frame..visible_end_frame {
        let frame_offset = frame - comp_start;
        if frame_offset < 0 || frame_offset >= statuses.len() as i32 {
            continue;
        }

        let status = statuses[frame_offset as usize];

        if let Some(ref current) = current_status {
            if *current != status {
                // Draw the previous run
                if let (Some(start_frame), Some(prev_status)) = (run_start_frame, current_status) {
                    let x_start = super::timeline_helpers::frame_to_screen_x(
                        start_frame as f32,
                        base_x,
                        config,
                        state,
                    );
                    let x_end = super::timeline_helpers::frame_to_screen_x(
                        frame as f32,
                        base_x,
                        config,
                        state,
                    );
                    // Clamp to visible rect
                    let x_start = x_start.max(ruler_rect.min.x);
                    let x_end = x_end.min(ruler_rect.max.x);
                    if x_start < x_end {
                        let run_rect = Rect::from_min_max(
                            Pos2::new(x_start, rect.min.y),
                            Pos2::new(x_end, rect.max.y),
                        );
                        painter.rect_filled(run_rect, 0.0, prev_status.color());
                    }
                }
                // Start new run
                run_start_frame = Some(frame);
                current_status = Some(status);
            }
        } else {
            // First frame
            run_start_frame = Some(frame);
            current_status = Some(status);
        }
    }

    // Draw the last run
    if let (Some(start_frame), Some(status)) = (run_start_frame, current_status) {
        let x_start = super::timeline_helpers::frame_to_screen_x(
            start_frame as f32,
            base_x,
            config,
            state,
        );
        let x_end = super::timeline_helpers::frame_to_screen_x(
            visible_end_frame as f32,
            base_x,
            config,
            state,
        );
        // Clamp to visible rect
        let x_start = x_start.max(ruler_rect.min.x);
        let x_end = x_end.min(ruler_rect.max.x);
        if x_start < x_end {
            let run_rect = Rect::from_min_max(
                Pos2::new(x_start, rect.min.y),
                Pos2::new(x_end, rect.max.y),
            );
            painter.rect_filled(run_rect, 0.0, status.color());
        }
    }
}
