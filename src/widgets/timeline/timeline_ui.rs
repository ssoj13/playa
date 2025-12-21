//! After Effects-style timeline - UI rendering
//!
//! Each layer is displayed as a row showing:
//! - Layer name / clip name
//! - Start..End range as horizontal bar
//! - Visual indication of current_frame (playhead)
//!
//! # View Modes
//!
//! Timeline supports three view modes (buttons in toolbar):
//! - **Split**: Outliner on left, Canvas on right (using `Frame::NONE` for alignment)
//! - **Outliner**: Full-width outline view only
//! - **Layers**: Full-width canvas/layers view only
//!
//! # Interactions
//!
//! - **Click**: Select layer (with Shift/Ctrl for multi-select)
//! - **Double-click**: Dive into source comp (activates the layer's source)
//! - **Drag**: Move layer position or reorder
//! - **Edge drag**: Trim in/out points
//!
//! # Architecture
//!
//! Consumed by: `ui::render_timeline_panel`. Emits events through
//! dispatch closures to EventBus, driven by shared `TimelineState` from
//! `timeline.rs` and helper routines in `timeline_helpers.rs`. Data flow:
//! egui input → dispatch(BoxedEvent) → EventBus → Project/Comp mutations.

use super::timeline_helpers::{
    detect_layer_tool_with_geom, draw_drop_preview, draw_frame_ruler,
    frame_to_screen_x, hash_color_str, row_to_y, screen_x_to_frame,
};
use super::{GlobalDragState, TimelineConfig, TimelineState};
use crate::entities::{Comp, Node, frame::FrameStatus};
use crate::core::event_bus::BoxedEvent;
use crate::core::player_events::{JumpToStartEvent, JumpToEndEvent, TogglePlayPauseEvent, StopEvent, SetFrameEvent, SetLoopEvent};
use super::TimelineViewMode;
use crate::widgets::project::project_events::{ProjectActiveChangedEvent, SelectionFocusEvent};
use crate::entities::comp_events::{
    AddLayerEvent, CompSelectionChangedEvent, LayerAttributesChangedEvent,
    MoveAndReorderLayerEvent, ReorderLayerEvent, SetLayerPlayEndEvent, SetLayerPlayStartEvent,
    SlideLayerEvent,
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

/// Render timeline toolbar (transport controls, zoom, snap, loop, view mode)
pub fn render_toolbar(
    ui: &mut Ui,
    state: &mut TimelineState,
    loop_enabled: bool,
    show_tooltips: bool,
    mut dispatch: impl FnMut(BoxedEvent),
) {
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

        // Zoom controls - fixed max width to leave room for buttons/checkboxes
        ui.label("Zoom:");
        ui.spacing_mut().slider_width = 500.0;
        let zoom_response = ui.add(
            egui::Slider::new(&mut state.zoom, 0.1..=20.0)
                .fixed_decimals(2),
        );
        if zoom_response.changed() {
            dispatch(Box::new(TimelineZoomChangedEvent(state.zoom)));
        }
        if ui.button("Reset").on_hover_text("Reset Zoom to 1.0").clicked() {
            state.zoom = 1.0;
            dispatch(Box::new(TimelineZoomChangedEvent(1.0)));
        }
        if ui.button("Fit").on_hover_text("Fit all clips to view").clicked() {
            dispatch(Box::new(TimelineFitAllEvent(state.last_canvas_width)));
        }

        // Snap checkbox with optional tooltip (2s delay)
        let snap_response = ui.checkbox(&mut state.snap_enabled, "Snap");
        if snap_response.changed() {
            dispatch(Box::new(TimelineSnapChangedEvent(state.snap_enabled)));
        }
        if show_tooltips {
            snap_response.on_hover_text_at_pointer("Snap to frame edges when dragging layers");
        }

        // Lock checkbox with optional tooltip (2s delay)
        let lock_response = ui.checkbox(&mut state.lock_work_area, "Lock");
        if lock_response.changed() {
            dispatch(Box::new(TimelineLockWorkAreaChangedEvent(state.lock_work_area)));
        }
        if show_tooltips {
            lock_response.on_hover_text_at_pointer("Lock work area markers (B/N keys)");
        }

        // Loop checkbox with optional tooltip (2s delay)
        let mut loop_state = loop_enabled;
        let loop_response = ui.checkbox(&mut loop_state, "Loop");
        if loop_response.changed() {
            dispatch(Box::new(SetLoopEvent(loop_state)));
        }
        if show_tooltips {
            loop_response.on_hover_text_at_pointer("Loop playback within work area (` key)");
        }

        ui.separator();

        // View mode selector (moved from ui.rs)
        for (label, mode) in [
            ("Split", TimelineViewMode::Split),
            ("Outliner", TimelineViewMode::OutlineOnly),
            ("Layers", TimelineViewMode::CanvasOnly),
        ] {
            if ui.selectable_label(state.view_mode == mode, label).clicked() {
                state.view_mode = mode;
            }
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

    // Match the top padding of the timeline canvas (ruler + status bar + spacing)
    // Must be OUTSIDE ScrollArea to stay in sync with canvas
    // Note: Both panels now use Frame::none() so no offset compensation needed
    let status_bar_height = 2.0; // Status strip is always shown
    ui.add_space(20.0 + status_bar_height + 4.0);

    // Render layer list with DnD inside a ScrollArea to avoid growing the parent panel.
    let mut child_order: Vec<usize> = (0..comp.layers.len()).collect();
    let dnd_response = egui::ScrollArea::vertical()
        .id_salt("timeline_layers_scroll") // share scroll with canvas
        .max_height(ui.available_height())
        .show(ui, |ui| {
            // Zero out spacing to match canvas side
            ui.spacing_mut().item_spacing.y = 0.0;
            dnd(ui, "timeline_child_names_outline").show_vec(
                    &mut child_order,
                    |ui, child_idx, handle, _state| {
                        let idx = *child_idx;
                        let layer = &comp.layers[idx];
                        let child_uuid = layer.uuid();
                        let attrs = &layer.attrs;

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

                        // Consume DnD handle without rendering (reorder via canvas DnD)
                        let _ = handle;

                        let mut visible = attrs.get_bool("visible").unwrap_or(true);
                        let mut solo = attrs.get_bool("solo").unwrap_or(false);
                        let mut opacity = attrs.get_float("opacity").unwrap_or(1.0);
                        let prev_blend = attrs
                            .get_str("blend_mode")
                            .unwrap_or("normal")
                            .to_string();
                        let mut blend = prev_blend.clone();
                        let mut speed = attrs.get_float("speed").unwrap_or(1.0);
                        let mut dirty = false;

                        // Visible checkbox (20px)
                        row_ui.allocate_ui_with_layout(
                            egui::Vec2::new(20.0, config.layer_height),
                            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                            |ui| {
                                if ui.checkbox(&mut visible, "").changed() {
                                    dirty = true;
                                }
                            },
                        );
                        // Solo checkbox (20px) - yellow when active
                        row_ui.allocate_ui_with_layout(
                            egui::Vec2::new(20.0, config.layer_height),
                            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                            |ui| {
                                let resp = ui.checkbox(&mut solo, "");
                                if solo {
                                    ui.painter().rect_filled(
                                        resp.rect.shrink(2.0),
                                        2.0,
                                        egui::Color32::from_rgb(200, 180, 50),
                                    );
                                }
                                if resp.changed() {
                                    dirty = true;
                                }
                            },
                        );

                        let child_name = attrs
                            .get_str("name")
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| child_uuid.to_string());
                        // Name column with configurable width
                        row_ui.allocate_ui_with_layout(
                            egui::Vec2::new(config.name_column_width, config.layer_height),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.set_min_width(config.name_column_width);
                                ui.add(egui::Label::new(child_name).truncate());
                            },
                        );

                        // Fixed-width opacity slider for column alignment
                        row_ui.allocate_ui_with_layout(
                            egui::Vec2::new(60.0, config.layer_height),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                if ui
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
                            },
                        );

                        // Fixed-width blend mode combo (90px)
                        row_ui.allocate_ui_with_layout(
                            egui::Vec2::new(90.0, config.layer_height),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                egui::ComboBox::from_id_salt(format!("blend_outline_{}", child_uuid))
                                    .width(80.0)
                                    .selected_text(blend.clone())
                                    .show_ui(ui, |ui| {
                                        for mode in [
                                            "normal", "screen", "add", "subtract",
                                            "multiply", "divide", "difference", "overlay",
                                        ] {
                                            ui.selectable_value(&mut blend, mode.to_string(), mode);
                                        }
                                    });
                            },
                        );
                        if blend != prev_blend {
                            dirty = true;
                        }

                        // Fixed-width speed control for column alignment
                        row_ui.allocate_ui_with_layout(
                            egui::Vec2::new(50.0, config.layer_height),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut speed)
                                            .speed(0.1)
                                            .range(0.1..=4.0),
                                    )
                                    .changed()
                                {
                                    dirty = true;
                                }
                            },
                        );

                        if dirty {
                            // Apply to all selected layers if this layer is selected
                            let targets = if comp.layer_selection.contains(&child_uuid) {
                                comp.layer_selection.clone()
                            } else {
                                vec![child_uuid]
                            };
                            dispatch(Box::new(LayerAttributesChangedEvent {
                                comp_uuid: comp_id,
                                layer_uuids: targets,
                                visible,
                                solo,
                                opacity,
                                blend_mode: blend,
                                speed,
                            }));
                        }

                        if response.clicked() {
                            let modifiers = ui.input(|i| i.modifiers);
                            let clicked_uuid = child_uuid;
                            let children_uuids = comp.layers_uuids_vec();
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
                                selection: selection.clone(),
                                anchor,
                            }));
                            dispatch(Box::new(SelectionFocusEvent(selection)));
                        }

                        // Double-click: dive into source comp
                        if response.double_clicked() {
                            // Convert parent frame to child comp frame
                            let parent_frame = comp.frame();
                            let local_frame = layer.parent_to_local(parent_frame);
                            // Child comp's "in" will be added in the handler
                            dispatch(Box::new(ProjectActiveChangedEvent::with_frame(
                                layer.source_uuid(),
                                local_frame,
                            )));
                        }
                    },
                )
        })
        .inner;

    if let Some(update) = dnd_response.final_update() {
        dispatch(Box::new(ReorderLayerEvent {
            comp_uuid: comp_id,
            from_idx: update.from,
            to_idx: update.to,
        }));
    }

    // Handle click on empty area below layers to clear selection
    let remaining_height = ui.available_height();
    if remaining_height > 0.0 {
        let (empty_rect, empty_response) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), remaining_height),
            Sense::click(),
        );
        // Only left-click clears selection (not right-click for context menu)
        if empty_response.clicked_by(egui::PointerButton::Primary) {
            log::trace!("Empty area clicked, clearing {} selected layers", comp.layer_selection.len());
            dispatch(Box::new(CompSelectionChangedEvent {
                comp_uuid: comp_id,
                selection: vec![],
                anchor: None,
            }));
            dispatch(Box::new(SelectionFocusEvent(vec![])));
        }
        // Visual feedback: subtle highlight on hover
        if empty_response.hovered() {
            ui.painter().rect_filled(
                empty_rect,
                0.0,
                ui.visuals().widgets.hovered.bg_fill.gamma_multiply(0.3),
            );
        }
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

    // Calculate extended range to include all layer positions (even beyond comp bounds)
    let (layers_min, layers_max) = comp.layers.iter().fold((comp_start, comp_end), |(min, max), layer| {
        let start = layer.attrs.full_bar_start();
        let end = layer.attrs.full_bar_end();
        (min.min(start), max.max(end))
    });
    // Large margin for smooth dragging - scales with visible area
    let margin = (ui.available_width() / (config.pixels_per_frame * state.zoom)).ceil() as i32 + 100;
    let extended_min = layers_min.min(comp_start) - margin;
    let extended_max = layers_max.max(comp_end) + margin;
    let total_frames = (extended_max - extended_min + 1).max(100);

    // Spammy per-frame log, use trace level
    log::trace!(
        "Comp '{}': start={}, end={}, layers={}..{}, total_frames={}",
        comp.name(),
        comp_start,
        comp_end,
        layers_min,
        layers_max,
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

    // Note: row = layer index (simple 1:1 mapping, no packing)

    let ruler_width =
        (total_frames as f32 * config.pixels_per_frame * state.zoom).max(ui.available_width());

    // Spammy per-frame log, use trace level
    log::trace!(
        "ruler_width={}, timeline_width={}, available_width={}",
        ruler_width,
        timeline_width,
        ui.available_width()
    );
    let status_strip = comp.cache_frame_statuses(project.global_cache.as_ref());
    let status_bar_height = 2.0; // Status strip is always shown

    // Options + time ruler row (always visible)
    let mut ruler_rect: Option<Rect> = None;
    let mut timeline_rect_global: Option<Rect> = None;
    let ruler_height = 20.0;
    let mut timeline_hovered = false; // Track hover state for input routing
    let tab_rect = ui.max_rect(); // Full tab rect for hover detection

    // Draw ruler with proper layout sync
    // Zero item_spacing to match outline panel alignment
    let saved_spacing = ui.spacing().item_spacing.y;
    ui.spacing_mut().item_spacing.y = 0.0;

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
        if rect.contains(ui.ctx().pointer_hover_pos().unwrap_or(Pos2::ZERO))
            && ui
                .ctx()
                .input(|i| i.pointer.button_down(egui::PointerButton::Middle))
                && state.drag_state.is_none()
                && let Some(pos) = ui.ctx().pointer_hover_pos() {
                    state.drag_state = Some(GlobalDragState::TimelinePan {
                        drag_start_pos: pos,
                        initial_pan_offset: state.pan_offset,
                    });
                }
    });

    // Status strip (if present) - draw inside horizontal layout to align with ruler
    if let Some(statuses) = &status_strip
        && let Some(ruler) = ruler_rect {
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

    ui.add_space(4.0);

    // Restore spacing for layers area
    ui.spacing_mut().item_spacing.y = saved_spacing;

    // Layers area with vertical scroll (but horizontal pan via state.pan_offset)
    // ScrollArea is needed here because layers can extend beyond visible area vertically
    egui::ScrollArea::vertical()
        .id_salt("timeline_layers_scroll")
        // Constrain ScrollArea to visible space so the parent panel doesn't grow
        .max_height(ui.available_height())
        .show(ui, |ui| {
        ui.push_id("timeline_layers", |ui| {
            // child_order computed here, row = index (no smart packing)

            // Simple row assignment: each layer gets its own row based on index
            // No "smart" packing - layer order in children = visual row order
            let child_order_inner: Vec<usize> = (0..comp.layers.len()).collect();
            let num_layers = comp.layers.len();
            let total_height_inner = (num_layers.max(1) as f32) * config.layer_height;

            // Timeline bars - horizontal pan via state.pan_offset, vertical scroll via ScrollArea.
            let (timeline_rect, timeline_response) = ui.allocate_exact_size(
                Vec2::new(timeline_width, total_height_inner),
                Sense::click_and_drag(),
            );
            let painter = ui.painter();

        // Get interaction response for click/drag (ui.interact doesn't show hover highlight)
        timeline_rect_global = Some(timeline_rect);
        timeline_hovered = timeline_response.hovered();

        // If mouse is over ruler or canvas rects, mark timeline hovered (hotkeys context)
        if let Some(pos) = ui.ctx().pointer_hover_pos()
            && (ruler_rect.map(|r| r.contains(pos)).unwrap_or(false)
                || timeline_rect.contains(pos))
            {
                timeline_hovered = true;
            }

        // Middle-drag pan on canvas - initialize only if not already dragging
        if timeline_response.hovered() && state.drag_state.is_none()
            && ui.ctx().input(|i| i.pointer.button_down(egui::PointerButton::Middle))
                && let Some(pos) = ui.ctx().pointer_hover_pos() {
                    state.drag_state = Some(GlobalDragState::TimelinePan {
                        drag_start_pos: pos,
                        initial_pan_offset: state.pan_offset,
                    });
                }

        // Scroll wheel horizontal pan
        let scroll_delta = ui.ctx().input(|i| i.smooth_scroll_delta);
        if scroll_delta.x.abs() > 0.0 {
            let delta_frames = scroll_delta.x / (config.pixels_per_frame * state.zoom);
            dispatch(Box::new(TimelinePanChangedEvent(state.pan_offset - delta_frames)));
        }

        // Draw layers (egui automatically clips to visible area inside ScrollArea)
                        // Cache LayerGeom results to avoid recalculating in interaction pass
                        let pan_offset = state.pan_offset;
                        let zoom = state.zoom;
                        state.geom_cache.clear();
                        state.geom_cache.reserve(child_order_inner.len());

                        // First pass: draw row backgrounds (alternating colors)
                        for row in 0..num_layers {
                            let row_y = row_to_y(row, config, timeline_rect);
                            let row_rect = Rect::from_min_size(
                                Pos2::new(timeline_rect.min.x, row_y),
                                Vec2::new(timeline_width, config.layer_height),
                            );
                            let bg_color = if row % 2 == 0 {
                                Color32::from_gray(30)
                            } else {
                                Color32::from_gray(35)
                            };
                            painter.rect_filled(row_rect, 0.0, bg_color);
                        }

                        // Second pass: draw layer bars
                        for &original_idx in child_order_inner.iter() {
                            let idx = original_idx;
                            let layer = &comp.layers[idx];
                            let child_uuid = layer.uuid();
                            let attrs = &layer.attrs;

                            // Get full bar start/end (computed from in + src_len/speed)
                            let child_start = attrs.full_bar_start();
                            let child_end = attrs.full_bar_end();

                            // Get precomputed row from layout
                            let row = idx;  // Simple: row = layer index
                            let child_y = row_to_y(row, config, timeline_rect);
                            let play_start = attrs.layer_start();
                            let play_end = attrs.layer_end();
                            let is_visible = attrs.get_bool("visible").unwrap_or(true);

                            // Calculate layer geometry and cache for interaction pass
                            let geom = super::timeline::LayerGeom::calc(
                                child_start, child_end, play_start, play_end,
                                child_y, timeline_rect, config, pan_offset, zoom
                            );
                            state.geom_cache.insert(idx, geom);

                            // Child bar color (use hash of name for stable color per clip)
                            let child_name = attrs.get_str("name").unwrap_or("?");
                            let base_color = if is_visible {
                                hash_color_str(child_name)
                            } else {
                                Color32::from_gray(70)
                            };
                            let is_selected = comp.layer_selection.contains(&child_uuid);
                            let gray_color = if is_selected {
                                // Slightly brighter grey with a blue tint when selected
                                Color32::from_rgba_unmultiplied(110, 140, 190, 130)
                            } else {
                                Color32::from_rgba_unmultiplied(80, 80, 80, 100)
                            };

                            painter.rect_filled(geom.full_bar_rect, 4.0, gray_color);

                            // Draw visible (trimmed) area with full color on top
                            if let Some(visible_bar_rect) = geom.visible_bar_rect {
                                // Check if source is file node (for hatching)
                                let is_source_file = attrs.get_uuid("uuid")
                                    .and_then(|source_uuid| project.with_node(source_uuid, |n| n.is_file()))
                                    .unwrap_or(false);

                                if is_source_file {
                                    // File comp: draw with diagonal hatch pattern (texture * base_color)
                                    let hatch_id = state.get_hatch_texture(ui.ctx());
                                    let tex_size = 64.0; // Texture size in pixels
                                    // UV relative to bar size for proper tiling
                                    let uv = Rect::from_min_max(
                                        Pos2::new(0.0, 0.0),
                                        Pos2::new(visible_bar_rect.width() / tex_size, visible_bar_rect.height() / tex_size),
                                    );
                                    painter.image(hatch_id, visible_bar_rect, uv, base_color);
                                } else {
                                    // Layer comp: solid color
                                    painter.rect_filled(visible_bar_rect, 4.0, base_color);
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
                              let stroke_width = 1.0; // Always 1px stroke
                              painter.rect_stroke(
                                  geom.full_bar_rect,
                                  4.0,
                                  egui::Stroke::new(stroke_width, stroke_color),
                                  egui::epaint::StrokeKind::Middle,
                              );
                        }

                        // Handle child bar interactions using proper response system
                        // We need to do this in a second pass after drawing to ensure responses are on top
                        for &original_idx in child_order_inner.iter() {
                            let idx = original_idx;
                            let layer = &comp.layers[idx];
                            let child_uuid = layer.uuid();
                            let attrs = &layer.attrs;

                            // Use cached geometry from draw pass
                            let Some(&geom) = state.geom_cache.get(&idx) else { continue };

                            // Tool detection with full geometry support (including Slide in trim zones)
                            let edge_threshold = 8.0;
                            // Use interact_pos which is in local UI coordinates (accounts for scroll)
                            if let Some(local_pos) = ui.input(|i| i.pointer.interact_pos()) {
                                // Double-click: dive into source comp (check on full bar)
                                if geom.full_bar_rect.contains(local_pos)
                                    && ui.ctx().input(|i| i.pointer.button_double_clicked(egui::PointerButton::Primary)) {
                                        // Convert parent frame to child comp frame
                                        let parent_frame = comp.frame();
                                        let local_frame = layer.parent_to_local(parent_frame);
                                        dispatch(Box::new(ProjectActiveChangedEvent::with_frame(
                                            layer.source_uuid(),
                                            local_frame,
                                        )));
                                    }

                                // Tool detection with geometry-aware function (supports Slide in trim zones)
                                if state.drag_state.is_none()
                                    && let Some(tool) = detect_layer_tool_with_geom(
                                        local_pos,
                                        geom.full_bar_rect,
                                        geom.visible_bar_rect,
                                        edge_threshold,
                                    ) {
                                        ui.ctx().set_cursor_icon(tool.cursor());

                                        // On mouse press, create appropriate drag state
                                        if ui.ctx().input(|i| i.pointer.primary_pressed()) {
                                            log::trace!(
                                                "[TIMELINE] Creating drag state: {:?} for layer {}",
                                                tool, idx
                                            );
                                            // Ensure selection switches to dragged layer if it wasn't selected
                                            {
                                                let modifiers = ui.ctx().input(|i| i.modifiers);
                                                let multi = modifiers.ctrl || modifiers.shift || modifiers.command;
                                                if !multi && !comp.layer_selection.contains(&child_uuid) {
                                                    dispatch(Box::new(CompSelectionChangedEvent {
                                                        comp_uuid: comp_id,
                                                        selection: vec![child_uuid],
                                                        anchor: Some(child_uuid),
                                                    }));
                                                    dispatch(Box::new(SelectionFocusEvent(vec![child_uuid])));
                                                }
                                            }
                                            {
                                                state.drag_state =
                                                    Some(tool.to_drag_state(idx, attrs, local_pos));
                                                log::trace!(
                                                    "[TIMELINE] Drag state created successfully"
                                                );
                                            }
                                        }
                                    }
                            }
                        }

                        // Helper: find display index for a physical layer index
                        let physical_to_display = |physical_idx: usize| -> Option<usize> {
                            child_order_inner.iter().position(|&idx| idx == physical_idx)
                        };

                        // Process active drag operations
                        // Use latest_pos() instead of hover_pos() to track cursor even outside window
                        if let Some(drag) = state.drag_state.take() {
                            let mut keep_drag = true;
                            if let Some(current_pos) = ui.ctx().input(|i| i.pointer.latest_pos()) {
                                match &drag {
                                    GlobalDragState::TimelinePan { drag_start_pos, initial_pan_offset } => {
                                        let delta_x = current_pos.x - drag_start_pos.x;
                                        let delta_frames = delta_x / (config.pixels_per_frame * state.zoom);
                                        let new_pan = initial_pan_offset - delta_frames;

                                        // Update state directly to avoid frame delay
                                        state.pan_offset = new_pan;
                                        dispatch(Box::new(TimelinePanChangedEvent(new_pan)));

                                        if ui.ctx().input(|i| i.pointer.any_released()) {
                                            keep_drag = false;
                                        }
                                    }
                                    GlobalDragState::MovingLayer { layer_idx, initial_start, drag_start_x, drag_start_y, .. } => {
                                        let delta_x = current_pos.x - drag_start_x;
                                        let delta_y = current_pos.y - drag_start_y;
                                        let delta_frames = (delta_x / (config.pixels_per_frame * state.zoom)).round() as i32;
                                        let new_start = *initial_start + delta_frames;  // Allow negative values

                                        // Determine target child index from vertical position
                                        // Calculate from display position, then convert to physical
                                        let current_display_idx = physical_to_display(*layer_idx).unwrap_or(*layer_idx);
                                        let delta_children = (delta_y / config.layer_height).round() as i32;
                                        let target_display_idx = (current_display_idx as i32 + delta_children).max(0).min(comp.layers.len() as i32 - 1) as usize;
                                        let target_child = child_order_inner.get(target_display_idx).copied().unwrap_or(*layer_idx);

                                        // Visual feedback: draw ghost bars for all selected (or just dragged) layers
                                        let dragged_uuid = comp.layers.get(*layer_idx).map(|l| l.uuid()).unwrap_or_default();
                                        let selection = if comp.layer_selection.contains(&dragged_uuid) {
                                            comp.layer_selection.clone()
                                        } else {
                                            vec![dragged_uuid]
                                        };

                                        for child_uuid in selection {
                                            if let Some(attrs) = comp.layers_attrs_get(&child_uuid) {
                                                let idx_sel = comp.uuid_to_idx(child_uuid).unwrap_or(0);
                                                let current_row = idx_sel;  // row = layer index
                                                let target_row = (current_row as i32 + delta_children)
                                                    .clamp(0, comp.layers.len().saturating_sub(1) as i32)
                                                    as usize;

                                                let ghost_child_y = row_to_y(target_row, config, timeline_rect);
                                                let duration = (attrs.full_bar_end() - attrs.full_bar_start() + 1).max(1);

                                                // Apply same delta to maintain relative offsets
                                                let child_start = attrs.full_bar_start();
                                                let ghost_start = child_start + delta_frames;

                                                draw_drop_preview(
                                                    painter,
                                                    ghost_start,
                                                    ghost_child_y,
                                                    duration,
                                                    timeline_rect,
                                                    config,
                                                    state,
                                                    false, // Not a cycle - moving existing layer
                                                );
                                            }
                                        }

                                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);

                                        // On release, commit both horizontal and vertical movements
                                        if ui.ctx().input(|i| i.pointer.any_released()) {
                                            let has_horizontal_move = new_start != *initial_start;
                                            let has_vertical_move = target_child != *layer_idx;

                                            log::trace!("[TIMELINE] Layer drag released: h_move={}, v_move={}, layer_idx={}, new_start={}, new_idx={}",
                                                has_horizontal_move, has_vertical_move, layer_idx, new_start, target_child);

                                            if has_horizontal_move || has_vertical_move {
                                                log::trace!(
                                                    "[TIMELINE] DRAG: layer_idx={} -> new_start={}, new_idx={} (h={}, v={})",
                                                    layer_idx, new_start, target_child, has_horizontal_move, has_vertical_move
                                                );
                                                dispatch(Box::new(MoveAndReorderLayerEvent {
                                                    comp_uuid: comp_id,
                                                    layer_idx: *layer_idx,
                                                    new_start,
                                                    new_idx: target_child,
                                                }));
                                            }
                                            keep_drag = false;
                                        }
                                    }
                                    GlobalDragState::AdjustPlayStart { layer_idx, initial_play_start, drag_start_x } => {
                                        let delta_x = current_pos.x - drag_start_x;
                                        let delta_frames = (delta_x / (config.pixels_per_frame * state.zoom)).round() as i32;
                                        let new_play_start = *initial_play_start + delta_frames;

                                        // Visual feedback: draw ghost play range preview
                                        if let Some(layer) = comp.layers.get(*layer_idx) {
                                            // row = layer index
                                            let target_row = *layer_idx;
                                            let layer_y = row_to_y(target_row, config, timeline_rect);
                                            let visual_start = new_play_start as f32;
                                            let layer_end = layer.attrs.full_bar_end() as f32;
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
                                            keep_drag = false;
                                        }
                                    }
                                    GlobalDragState::AdjustPlayEnd { layer_idx, initial_play_end, drag_start_x } => {
                                        let delta_x = current_pos.x - drag_start_x;
                                        let delta_frames = (delta_x / (config.pixels_per_frame * state.zoom)).round() as i32;
                                        // play_end is ABSOLUTE source frame, so drag right = increase
                                        let new_play_end = *initial_play_end + delta_frames;

                                        // Visual feedback: draw ghost play range preview
                                        if let Some(layer) = comp.layers.get(*layer_idx) {
                                            // row = layer index
                                            let target_row = *layer_idx;
                                            let layer_y = row_to_y(target_row, config, timeline_rect);
                                            let play_start = layer.attrs.layer_start();
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
                                            keep_drag = false;
                                        }
                                    }
                                    GlobalDragState::SlidingLayer {
                                        layer_idx, initial_in, initial_trim_in, initial_trim_out, speed, drag_start_x
                                    } => {
                                        let delta_x = current_pos.x - drag_start_x;
                                        let delta_frames = (delta_x / (config.pixels_per_frame * state.zoom)).round() as i32;

                                        // Slide: move full bar while keeping visible content (layer_start & layer_end) in place
                                        //
                                        // Geometry:
                                        //   layer_start = in + trim_in/speed
                                        //   layer_end = layer_start + (src_len - trim_in - trim_out)/speed - 1
                                        //
                                        // When in changes by delta:
                                        //   - To keep layer_start fixed: trim_in must change by -delta*speed
                                        //   - To keep visible_src (and thus layer_end) fixed:
                                        //     visible_src = src_len - trim_in - trim_out = const
                                        //     If trim_in decreases by X, trim_out must increase by X
                                        //
                                        let new_in = *initial_in + delta_frames;
                                        let trim_delta = (delta_frames as f32 * speed).round() as i32;
                                        // trim_in decreases when sliding right (delta > 0)
                                        let new_trim_in = (*initial_trim_in - trim_delta).max(0);
                                        // trim_out increases by same amount to keep visible_src constant
                                        let new_trim_out = (*initial_trim_out + trim_delta).max(0);

                                        // Visual feedback: draw ghost full bar at new position
                                        if let Some(layer) = comp.layers.get(*layer_idx) {
                                            let target_row = *layer_idx;  // row = layer index
                                            let layer_y = row_to_y(target_row, config, timeline_rect);
                                            let src_len = layer.attrs.src_len();
                                            let new_full_bar_end = new_in + (src_len as f32 / speed).ceil() as i32 - 1;
                                            let ghost_x_start = frame_to_screen_x(new_in as f32, timeline_rect.min.x, config, state);
                                            let ghost_x_end = frame_to_screen_x((new_full_bar_end + 1) as f32, timeline_rect.min.x, config, state);

                                            let ghost_rect = Rect::from_min_max(
                                                Pos2::new(ghost_x_start, layer_y + 4.0),
                                                Pos2::new(ghost_x_end, layer_y + config.layer_height - 4.0),
                                            );
                                            painter.rect_stroke(
                                                ghost_rect,
                                                4.0,
                                                egui::Stroke::new(2.0, Color32::from_rgba_unmultiplied(255, 180, 100, 200)),
                                                egui::epaint::StrokeKind::Middle,
                                            );
                                        }

                                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeColumn);

                                        if ui.ctx().input(|i| i.pointer.any_released()) {
                                            log::trace!(
                                                "[SLIDE COMMIT] delta={}, in: {}→{}, trim_in: {}→{}, trim_out: {}→{}",
                                                delta_frames, initial_in, new_in,
                                                initial_trim_in, new_trim_in, initial_trim_out, new_trim_out
                                            );
                                            dispatch(Box::new(SlideLayerEvent {
                                                comp_uuid: comp_id,
                                                layer_idx: *layer_idx,
                                                new_in,
                                                new_trim_in,
                                                new_trim_out,
                                            }));
                                            keep_drag = false;
                                        }
                                    }
                                    // Other drag states are handled elsewhere (ProjectItem, TimelineScrub)
                                    _ => {}
                                }
                            }

                            // Cancel drag on escape
                            if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
                                keep_drag = false;
                            }

                            if keep_drag {
                                state.drag_state = Some(drag);
                            }
                        }

                        // Draw work area overlay (darken regions outside play_range)
                        let (play_start, play_end) = comp.play_range(true);

                        // Spammy per-frame log, use trace level
                        log::trace!("work_area: play=[{}..{}]", play_start, play_end);

                        // Darken everything outside play_range across the full visible width.
                        let left_edge = timeline_rect.min.x;
                        let right_edge = timeline_rect.max.x;
                        let play_start_x = frame_to_screen_x(play_start as f32, timeline_rect.min.x, config, state);
                        let play_end_x = frame_to_screen_x((play_end + 1) as f32, timeline_rect.min.x, config, state);

                        if play_start_x > left_edge {
                            let overlay_rect = Rect::from_min_max(
                                Pos2::new(left_edge, timeline_rect.min.y),
                                Pos2::new(play_start_x, timeline_rect.max.y),
                            );
                            painter.rect_filled(overlay_rect, 0.0, Color32::from_rgba_unmultiplied(0, 0, 0, 100));
                        }

                        if play_end_x < right_edge {
                            let overlay_rect = Rect::from_min_max(
                                Pos2::new(play_end_x, timeline_rect.min.y),
                                Pos2::new(right_edge, timeline_rect.max.y),
                            );
                            painter.rect_filled(overlay_rect, 0.0, Color32::from_rgba_unmultiplied(0, 0, 0, 100));
                        }

                        // Check for drag'n'drop from Project Window using global drag state
                        let global_drag: Option<GlobalDragState> = ui.ctx().data(|data| {
                            data.get_temp(egui::Id::new("global_drag_state"))
                        });

                        if let Some(GlobalDragState::ProjectItem { source_uuid, duration, .. }) = global_drag {
                            // Use mouse Y position directly, adjust only for time overlap
                            if let Some(hover_pos) = ui.ctx().input(|i| i.pointer.hover_pos())
                                && hover_pos.x >= timeline_rect.min.x && hover_pos.x <= timeline_rect.max.x {
                                    let raw_drop_frame = screen_x_to_frame(hover_pos.x, timeline_rect.min.x, config, state).round() as i32;
                                    let dur = duration.unwrap_or(10).max(1);

                                    // Calculate mouse row for visual positioning
                                    let mouse_row_raw = ((hover_pos.y - timeline_rect.min.y) / config.layer_height).floor() as i32;
                                    let mouse_row = mouse_row_raw.max(0) as usize;

                                    // Drop at cursor position, insert at mouse row
                                    let drop_frame = raw_drop_frame;
                                    let insert_idx = mouse_row.min(comp.layers.len());

                                    // Check for cyclic dependency
                                    let is_cycle = {
                                        let media = project.media.read().expect("media lock");
                                        comp.check_collisions(source_uuid, &media, true)
                                    };

                                    // Use mouse position for preview Y (shows insertion point)
                                    let row_y = if hover_pos.y >= timeline_rect.min.y {
                                        row_to_y(mouse_row, config, timeline_rect)
                                    } else {
                                        // Above timeline -> show at top
                                        timeline_rect.min.y
                                    };

                                    draw_drop_preview(
                                        ui.painter(),
                                        drop_frame,
                                        row_y,
                                        dur,
                                        timeline_rect,
                                        config,
                                        state,
                                        is_cycle,
                                    );

                                    // Only allow drop if not a cycle
                                    if !is_cycle && ui.ctx().input(|i| i.pointer.any_released()) {
                                        dispatch(Box::new(AddLayerEvent {
                                            comp_uuid: comp_id,
                                            source_uuid,
                                            start_frame: drop_frame,
                                            insert_idx: Some(insert_idx),
                                        }));
                                        ui.ctx().data_mut(|data| {
                                            data.remove::<GlobalDragState>(egui::Id::new("global_drag_state"));
                                        });
                                    } else if is_cycle && ui.ctx().input(|i| i.pointer.any_released()) {
                                        // Clear drag state even if cycle detected
                                        ui.ctx().data_mut(|data| {
                                            data.remove::<GlobalDragState>(egui::Id::new("global_drag_state"));
                                        });
                                        log::warn!("Blocked cyclic dependency: {} -> {}", source_uuid, comp_id);
                                    }
                                }
            } else if state.drag_state.is_none() && global_drag.is_none() {
                // Handle click/drag interaction only if no active drag state
                if (timeline_response.clicked() || timeline_response.dragged())
                    && !ui
                        .ctx()
                        .input(|i| i.pointer.button_down(egui::PointerButton::Middle))
                    && let Some(pos) = timeline_response.interact_pointer_pos() {
                        // If click is within any layer row, select that layer;
                        // otherwise treat it as a frame scrub on empty space.
                        let mut clicked_layer: Option<usize> = None;
                        for &original_idx in child_order_inner.iter() {
                            // row = layer index
                            let row = original_idx;
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
                            if let Some(layer) = comp.layers.get(idx) {
                                let children_uuids = comp.layers_uuids_vec();
                                let (selection, anchor) = compute_layer_selection(
                                    &comp.layer_selection,
                                    comp.layer_selection_anchor,
                                    layer.uuid(),
                                    idx,
                                    modifiers,
                                    &children_uuids,
                                );
                                dispatch(Box::new(CompSelectionChangedEvent {
                                    comp_uuid: comp_id,
                                    selection: selection.clone(),
                                    anchor,
                                }));
                                dispatch(Box::new(SelectionFocusEvent(selection)));
                            }
                        } else {
                            // Click on empty space
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

                            // If click is BELOW all layers, clear selection
                            let max_layer_y = comp.layers.len() as f32 * config.layer_height + timeline_rect.min.y;
                            if pos.y > max_layer_y && !comp.layer_selection.is_empty() {
                                log::trace!("Canvas: click below layers at y={}, clearing selection", pos.y);
                                dispatch(Box::new(CompSelectionChangedEvent {
                                    comp_uuid: comp_id,
                                    selection: vec![],
                                    anchor: None,
                                }));
                                dispatch(Box::new(SelectionFocusEvent(vec![])));
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
    if let Some(pointer) = ui.ctx().pointer_hover_pos()
        && tab_rect.contains(pointer) {
            timeline_hovered = true;
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
