//! After Effects-style timeline - UI rendering
//!
//! Each layer is displayed as a row showing:
//! - Layer name / clip name
//! - Start..End range as horizontal bar
//! - Visual indication of current_frame (playhead)

use crate::entities::Comp;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Ui, Vec2};
use egui_dnd::dnd;
use super::{TimelineAction, TimelineConfig, TimelineState, GlobalDragState};
use super::timeline_helpers::{LayerTool, frame_to_screen_x, screen_x_to_frame, draw_frame_ruler, hash_color};

/// Render After Effects-style timeline
pub fn render(
    ui: &mut Ui,
    comp: &mut Comp,
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

    // Options + time ruler row (split: left options, right ruler)
    let mut ruler_rect: Option<Rect> = None;
    let mut timeline_rect_global: Option<Rect> = None;
    let ruler_height = 20.0;
    ui.horizontal(|ui| {
        let (opt_rect, _) = ui.allocate_exact_size(
            Vec2::new(config.name_column_width, ruler_height),
            Sense::hover(),
        );
        let mut opt_ui = ui.child_ui(
            opt_rect,
            egui::Layout::left_to_right(egui::Align::Center),
            None,
        );
        opt_ui.spacing_mut().item_spacing = egui::vec2(6.0, 0.0);
        opt_ui.checkbox(&mut state.show_frame_numbers, "Frames");
        opt_ui.checkbox(&mut state.snap_enabled, "Snap");
        opt_ui.checkbox(&mut state.lock_work_area, "Lock");

        let mut cfg_for_ruler = config.clone();
        cfg_for_ruler.show_frame_numbers = state.show_frame_numbers;
        let ruler_width = (total_frames as f32 * cfg_for_ruler.pixels_per_frame * state.zoom)
            .max(ui.available_width());
        egui::ScrollArea::horizontal()
            .id_salt("timeline_h_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_height(ruler_height);
                ui.set_width(ruler_width);
                if cfg_for_ruler.show_frame_numbers {
                    let (frame_opt, rect) = draw_frame_ruler(ui, comp, &cfg_for_ruler, state, ruler_width);
                    ruler_rect = Some(rect);
                    if let Some(frame) = frame_opt {
                        action = TimelineAction::SetFrame(frame);
                    }
                } else {
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(ruler_width, ruler_height), Sense::hover());
                    ruler_rect = Some(rect);
                }
            });
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

    // Two-column layout without vertical scroll (timeline panel height is fixed)
    ui.push_id("timeline_layers", |ui| {
            // Create temporary child order for egui_dnd
            let mut child_order: Vec<usize> = (0..comp.children.len()).collect();

            // Two-column layout: layer names (with DnD) | timeline bars
            ui.horizontal(|ui| {
                // Left column: layer names with egui_dnd for smooth reordering
                let dnd_response = ui.vertical(|ui| {
                    dnd(ui, "timeline_child_names")
                        .show_vec(&mut child_order, |ui, child_idx, handle, _state| {
                            let idx = *child_idx;
                            let child_uuid = &comp.children[idx];
                            let attrs = comp.children_attrs.get(child_uuid);

                            let (row_rect, response) = ui.allocate_exact_size(
                                Vec2::new(config.name_column_width, config.layer_height),
                                Sense::click(),
                            );
                            let mut row_ui = ui.child_ui(
                                row_rect,
                                egui::Layout::left_to_right(egui::Align::Center),
                                None,
                            );
                            row_ui.spacing_mut().item_spacing = egui::vec2(6.0, 0.0);
                            row_ui.set_min_height(config.layer_height);

                            // Drag handle
                            handle.ui(&mut row_ui, |ui| {
                                ui.label("☰");
                            });

                            // Visibility toggle
                            let mut visible = attrs.and_then(|a| a.get_bool("visible")).unwrap_or(true);
                            let mut opacity = attrs.and_then(|a| a.get_float("opacity")).unwrap_or(1.0);
                            let prev_blend = attrs.and_then(|a| a.get_str("blend_mode")).unwrap_or("normal").to_string();
                            let mut blend = prev_blend.clone();
                            let mut speed = attrs.and_then(|a| a.get_float("speed")).unwrap_or(1.0);
                            let mut dirty = false;

                            if row_ui.checkbox(&mut visible, "").changed() {
                                dirty = true;
                            }

                            // Layer name
                            let child_name = comp.children_attrs.get(child_uuid)
                                .and_then(|attrs| attrs.get_str("name"))
                                .unwrap_or(child_uuid.as_str());
                            row_ui.label(child_name);

                            // Opacity slider
                            if row_ui.add(
                                egui::Slider::new(&mut opacity, 0.0..=1.0)
                                    .show_value(false)
                                    .smallest_positive(0.01)
                                    .text(""),
                            ).changed() {
                                dirty = true;
                            }

                            // Blend mode combo
                            egui::ComboBox::from_id_salt(format!("blend_{}", child_uuid))
                                .width(80.0)
                                .selected_text(blend.clone())
                                .show_ui(&mut row_ui, |ui| {
                                    for mode in ["normal", "screen", "add", "subtract", "multiply", "divide"] {
                                        ui.selectable_value(&mut blend, mode.to_string(), mode);
                                    }
                                });
                            if blend != prev_blend {
                                dirty = true;
                            }

                            // Speed
                            if row_ui.add(egui::DragValue::new(&mut speed).speed(0.1).range(0.01..=8.0)).changed() {
                                dirty = true;
                            }

                            if dirty {
                                if let Some(attrs_mut) = comp.children_attrs.get_mut(child_uuid) {
                                    attrs_mut.set("visible", crate::entities::AttrValue::Bool(visible));
                                    attrs_mut.set("opacity", crate::entities::AttrValue::Float(opacity));
                                    attrs_mut.set("blend_mode", crate::entities::AttrValue::Str(blend));
                                    attrs_mut.set("speed", crate::entities::AttrValue::Float(speed));
                                    comp.clear_cache();
                                }
                            }

                            if response.clicked() {
                                action = TimelineAction::SelectLayer(idx);
                            }
                        })
                }).inner;

                // Check if layer order changed and emit ReorderLayer action
                if let Some(update) = dnd_response.final_update() {
                    action = TimelineAction::ReorderLayer {
                        from_idx: update.from,
                        to_idx: update.to,
                    };
                }

                // Right column: timeline bars (horizontal scroll synced with ruler)
                egui::ScrollArea::horizontal()
                    .id_salt("timeline_h_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_width(timeline_width);
                        ui.set_height(total_height);

                        // Allocate rect for timeline without hover highlight
                        let timeline_rect = Rect::from_min_size(
                            ui.cursor().min,
                            Vec2::new(timeline_width, total_height),
                        );

                        // Get interaction response for click/drag (ui.interact doesn't show hover highlight)
                        let timeline_response = ui.interact(
                            timeline_rect,
                            ui.id().with("timeline_interaction"),
                            Sense::click_and_drag(),
                        );
                        timeline_rect_global = Some(timeline_rect);

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

                            // Get child start/end from attrs (now supports negative values)
                            let attrs = comp.children_attrs.get(child_uuid);
                            let child_start = attrs.and_then(|a| Some(a.get_i32("start").unwrap_or(0))).unwrap_or(0);
                            let child_end = attrs.and_then(|a| Some(a.get_i32("end").unwrap_or(0))).unwrap_or(0);
                            let play_start = attrs.and_then(|a| Some(a.get_i32("play_start").unwrap_or(0))).unwrap_or(0);
                            let play_end = attrs.and_then(|a| Some(a.get_i32("play_end").unwrap_or(0))).unwrap_or(0);
                            let is_visible = attrs.and_then(|a| a.get_bool("visible")).unwrap_or(true);

                            // Calculate full clip range and visible (play) range
                            let full_start = child_start;
                            let full_end = child_end;
                            let visible_start = child_start + play_start;
                            let visible_end = child_end - play_end;

                            // Draw full child bar (grayed out, semi-transparent)
                            let full_bar_x_start = frame_to_screen_x(full_start as f32, timeline_rect.min.x, config, state);
                            let full_bar_x_end = frame_to_screen_x((full_end + 1) as f32, timeline_rect.min.x, config, state);
                            let full_bar_rect = Rect::from_min_max(
                                Pos2::new(full_bar_x_start, child_y + 4.0),
                                Pos2::new(full_bar_x_end, child_y + config.layer_height - 4.0),
                            );

                            // Child bar color (use hash of child_uuid for stable color)
                            let base_color = if is_visible {
                                hash_color(child_uuid)
                            } else {
                                Color32::from_gray(70)
                            };
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

                            // Get child attrs (now supports negative values)
                            let attrs = comp.children_attrs.get(child_uuid);
                            let child_start = attrs.and_then(|a| Some(a.get_i32("start").unwrap_or(0))).unwrap_or(0);
                            let child_end = attrs.and_then(|a| Some(a.get_i32("end").unwrap_or(0))).unwrap_or(0);
                            let play_start = attrs.and_then(|a| Some(a.get_i32("play_start").unwrap_or(0))).unwrap_or(0);
                            let play_end = attrs.and_then(|a| Some(a.get_i32("play_end").unwrap_or(0))).unwrap_or(0);

                            // Calculate full clip range and visible (play) range
                            let full_start = child_start;
                            let full_end = child_end;
                            let visible_start = child_start + play_start;
                            let visible_end = child_end - play_end;

                            let child_y = timeline_rect.min.y + (display_idx as f32 * config.layer_height);

                            // Create rects for full range and visible range
                            let full_bar_x_start = frame_to_screen_x(full_start as f32, timeline_rect.min.x, config, state);
                            let full_bar_x_end = frame_to_screen_x((full_end + 1) as f32, timeline_rect.min.x, config, state);
                            let full_bar_rect = Rect::from_min_max(
                                Pos2::new(full_bar_x_start, child_y + 4.0),
                                Pos2::new(full_bar_x_end, child_y + config.layer_height - 4.0),
                            );

                            let visible_bar_x_start = frame_to_screen_x(visible_start as f32, timeline_rect.min.x, config, state);
                            let visible_bar_x_end = frame_to_screen_x((visible_end + 1) as f32, timeline_rect.min.x, config, state);
                            let visible_bar_rect = Rect::from_min_max(
                                Pos2::new(visible_bar_x_start, child_y + 4.0),
                                Pos2::new(visible_bar_x_end, child_y + config.layer_height - 4.0),
                            );

                            // Manual tool detection: trim edges on full bar, move on visible bar
                            let edge_threshold = 8.0;
                            if let Some(hover_pos) = ui.ctx().input(|i| i.pointer.hover_pos()) {
                                if state.drag_state.is_none() && full_bar_rect.contains(hover_pos) {
                                    let dist_to_left = (hover_pos.x - full_bar_rect.min.x).abs();
                                    let dist_to_right = (hover_pos.x - full_bar_rect.max.x).abs();

                                    let tool = if dist_to_left < edge_threshold {
                                        Some(LayerTool::AdjustPlayStart)
                                    } else if dist_to_right < edge_threshold {
                                        Some(LayerTool::AdjustPlayEnd)
                                    } else if visible_bar_rect.contains(hover_pos) {
                                        Some(LayerTool::Move)
                                    } else {
                                        None
                                    };

                                    if let Some(tool) = tool {
                                        // Set cursor based on detected tool
                                        ui.ctx().set_cursor_icon(tool.cursor());

                                        // On mouse press, create appropriate drag state
                                        if ui.ctx().input(|i| i.pointer.primary_pressed()) {
                                            log::debug!("[TIMELINE] Creating drag state: {:?} for layer {}", tool, idx);
                                            if let Some(child_attrs) = attrs {
                                                state.drag_state = Some(tool.to_drag_state(idx, child_attrs, hover_pos));
                                                log::debug!("[TIMELINE] Drag state created successfully");
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

                                        // Visual feedback: draw ghost bar at new position
                                        // Use display index for Y positioning (already calculated above)
                                        let child_uuid = &comp.children[*layer_idx];
                                        if let Some(attrs) = comp.children_attrs.get(child_uuid) {
                                            let ghost_child_y = timeline_rect.min.y + (target_display_idx as f32 * config.layer_height);
                                            let duration = (attrs.get_i32("end").unwrap_or(0)
                                                          - attrs.get_i32("start").unwrap_or(0)).max(0);

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

                                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);

                                        // On release, commit both horizontal and vertical movements
                                        if ui.ctx().input(|i| i.pointer.any_released()) {
                                            let has_horizontal_move = new_start != *initial_start;
                                            let has_vertical_move = target_child != *layer_idx;

                                            log::debug!("[TIMELINE] Layer drag released: h_move={}, v_move={}, layer_idx={}, new_start={}, new_idx={}",
                                                has_horizontal_move, has_vertical_move, layer_idx, new_start, target_child);

                                            if has_horizontal_move || has_vertical_move {
                                                action = TimelineAction::MoveAndReorderLayer {
                                                    layer_idx: *layer_idx,
                                                    new_start,
                                                    new_idx: target_child,
                                                };
                                                log::debug!("[TIMELINE] Emitting MoveAndReorderLayer action");
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
                                                // Use display index for Y positioning
                                                let display_idx = physical_to_display(*layer_idx).unwrap_or(*layer_idx);
                                                let layer_y = timeline_rect.min.y + (display_idx as f32 * config.layer_height);
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
                                                // Use display index for Y positioning
                                                let display_idx = physical_to_display(*layer_idx).unwrap_or(*layer_idx);
                                                let layer_y = timeline_rect.min.y + (display_idx as f32 * config.layer_height);
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

                        // Check for drag'n'drop from Project Window using global drag state
                        let global_drag: Option<GlobalDragState> = ui.ctx().data(|data| {
                            data.get_temp(egui::Id::new("global_drag_state"))
                        });

                        if let Some(GlobalDragState::ProjectItem { source_uuid, display_name, duration, .. }) = global_drag {
                            // Show drop preview (shadow bar)
                            if let Some(hover_pos) = ui.ctx().input(|i| i.pointer.hover_pos()) {
                                // Treat full vertical span of timeline area as drop zone; only X matters.
                                if hover_pos.x >= timeline_rect.min.x && hover_pos.x <= timeline_rect.max.x {
                                    let drop_frame = screen_x_to_frame(hover_pos.x, timeline_rect.min.x, config, state).round() as i32;
                                    let drop_duration = duration.unwrap_or(100);

                                    // Calculate bar position and size
                                    let start_x = frame_to_screen_x(drop_frame as f32, timeline_rect.min.x, config, state);
                                    let end_x = frame_to_screen_x((drop_frame + drop_duration) as f32, timeline_rect.min.x, config, state);
                                    let bar_width = (end_x - start_x).max(2.0);

                                    // Draw semi-transparent shadow bar
                                    let shadow_rect = Rect::from_min_size(
                                        Pos2::new(start_x, timeline_rect.min.y),
                                        Vec2::new(bar_width, timeline_rect.height()),
                                    );
                                    painter.rect_filled(
                                        shadow_rect,
                                        2.0,
                                        Color32::from_rgba_premultiplied(100, 220, 255, 60), // Semi-transparent cyan
                                    );

                                    // Draw left edge line (brighter)
                                    painter.line_segment(
                                        [Pos2::new(start_x, timeline_rect.min.y), Pos2::new(start_x, timeline_rect.max.y)],
                                        (2.0, Color32::from_rgb(100, 220, 255)),
                                    );

                                    // Draw name label inside shadow bar
                                    if bar_width > 40.0 {
                                        let label_pos = Pos2::new(start_x + 4.0, timeline_rect.min.y + 4.0);
                                        painter.text(
                                            label_pos,
                                            egui::Align2::LEFT_TOP,
                                            display_name,
                                            egui::FontId::proportional(11.0),
                                            Color32::from_rgb(200, 240, 255),
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
                                      for (display_idx, &original_idx) in child_order.iter().enumerate() {
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
                                          // Click on empty space: clear selection and scrub timeline
                                          state.selected_layer = None;
                                          let frame = screen_x_to_frame(pos.x, timeline_rect.min.x, config, state).round() as i32;
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

    // Draw playhead once across ruler + bars
    if let (Some(ruler_rect), Some(timeline_rect)) = (ruler_rect, timeline_rect_global) {
        let painter = ui.painter();
        let x = frame_to_screen_x(comp.current_frame as f32, ruler_rect.min.x, config, state);
        painter.line_segment(
            [Pos2::new(x, ruler_rect.min.y), Pos2::new(x, timeline_rect.max.y)],
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

    action
}


