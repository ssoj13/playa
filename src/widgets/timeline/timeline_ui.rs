//! After Effects-style timeline - UI rendering
//!
//! Each layer is displayed as a row showing:
//! - Layer name / clip name
//! - Start..End range as horizontal bar
//! - Visual indication of current_frame (playhead)

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Ui, Vec2};
use crate::entities::Comp;
use egui_dnd::{dnd, DragDropItem};
use super::{TimelineConfig, TimelineState, GlobalDragState, TimelineAction};

/// Tool/interaction mode detected at cursor position over a layer bar
#[derive(Debug, Clone, Copy, PartialEq)]
enum LayerTool {
    AdjustPlayStart,
    AdjustPlayEnd,
    Move,
}

impl LayerTool {
    /// Get cursor icon for this tool
    fn cursor(&self) -> egui::CursorIcon {
        match self {
            LayerTool::AdjustPlayStart | LayerTool::AdjustPlayEnd => egui::CursorIcon::ResizeHorizontal,
            LayerTool::Move => egui::CursorIcon::Grab,
        }
    }

    /// Convert to drag state for given layer
    fn to_drag_state(&self, layer_idx: usize, layer: &crate::entities::layer::Layer, drag_start_pos: Pos2) -> GlobalDragState {
        match self {
            LayerTool::AdjustPlayStart => {
                let initial_play_start = layer.attrs.get_i32("play_start").unwrap_or(0);
                GlobalDragState::AdjustPlayStart { layer_idx, initial_play_start, drag_start_x: drag_start_pos.x }
            }
            LayerTool::AdjustPlayEnd => {
                let initial_play_end = layer.attrs.get_i32("play_end").unwrap_or(0);
                GlobalDragState::AdjustPlayEnd { layer_idx, initial_play_end, drag_start_x: drag_start_pos.x }
            }
            LayerTool::Move => {
                let initial_start = layer.attrs.get_u32("start").unwrap_or(0) as usize;
                let initial_end = layer.attrs.get_u32("end").unwrap_or(0) as usize;
                GlobalDragState::MovingLayer {
                    layer_idx,
                    initial_start,
                    initial_end,
                    drag_start_x: drag_start_pos.x,
                    drag_start_y: drag_start_pos.y
                }
            }
        }
    }
}

/// Detect which tool should be active at the given position over a layer bar
fn detect_layer_tool(hover_pos: Pos2, bar_rect: Rect, edge_threshold: f32) -> Option<LayerTool> {
    if !bar_rect.contains(hover_pos) {
        return None;
    }

    let dist_to_left = (hover_pos.x - bar_rect.min.x).abs();
    let dist_to_right = (hover_pos.x - bar_rect.max.x).abs();

    if dist_to_left < edge_threshold {
        Some(LayerTool::AdjustPlayStart)
    } else if dist_to_right < edge_threshold {
        Some(LayerTool::AdjustPlayEnd)
    } else {
        Some(LayerTool::Move)
    }
}

/// Convert frame index to screen X coordinate
fn frame_to_screen_x(frame: f32, timeline_rect_min_x: f32, config: &TimelineConfig, state: &TimelineState) -> f32 {
    timeline_rect_min_x + (frame - state.pan_offset) * config.pixels_per_frame * state.zoom
}

/// Convert screen X coordinate to frame index
fn screen_x_to_frame(x: f32, timeline_rect_min_x: f32, config: &TimelineConfig, state: &TimelineState) -> f32 {
    ((x - timeline_rect_min_x) / (config.pixels_per_frame * state.zoom)) + state.pan_offset
}

/// Render After Effects-style timeline
pub fn render_timeline(
    ui: &mut Ui,
    comp: &Comp,
    config: &TimelineConfig,
    state: &mut TimelineState,
) -> TimelineAction {
    let mut action = TimelineAction::None;

    // Calculate dimensions (use full frame count for timeline width, not just play_range)
    let total_frames = comp.frame_count().max(100); // Minimum 100 frames for empty comps

    let timeline_width = (total_frames as f32 * config.pixels_per_frame * state.zoom)
        .max(ui.available_width() - config.name_column_width);
    // Ensure non-zero height so DnD/drop zone works even for empty comps
    let total_height = (comp.children.len().max(1) as f32) * config.layer_height;

    // Toolbar with transport controls and zoom
    ui.horizontal(|ui| {
        // Transport controls
        if ui.button("⏮").on_hover_text("To Start").clicked() {
            action = TimelineAction::ToStart;
        }

        let play_icon = "▶"; // Will be updated based on playback state
        if ui.button(play_icon).on_hover_text("Play/Pause").clicked() {
            action = TimelineAction::TogglePlay;
        }

        if ui.button("⏹").on_hover_text("Stop").clicked() {
            action = TimelineAction::Stop;
        }

        if ui.button("⏭").on_hover_text("To End").clicked() {
            action = TimelineAction::ToEnd;
        }

        ui.separator();

        // Zoom slider
        ui.label("Zoom:");
        let mut zoom_changed = false;
        let old_zoom = state.zoom;

        let zoom_response = ui.add(
            egui::Slider::new(&mut state.zoom, 0.1..=4.0)
                .fixed_decimals(2)
                .show_value(true)
        );

        if zoom_response.changed() {
            zoom_changed = true;
        }

        // Reset zoom button
        if ui.button("R").on_hover_text("Reset Zoom to 1.0").clicked() {
            state.zoom = 1.0;
            zoom_changed = true;
        }

        // When zoom changes, adjust pan_offset to keep playhead centered
        if zoom_changed && old_zoom != state.zoom {
            // Keep playhead position stable when zooming
            let playhead_pos = comp.current_frame as f32;
            let old_screen_x = (playhead_pos - state.pan_offset) * config.pixels_per_frame * old_zoom;
            // After zoom change, adjust pan so playhead stays at same screen position
            state.pan_offset = playhead_pos - (old_screen_x / (config.pixels_per_frame * state.zoom));
        }
    });

    ui.add_space(4.0);

    // Handle keyboard shortcuts for jumping to layer edges
    if ui.ctx().input(|i| i.key_pressed(egui::Key::OpenBracket)) {
        action = TimelineAction::JumpToPrevEdge;
    }
    if ui.ctx().input(|i| i.key_pressed(egui::Key::CloseBracket)) {
        action = TimelineAction::JumpToNextEdge;
    }

    // Handle keyboard shortcuts for work area
    if ui.ctx().input(|i| i.key_pressed(egui::Key::B)) {
        let ctrl_pressed = ui.ctx().input(|i| i.modifiers.ctrl);
        if ctrl_pressed {
            action = TimelineAction::ResetCompPlayArea;
        } else {
            action = TimelineAction::SetCompPlayStart { frame: comp.current_frame };
        }
    }
    if ui.ctx().input(|i| i.key_pressed(egui::Key::N)) {
        action = TimelineAction::SetCompPlayEnd { frame: comp.current_frame };
    }

    // Header: frame numbers ruler
    if config.show_frame_numbers {
        if let Some(frame) = draw_frame_ruler(ui, comp, config, state, timeline_width) {
            action = TimelineAction::SetFrame(frame);
        }
    }

    // Scrollable area for layers
    // Use id_salt for persistence and let egui manage sizing naturally
    egui::ScrollArea::both()
        .id_salt("timeline_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Create temporary child order for egui_dnd
            let mut child_order: Vec<usize> = (0..comp.children.len()).collect();

            // Two-column layout: layer names (with DnD) | timeline bars
            ui.horizontal(|ui| {
                // Left column: layer names with egui_dnd for smooth reordering
                {
                    let dnd_response = dnd(ui, "timeline_child_names")
                        .show_vec(&mut child_order, |ui, child_idx, handle, _state| {
                            let idx = *child_idx;
                            let child_uuid = &comp.children[idx];

                            ui.horizontal(|ui| {
                                // Drag handle
                                handle.ui(ui, |ui| {
                                    ui.label("☰");
                                });

                                // Layer name
                                let (rect, response) = ui.allocate_exact_size(
                                    Vec2::new(config.name_column_width - 20.0, config.layer_height),
                                    Sense::click(),
                                );

                                let is_selected = comp.selected_layer == Some(idx);

                                // Draw layer name background (highlight when selected)
                                let name_bg = if is_selected {
                                    Color32::from_rgb(70, 100, 140)
                                } else {
                                    Color32::from_gray(40)
                                };
                                ui.painter().rect_filled(rect, 2.0, name_bg);

                                // Optional border for selected header
                                if is_selected {
                                    ui.painter().rect_stroke(
                                        rect.shrink(1.0),
                                        2.0,
                                        egui::Stroke::new(1.5, Color32::from_rgb(180, 230, 255)),
                                        egui::epaint::StrokeKind::Middle,
                                    );
                                }

                                // Get child name from attrs or use UUID
                                let child_name = comp.children_attrs.get(child_uuid)
                                    .and_then(|attrs| attrs.get_str("name"))
                                    .unwrap_or(child_uuid.as_str());

                                // Draw child name text
                                ui.painter().text(
                                    Pos2::new(rect.min.x + 8.0, rect.center().y),
                                    egui::Align2::LEFT_CENTER,
                                    child_name,
                                    egui::FontId::proportional(12.0),
                                    Color32::from_gray(200),
                                );

                                if response.clicked() {
                                    action = TimelineAction::SelectLayer(idx);
                                }
                            });
                        });

                    // Check if layer order changed and emit ReorderLayer action
                    if let Some(update) = dnd_response.final_update() {
                        action = TimelineAction::ReorderLayer {
                            from_idx: update.from,
                            to_idx: update.to,
                        };
                    }
                }

                // Right column: timeline bars
                ui.vertical(|ui| {
                    ui.set_width(timeline_width);
                    ui.set_height(total_height);

                    let (timeline_rect, timeline_response) = ui.allocate_exact_size(
                        Vec2::new(timeline_width, total_height),
                        Sense::click_and_drag(),
                    );

                    if ui.is_rect_visible(timeline_rect) {
                        let painter = ui.painter();

                        // Draw child bars in same order as child names (using child_order from DnD)
                        for (display_idx, &original_idx) in child_order.iter().enumerate() {
                            let idx = original_idx;
                            let child_uuid = &comp.children[idx];
                            let child_y = timeline_rect.min.y + (display_idx as f32 * config.layer_height);
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

                            // Get child start/end from attrs
                            let attrs = comp.children_attrs.get(child_uuid);
                            let child_start = attrs.and_then(|a| Some(a.get_u32("start").unwrap_or(0) as usize)).unwrap_or(0);
                            let child_end = attrs.and_then(|a| Some(a.get_u32("end").unwrap_or(0) as usize)).unwrap_or(0);
                            let play_start = attrs.and_then(|a| Some(a.get_i32("play_start").unwrap_or(0))).unwrap_or(0);
                            let play_end = attrs.and_then(|a| Some(a.get_i32("play_end").unwrap_or(0))).unwrap_or(0);

                            // Calculate full clip range and visible (play) range
                            let full_start = child_start;
                            let full_end = child_end;
                            let visible_start = child_start + play_start as usize;
                            let visible_end = child_end.saturating_sub(play_end as usize);

                            // Draw full child bar (grayed out, semi-transparent)
                            let full_bar_x_start = frame_to_screen_x(full_start as f32, timeline_rect.min.x, config, state);
                            let full_bar_x_end = frame_to_screen_x((full_end + 1) as f32, timeline_rect.min.x, config, state);
                            let full_bar_rect = Rect::from_min_max(
                                Pos2::new(full_bar_x_start, child_y + 4.0),
                                Pos2::new(full_bar_x_end, child_y + config.layer_height - 4.0),
                            );

                            // Child bar color (use hash of child_uuid for stable color)
                            let base_color = hash_color(child_uuid);
                            let is_selected = comp.selected_layer == Some(idx);
                            let gray_color = if is_selected {
                                // Slightly brighter grey with a blue tint when selected
                                Color32::from_rgba_unmultiplied(110, 140, 190, 130)
                            } else {
                                Color32::from_rgba_unmultiplied(80, 80, 80, 100)
                            };

                            painter.rect_filled(full_bar_rect, 4.0, gray_color);

                            // Draw visible (trimmed) area with full color on top
                            if visible_start < visible_end {
                                let visible_bar_x_start = frame_to_screen_x(visible_start as f32, timeline_rect.min.x, config, state);
                                let visible_bar_x_end = frame_to_screen_x((visible_end + 1) as f32, timeline_rect.min.x, config, state);
                                let visible_bar_rect = Rect::from_min_max(
                                    Pos2::new(visible_bar_x_start, child_y + 4.0),
                                    Pos2::new(visible_bar_x_end, child_y + config.layer_height - 4.0),
                                );
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
                                  full_bar_rect,
                                  4.0,
                                  egui::Stroke::new(stroke_width, stroke_color),
                                  egui::epaint::StrokeKind::Middle,
                              );
                        }

                        // Handle child bar interactions using proper response system
                        // We need to do this in a second pass after drawing to ensure responses are on top
                        for (display_idx, &original_idx) in child_order.iter().enumerate() {
                            let idx = original_idx;
                            let child_uuid = &comp.children[idx];

                            // Get child attrs
                            let attrs = comp.children_attrs.get(child_uuid);
                            let child_start = attrs.and_then(|a| Some(a.get_u32("start").unwrap_or(0) as usize)).unwrap_or(0);
                            let child_end = attrs.and_then(|a| Some(a.get_u32("end").unwrap_or(0) as usize)).unwrap_or(0);
                            let play_start = attrs.and_then(|a| Some(a.get_i32("play_start").unwrap_or(0))).unwrap_or(0);
                            let play_end = attrs.and_then(|a| Some(a.get_i32("play_end").unwrap_or(0))).unwrap_or(0);

                            // Calculate visible (play) range for interaction
                            let visible_start = child_start + play_start as usize;
                            let visible_end = child_end.saturating_sub(play_end as usize);

                            let child_y = timeline_rect.min.y + (display_idx as f32 * config.layer_height);

                            // Use visible range for interaction rect (user should interact with visible edges)
                            let bar_x_start = frame_to_screen_x(visible_start as f32, timeline_rect.min.x, config, state);
                            let bar_x_end = frame_to_screen_x((visible_end + 1) as f32, timeline_rect.min.x, config, state);
                            let bar_rect = Rect::from_min_max(
                                Pos2::new(bar_x_start, layer_y + 4.0),
                                Pos2::new(bar_x_end, layer_y + config.layer_height - 4.0),
                            );

                            // Check interaction with this bar using unified tool detection
                            let edge_threshold = 8.0;
                            if let Some(hover_pos) = ui.ctx().input(|i| i.pointer.hover_pos()) {
                                if state.drag_state.is_none() {
                                    if let Some(tool) = detect_layer_tool(hover_pos, bar_rect, edge_threshold) {
                                        // Set cursor based on detected tool
                                        ui.ctx().set_cursor_icon(tool.cursor());

                                        // On mouse press, create appropriate drag state
                                        if ui.ctx().input(|i| i.pointer.primary_pressed()) {
                                            state.drag_state = Some(tool.to_drag_state(idx, layer, hover_pos));
                                        }
                                    }
                                }
                            }
                        }

                        // Process active drag operations
                        if let Some(drag) = &state.drag_state.clone() {
                            if let Some(current_pos) = ui.ctx().input(|i| i.pointer.hover_pos()) {
                                match drag {
                                    GlobalDragState::MovingLayer { layer_idx, initial_start, drag_start_x, drag_start_y, .. } => {
                                        let delta_x = current_pos.x - drag_start_x;
                                        let delta_y = current_pos.y - drag_start_y;
                                        let delta_frames = (delta_x / (config.pixels_per_frame * state.zoom)).round() as i32;
                                        let new_start = (*initial_start as i32 + delta_frames).max(0) as usize;

                                        // Determine target child index from vertical position
                                        let delta_children = (delta_y / config.layer_height).round() as i32;
                                        let target_child = (*layer_idx as i32 + delta_children).max(0).min(comp.children.len() as i32 - 1) as usize;

                                        // Visual feedback: draw ghost bar at new position
                                        if *layer_idx < comp.children.len() {
                                            let child_uuid = &comp.children[*layer_idx];
                                            if let Some(attrs) = comp.children_attrs.get(child_uuid) {
                                                let ghost_child_y = timeline_rect.min.y + (target_child as f32 * config.layer_height);
                                                let duration = (attrs.get_u32("end").unwrap_or(0) as i32
                                                              - attrs.get_u32("start").unwrap_or(0) as i32).max(0) as usize;

                                                let ghost_x_start = frame_to_screen_x(new_start as f32, timeline_rect.min.x, config, state);
                                                let ghost_x_end = frame_to_screen_x((new_start + duration) as f32, timeline_rect.min.x, config, state);
                                                let ghost_rect = Rect::from_min_max(
                                                    Pos2::new(ghost_x_start, ghost_child_y + 4.0),
                                                    Pos2::new(ghost_x_end, ghost_child_y + config.layer_height - 4.0),
                                                );
                                                painter.rect_stroke(
                                                    ghost_rect,
                                                    4.0,
                                                    egui::Stroke::new(2.0, Color32::from_rgba_unmultiplied(255, 255, 255, 128)),
                                                    egui::epaint::StrokeKind::Middle,
                                                );
                                            }
                                        }

                                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);

                                        // On release, commit the move (horizontal and/or vertical)
                                        if ui.ctx().input(|i| i.pointer.any_released()) {
                                            // If layer changed vertically, reorder first
                                            if target_layer != *layer_idx {
                                                action = TimelineAction::ReorderLayer {
                                                    from_idx: *layer_idx,
                                                    to_idx: target_layer,
                                                };
                                            } else {
                                                action = TimelineAction::MoveLayer {
                                                    layer_idx: *layer_idx,
                                                    new_start,
                                                };
                                            }
                                            state.drag_state = None;
                                        }
                                    }
                                    GlobalDragState::AdjustPlayStart { layer_idx, initial_play_start, drag_start_x } => {
                                        let delta_x = current_pos.x - drag_start_x;
                                        let delta_frames = (delta_x / (config.pixels_per_frame * state.zoom)).round() as i32;
                                        let new_play_start = (*initial_play_start + delta_frames).max(0);

                                        // Visual feedback: draw ghost play range preview
                                        if *layer_idx < comp.children.len() {
                                            let child_uuid = &comp.children[*layer_idx];
                                            if let Some(attrs) = comp.children_attrs.get(child_uuid) {
                                                let layer_y = timeline_rect.min.y + (*layer_idx as f32 * config.layer_height);
                                                let layer_start = attrs.get_u32("start").unwrap_or(0) as usize;
                                                let layer_end = attrs.get_u32("end").unwrap_or(0) as usize;

                                                // New visual start accounting for play_start
                                                let visual_start = layer_start + new_play_start as usize;
                                                let ghost_x_start = frame_to_screen_x(visual_start as f32, timeline_rect.min.x, config, state);
                                                let ghost_x_end = frame_to_screen_x(layer_end as f32, timeline_rect.min.x, config, state);

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
                                            action = TimelineAction::SetLayerPlayStart {
                                                layer_idx: *layer_idx,
                                                new_play_start,
                                            };
                                            state.drag_state = None;
                                        }
                                    }
                                    GlobalDragState::AdjustPlayEnd { layer_idx, initial_play_end, drag_start_x } => {
                                        let delta_x = current_pos.x - drag_start_x;
                                        let delta_frames = (delta_x / (config.pixels_per_frame * state.zoom)).round() as i32;
                                        let new_play_end = (*initial_play_end - delta_frames).max(0); // Note: inverted for end

                                        // Visual feedback: draw ghost play range preview
                                        if *layer_idx < comp.children.len() {
                                            let child_uuid = &comp.children[*layer_idx];
                                            if let Some(attrs) = comp.children_attrs.get(child_uuid) {
                                                let layer_y = timeline_rect.min.y + (*layer_idx as f32 * config.layer_height);
                                                let layer_start = attrs.get_u32("start").unwrap_or(0) as usize;
                                                let layer_end = attrs.get_u32("end").unwrap_or(0) as usize;

                                                // New visual end accounting for play_end
                                                let visual_end = layer_end.saturating_sub(new_play_end as usize);
                                                let ghost_x_start = frame_to_screen_x(layer_start as f32, timeline_rect.min.x, config, state);
                                                let ghost_x_end = frame_to_screen_x(visual_end as f32, timeline_rect.min.x, config, state);

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
                                            action = TimelineAction::SetLayerPlayEnd {
                                                layer_idx: *layer_idx,
                                                new_play_end,
                                            };
                                            state.drag_state = None;
                                        }
                                    }
                                    // Other drag states are handled elsewhere (ProjectItem, TimelineScrub, TimelinePan)
                                    _ => {}
                                }
                            }

                            // Cancel drag on escape
                            if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
                                state.drag_state = None;
                            }
                        }

                        // Draw work area overlay (darken regions outside play_range)
                        let (play_start, play_end) = comp.play_range();
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

                        // Draw playhead (current_frame) as vertical line
                        draw_playhead(painter, timeline_rect, comp.current_frame, config, state);

                        // Check for drag'n'drop from Project Window using global drag state
                        let global_drag: Option<GlobalDragState> = ui.ctx().data(|data| {
                            data.get_temp(egui::Id::new("global_drag_state"))
                        });

                        if let Some(GlobalDragState::ProjectItem { source_uuid, display_name, duration, .. }) = global_drag {
                            // Show drop preview
                            if let Some(hover_pos) = ui.ctx().input(|i| i.pointer.hover_pos()) {
                                // Treat full vertical span of timeline area as drop zone; only X matters.
                                if hover_pos.x >= timeline_rect.min.x && hover_pos.x <= timeline_rect.max.x {
                                    let drop_frame = screen_x_to_frame(hover_pos.x, timeline_rect.min.x, config, state).round() as usize;

                                    // Draw drop indicator (vertical line)
                                    let drop_x = frame_to_screen_x(drop_frame as f32, timeline_rect.min.x, config, state);
                                    painter.line_segment(
                                        [Pos2::new(drop_x, timeline_rect.min.y), Pos2::new(drop_x, timeline_rect.max.y)],
                                        (3.0, Color32::from_rgb(100, 220, 255)),
                                    );

                                    // Optional ghost bar to visualize approximate layer span
                                    if let Some(len) = duration {
                                        let ghost_start = drop_frame as f32;
                                        let ghost_end = ghost_start + len as f32;
                                        let ghost_x_start = frame_to_screen_x(ghost_start, timeline_rect.min.x, config, state);
                                        let ghost_x_end = frame_to_screen_x(ghost_end, timeline_rect.min.x, config, state);
                                        let ghost_rect = Rect::from_min_max(
                                            Pos2::new(ghost_x_start, timeline_rect.min.y + 4.0),
                                            Pos2::new(ghost_x_end, timeline_rect.min.y + config.layer_height - 4.0),
                                        );
                                        painter.rect_filled(
                                            ghost_rect,
                                            4.0,
                                            Color32::from_rgba_unmultiplied(100, 220, 255, 40),
                                        );
                                        // Draw name inside ghost bar
                                        painter.text(
                                            Pos2::new((ghost_x_start + ghost_x_end) * 0.5, ghost_rect.center().y),
                                            egui::Align2::CENTER_CENTER,
                                            display_name,
                                            egui::FontId::proportional(12.0),
                                            Color32::from_rgb(200, 230, 255),
                                        );
                                    }

                                    // Check for mouse release (drop)
                                    if ui.ctx().input(|i| i.pointer.any_released()) {
                                        action = TimelineAction::AddLayer {
                                            source_uuid: source_uuid.clone(),
                                            start_frame: drop_frame,
                                        };
                                        // Clear global drag state
                                        ui.ctx().data_mut(|data| {
                                            data.remove::<GlobalDragState>(egui::Id::new("global_drag_state"));
                                        });
                                    }
                                }
                            }
                        } else if state.drag_state.is_none() && global_drag.is_none() {
                          // Handle click/drag interaction only if no active drag state
                              if timeline_response.clicked() || timeline_response.dragged() {
                                  if let Some(pos) = timeline_response.interact_pointer_pos() {
                                      // If click is within any layer row, select that layer;
                                      // otherwise treat it as a frame scrub on empty space.
                                      let mut clicked_layer: Option<usize> = None;
                                      for (display_idx, &original_idx) in layer_order.iter().enumerate() {
                                          let layer_y = timeline_rect.min.y + (display_idx as f32 * config.layer_height);
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
                                          action = TimelineAction::SelectLayer(idx);
                                      } else {
                                          let frame = screen_x_to_frame(pos.x, timeline_rect.min.x, config, state).round() as usize;
                                          action = TimelineAction::SetFrame(frame.min(total_frames.saturating_sub(1)));
                                      }
                                  } else {
                                      // Click without position: clear selection
                                      action = TimelineAction::ClearSelection;
                                  }
                              }
                        }
                    }
                });
            });
        });

    action
}

/// Draw frame number ruler at top of timeline
fn draw_frame_ruler(
    ui: &mut Ui,
    comp: &Comp,
    config: &TimelineConfig,
    state: &TimelineState,
    timeline_width: f32,
) -> Option<usize> {
    let total_frames = comp.frame_count(); // Use full frame count, not play_range
    let ruler_height = 20.0;

    ui.horizontal(|ui| {
        // Empty space for name column
        ui.allocate_exact_size(Vec2::new(config.name_column_width, ruler_height), Sense::hover());

        // Ruler area - make it interactive for timeline scrubbing
        let (rect, ruler_response) = ui.allocate_exact_size(
            Vec2::new(timeline_width, ruler_height),
            Sense::click_and_drag(),
        );

        if ui.is_rect_visible(rect) {
            let painter = ui.painter();

            // Ruler background
            painter.rect_filled(rect, 0.0, Color32::from_gray(25));

            // Draw playhead line on ruler too
            let playhead_x = frame_to_screen_x(comp.current_frame as f32, rect.min.x, config, state);
            if playhead_x >= rect.min.x && playhead_x <= rect.max.x {
                painter.line_segment(
                    [Pos2::new(playhead_x, rect.min.y), Pos2::new(playhead_x, rect.max.y)],
                    (2.0, Color32::from_rgb(255, 220, 100)),
                );
            }

            // Draw frame markers every N frames (adaptive based on zoom)
            let effective_ppf = config.pixels_per_frame * state.zoom;
            let frame_step = if effective_ppf > 10.0 {
                1
            } else if effective_ppf > 2.0 {
                5
            } else if effective_ppf > 0.5 {
                10
            } else {
                50
            };

            // Adaptive label step to prevent overlap at high zoom
            let label_step = if effective_ppf > 50.0 {
                10
            } else if effective_ppf > 20.0 {
                5
            } else {
                (frame_step * 2).max(frame_step) // At least frame_step to ensure some labels
            };

            // Determine visible frame range
            let visible_start = state.pan_offset.max(0.0) as usize;
            let visible_end = (state.pan_offset + (timeline_width / effective_ppf)).min(total_frames as f32) as usize;

            // Start from frame that is aligned to frame_step grid
            let start_frame = (visible_start / frame_step.max(1)) * frame_step.max(1);

            for frame in (start_frame..=visible_end).step_by(frame_step.max(1)) {
                let x = frame_to_screen_x(frame as f32, rect.min.x, config, state);
                if x < rect.min.x || x > rect.max.x {
                    continue;
                }

                // Draw tick mark
                painter.line_segment(
                    [Pos2::new(x, rect.max.y - 5.0), Pos2::new(x, rect.max.y)],
                    (1.0, Color32::from_gray(100)),
                );

                // Draw frame number with adaptive step to prevent overlap
                if frame % label_step == 0 {
                    painter.text(
                        Pos2::new(x, rect.min.y + 2.0),
                        egui::Align2::CENTER_TOP,
                        format!("{}", frame),
                        egui::FontId::monospace(9.0),
                        Color32::from_gray(150),
                    );
                }
            }

            // Handle ruler click/drag for timeline scrubbing
            if ruler_response.clicked() || ruler_response.dragged() {
                if let Some(pos) = ruler_response.interact_pointer_pos() {
                    let frame = screen_x_to_frame(pos.x, rect.min.x, config, state).round() as usize;
                    return Some(frame.min(total_frames.saturating_sub(1)));
                }
            }
        }

        None
    }).inner
}

/// Draw playhead indicator at current frame
fn draw_playhead(
    painter: &egui::Painter,
    timeline_rect: Rect,
    current_frame: usize,
    config: &TimelineConfig,
    state: &TimelineState,
) {
    let x = frame_to_screen_x(current_frame as f32, timeline_rect.min.x, config, state);

    // Vertical line through all layers
    painter.line_segment(
        [Pos2::new(x, timeline_rect.min.y), Pos2::new(x, timeline_rect.max.y)],
        (2.0, Color32::from_rgb(255, 220, 100)),
    );

    // Triangle indicator at top
    let triangle_size = 8.0;
    let top_y = timeline_rect.min.y;
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

/// Generate stable color from string using hash
fn hash_color(s: &str) -> Color32 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    let hash = hasher.finish();

    // Use hash to generate hue (0-360)
    let hue = (hash % 360) as f32;

    // Fixed saturation and value for consistent look
    let saturation = 0.65;
    let value = 0.55;

    hsv_to_rgb(hue, saturation, value)
}

/// Convert HSV to RGB (for color generation)
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> Color32 {
    let c = v * s;
    let h_prime = h / 60.0;
    let x = c * (1.0 - ((h_prime % 2.0) - 1.0).abs());
    let m = v - c;

    let (r, g, b) = if h_prime < 1.0 {
        (c, x, 0.0)
    } else if h_prime < 2.0 {
        (x, c, 0.0)
    } else if h_prime < 3.0 {
        (0.0, c, x)
    } else if h_prime < 4.0 {
        (0.0, x, c)
    } else if h_prime < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    Color32::from_rgb(
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}

