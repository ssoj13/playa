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

use super::TimelineViewMode;
use super::timeline_events::{
    TimelineFitAllEvent, TimelineLockWorkAreaChangedEvent, TimelinePanChangedEvent,
    TimelineSnapChangedEvent, TimelineZoomChangedEvent,
};
use super::timeline_helpers::{drop_preview_thumb_rect, hash_color_str};
use super::{TimelineConfig, TimelineState};
use crate::widgets::dnd::{
    GlobalDragState, ProjectDragSnapOverlay, global_drag_state_id, project_drag_snap_overlay_id,
};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Ui, Vec2};
use egui_dnd::dnd;
use egui_track_timeline::{
    Clip, Fill, TimelineAction, TimelineConfig as TtConfig, TimelineModel, Track, TrackTimeline,
    WorkArea,
};
use playa_engine::core::event_bus::BoxedEvent;
use playa_engine::core::player_events::{
    JumpToEndEvent, JumpToStartEvent, SetFrameEvent, SetLoopEvent, StopEvent, TogglePlayPauseEvent,
};
use playa_engine::entities::comp_events::{
    AddLayerEvent, CompSelectionChangedEvent, HoverLayerEvent, LayerAttributesChangedEvent,
    MoveAndReorderLayerEvent, ReorderLayerEvent, SetLayerPlayEndEvent, SetLayerPlayStartEvent,
    SlideLayerEvent,
};
use playa_engine::entities::keys::{A_IN, A_SPEED, A_TRIM_IN, A_TRIM_OUT};
use playa_engine::entities::{AttrValue, Comp, Node, frame::FrameStatus};
use playa_events::project_media::{ProjectActiveChangedEvent, SelectionFocusEvent};
use playa_time::{Round, Speed};
use uuid::Uuid;

/// Derive a stable `u64` clip id from a layer `Uuid` (first 8 bytes, LE).
/// Matches `project_ui.rs::uuid_to_u64`; the per-frame `id_map` is the single
/// source of truth for the reverse lookup, so head collisions are harmless.
fn uuid_to_u64(uuid: &Uuid) -> u64 {
    let bytes = uuid.as_bytes();
    let mut head = [0u8; 8];
    head.copy_from_slice(&bytes[0..8]);
    u64::from_le_bytes(head)
}

#[inline]
fn frame_status_paint_rgba(status: FrameStatus) -> Color32 {
    let [r, g, b, a] = status.indicator_rgba_unmul();
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

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
        let anchor_idx = all_children
            .iter()
            .position(|u| *u == anchor_uuid)
            .unwrap_or(clicked_idx);
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

/// Render timeline toolbar (transport controls, zoom, snap, loop, view mode, layouts)
pub fn render_toolbar(
    ui: &mut Ui,
    state: &mut TimelineState,
    loop_enabled: bool,
    show_tooltips: bool,
    layout_names: &[String],
    current_layout: &str,
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
        let zoom_response =
            ui.add(egui::Slider::new(&mut state.zoom, 0.1..=20.0).fixed_decimals(2));
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
            dispatch(Box::new(TimelineLockWorkAreaChangedEvent(
                state.lock_work_area,
            )));
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
            if ui
                .selectable_label(state.view_mode == mode, label)
                .clicked()
            {
                state.view_mode = mode;
            }
        }

        ui.separator();

        // === Layout selector and management buttons ===
        // Allows switching between named UI layouts stored in AppSettings.
        // Layouts persist dock panel sizes, timeline zoom/pan, and viewport state.
        // Events dispatched here are handled by main.rs layout event handlers.
        use playa_engine::core::layout_events::{
            LayoutCreatedEvent, LayoutDeletedEvent, LayoutSelectedEvent,
        };

        let display_name = if current_layout.is_empty() {
            "(none)"
        } else {
            current_layout
        };
        egui::ComboBox::from_id_salt("layout_selector")
            .selected_text(display_name)
            .width(100.0)
            .show_ui(ui, |ui| {
                for name in layout_names {
                    if ui.selectable_label(current_layout == name, name).clicked() {
                        dispatch(Box::new(LayoutSelectedEvent(name.clone())));
                    }
                }
            });

        // Add layout button - duplicates current UI state into new named layout
        if ui.button("+").on_hover_text("Create new layout").clicked() {
            dispatch(Box::new(LayoutCreatedEvent(None)));
        }

        // Delete layout button (only enabled if a layout is selected)
        if ui
            .add_enabled(!current_layout.is_empty(), egui::Button::new("−"))
            .on_hover_text("Delete current layout")
            .clicked()
        {
            dispatch(Box::new(LayoutDeletedEvent(current_layout.to_string())));
        }

        // Rename layout button - opens inline rename dialog
        // Uses pencil icon, only enabled when a layout is selected
        if ui
            .add_enabled(!current_layout.is_empty(), egui::Button::new("✎"))
            .on_hover_text("Rename current layout")
            .clicked()
        {
            state.rename_dialog_open = true;
            state.rename_dialog_old_name = current_layout.to_string();
            state.rename_dialog_name = current_layout.to_string();
        }
    });

    // === Layout rename dialog ===
    // Modal-like window that appears when user clicks rename button.
    // Contains text input for new name and OK/Cancel buttons.
    // Dispatches LayoutRenamedEvent on confirmation.
    if state.rename_dialog_open {
        use playa_engine::core::layout_events::LayoutRenamedEvent;

        let mut should_close = false;
        let mut should_rename = false;

        egui::Window::new("Rename Layout")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ui.ctx(), |ui| {
                ui.horizontal(|ui| {
                    ui.label("Name:");
                    let response = ui.text_edit_singleline(&mut state.rename_dialog_name);

                    // Auto-focus the text field when dialog opens
                    if response.gained_focus()
                        || state.rename_dialog_name == state.rename_dialog_old_name
                    {
                        response.request_focus();
                    }

                    // Enter key confirms rename
                    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        should_rename = true;
                    }
                });

                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        should_rename = true;
                    }
                    if ui.button("Cancel").clicked() {
                        should_close = true;
                    }
                });
            });

        // Handle dialog actions outside the closure to avoid borrow issues
        if should_rename {
            let new_name = state.rename_dialog_name.trim().to_string();
            let old_name = state.rename_dialog_old_name.clone();

            // Only rename if name actually changed and is not empty
            if !new_name.is_empty() && new_name != old_name {
                dispatch(Box::new(LayoutRenamedEvent(old_name, new_name)));
            }
            should_close = true;
        }

        if should_close {
            state.rename_dialog_open = false;
            state.rename_dialog_name.clear();
            state.rename_dialog_old_name.clear();
        }
    }
}

/// Render left outline: layer list only (no toolbar)
/// Render left outline panel: layer list with controls (visibility, name, blend mode, opacity).
///
/// # Parameters
/// - `outline_top_offset`: Vertical offset to align with canvas (from AppSettings.timeline_outline_top_offset)
///
/// Called from ui.rs in Split and OutlineOnly view modes.
pub fn render_outline(
    ui: &mut Ui,
    comp_uuid: Uuid,
    comp: &Comp,
    config: &TimelineConfig,
    _state: &mut TimelineState,
    view_mode: super::TimelineViewMode,
    outline_top_offset: f32,
    mut dispatch: impl FnMut(BoxedEvent),
) {
    let comp_id = comp_uuid;

    // Match the top padding of the timeline canvas (ruler + status bar + spacing)
    // Configurable via AppSettings.timeline_outline_top_offset for fine-tuning alignment
    ui.add_space(outline_top_offset);

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
                    let prev_blend = attrs.get_str("blend_mode").unwrap_or("normal").to_string();
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
                            egui::ComboBox::from_id_salt(
                                egui::Id::new("blend_outline").with(child_uuid),
                            )
                            .width(80.0)
                            .selected_text(blend.clone())
                            .show_ui(ui, |ui| {
                                for mode in [
                                    "normal",
                                    "screen",
                                    "add",
                                    "subtract",
                                    "multiply",
                                    "divide",
                                    "difference",
                                    "overlay",
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
                                .add(egui::DragValue::new(&mut speed).speed(0.1).range(0.1..=4.0))
                                .changed()
                            {
                                dirty = true;
                            }
                        },
                    );

                    // Track matte: dropdown of other layers in this comp.
                    // "None" clears the mask. Picking a layer dispatches
                    // `LayerMaskRefChangedEvent`; the host creates a
                    // `RefNode` (channel=Alpha) and links it. Resolved
                    // ref name display is deferred — combo shows "Set" /
                    // "None" based on the layer's mask_ref_uuid attr.
                    row_ui.allocate_ui_with_layout(
                        egui::Vec2::new(110.0, config.layer_height),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            let has_mask = attrs
                                .get_uuid("mask_ref_uuid")
                                .map(|u| !u.is_nil())
                                .unwrap_or(false);
                            let label = if has_mask { "Mask: ● Set" } else { "Mask: None" };
                            egui::ComboBox::from_id_salt(
                                egui::Id::new("mask_outline").with(child_uuid),
                            )
                            .width(100.0)
                            .selected_text(label)
                            .show_ui(ui, |ui| {
                                // None entry
                                if ui.selectable_label(!has_mask, "None").clicked() {
                                    dispatch(Box::new(
                                        playa_engine::entities::comp_events::LayerMaskRefChangedEvent {
                                            comp_uuid: comp_id,
                                            layer_uuid: child_uuid,
                                            target_layer_uuid: None,
                                        },
                                    ));
                                }
                                // Every OTHER layer in the comp is a
                                // candidate matte source.
                                for other in &comp.layers {
                                    let other_uuid = other.uuid();
                                    if other_uuid == child_uuid {
                                        continue;
                                    }
                                    let other_name = other
                                        .attrs
                                        .get_str("name")
                                        .map(|s| s.to_string())
                                        .unwrap_or_else(|| other_uuid.to_string());
                                    if ui.selectable_label(false, other_name).clicked() {
                                        dispatch(Box::new(
                                            playa_engine::entities::comp_events::LayerMaskRefChangedEvent {
                                                comp_uuid: comp_id,
                                                layer_uuid: child_uuid,
                                                target_layer_uuid: Some(other_uuid),
                                            },
                                        ));
                                    }
                                }
                            });
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
            log::trace!(
                "Empty area clicked, clearing {} selected layers",
                comp.layer_selection.len()
            );
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

/// Render the After Effects-style timeline canvas (right side) by consuming the
/// generic [`egui_track_timeline`] widget.
///
/// Mapping: each playa layer becomes exactly **one widget track holding one clip**
/// (track index == layer index). A fresh [`TimelineModel`] is built every frame;
/// returned [`TimelineAction`]s and the [`TimelineResponse`] hooks (ruler/track
/// rects, hover, double-click) are translated back into playa events.
///
/// The widget owns zoom/pan in [`egui_track_timeline::TimelineView`] (stored in
/// `TimelineState::track_view`). We mirror playa's canonical `zoom`/`pan_offset`
/// into it before `show` and read changes back after, re-emitting
/// `TimelineZoom/PanChangedEvent`, so the toolbar slider, Fit, the ruler-aligned
/// cache status strip, and persistence all keep working.
pub fn render_canvas(
    ui: &mut Ui,
    comp_uuid: Uuid,
    comp: &Comp,
    project: &playa_engine::entities::Project,
    config: &TimelineConfig,
    state: &mut TimelineState,
    view_mode: super::TimelineViewMode,
    timeline_hover_highlight: bool,
    mut dispatch: impl FnMut(BoxedEvent),
) -> super::timeline::TimelineActions {
    // The widget renders bars identically regardless of playa's split/canvas mode;
    // the outline is a separate panel, so view_mode is not needed here.
    let _ = view_mode;

    // Save canvas width for the toolbar "Fit" button calculation.
    state.last_canvas_width = ui.available_width();
    let comp_id = comp_uuid;
    let tab_rect = ui.max_rect();

    // Dynamic src_len lookups come from media.
    let media = project.media.read().expect("media lock");

    // --- Build the generic model: 1 layer => 1 track => 1 clip. ---
    // `id_map` reverses the opaque u64 clip id (first 8 bytes of the layer uuid)
    // back to the real uuid; unknown ids in returned actions are ignored.
    let mut id_map: std::collections::HashMap<u64, Uuid> = std::collections::HashMap::new();
    let mut tracks: Vec<Track> = Vec::with_capacity(comp.layers.len());
    for layer in comp.layers.iter() {
        let uuid = layer.uuid();
        let id = uuid_to_u64(&uuid);
        id_map.insert(id, uuid);

        let attrs = &layer.attrs;
        let start = layer.start() as i64; // == A_IN
        let end = comp.get_layer_end(layer, &media) as i64;
        let duration = (end - start + 1).max(1);
        let (play_start, play_end) = comp.get_layer_work_area(layer, &media);
        let trim_in = play_start as i64 - start;
        let trim_out = (start + duration) - (play_end as i64 + 1);

        let name = attrs.get_str("name").unwrap_or("?").to_string();
        let visible = attrs.get_bool("visible").unwrap_or(true);
        // Preserve the exact old bar colour (hash of name, grey when hidden).
        let color = if visible {
            hash_color_str(&name)
        } else {
            Color32::from_gray(70)
        };
        // File-source layers get a diagonal hatch overlay, like the old bars.
        let is_file = project
            .with_node(layer.source_uuid(), |n| n.is_file())
            .unwrap_or(false);
        let fill = if is_file { Fill::Hatch } else { Fill::Solid };

        let clip = Clip::new(id, start, duration, name.clone())
            .with_trims(trim_in, trim_out)
            .with_color(color)
            .with_fill(fill);
        tracks.push(Track::new(name, vec![clip]));
    }

    let selection: Vec<u64> = comp.layer_selection.iter().map(uuid_to_u64).collect();

    // Work-area (play range) overlay, in exclusive-end frames.
    let (wa_start, wa_end) = comp.play_range(true);
    let work_area = Some(WorkArea {
        start: wa_start as i64,
        end: wa_end as i64 + 1,
    });

    // Ruler bookmarks -> marker glyphs.
    let mut markers: Vec<i64> = Vec::new();
    if let Some(bookmarks) = comp.attrs.get_map("bookmarks") {
        for value in bookmarks.values() {
            if let AttrValue::Int(f) = value {
                markers.push(*f as i64);
            }
        }
    }

    let mut model = TimelineModel::new(comp.fps(), tracks);
    model.playhead = comp.frame() as i64;
    model.selection = selection;
    model.work_area = work_area;
    model.markers = markers;

    // Widget layout config derived from playa's TimelineConfig (8px edge handles,
    // matching the old `edge_threshold`).
    let ett_cfg = TtConfig {
        row_height: config.layer_height,
        pixels_per_frame: config.pixels_per_frame,
        edge_threshold: 8.0,
        show_work_area: true,
    };

    // Frame-cache status strip data; kept ruler-aligned, painted as an overlay
    // just below the widget ruler (the widget draws no status strip).
    let status_strip = comp.cache_frame_statuses(project.global_cache.as_ref());
    let comp_start = comp._in();

    // Mirror playa's canonical zoom/pan into the widget view before show.
    state.track_view.zoom = state.zoom;
    state.track_view.pan_offset = state.pan_offset;

    // Run the widget inside the shared vertical scroll area so its bars stay
    // vertically aligned with the left outline panel (same `id_salt`).
    let response = egui::ScrollArea::vertical()
        .id_salt("timeline_layers_scroll")
        .max_height(ui.available_height())
        .show(ui, |ui| {
            let resp = TrackTimeline::new(ett_cfg).show(ui, &mut state.track_view, &model);

            // Paint the cache status strip aligned to the widget ruler, using the
            // pre-show zoom/pan still held in `state` (matches the ruler drawn
            // this frame).
            if let Some(statuses) = &status_strip {
                let ruler = resp.ruler_rect;
                let strip_height = 2.0;
                let strip_rect = Rect::from_min_max(
                    Pos2::new(ruler.min.x, ruler.max.y),
                    Pos2::new(ruler.max.x, ruler.max.y + strip_height),
                );
                draw_status_strip(ui, strip_rect, statuses, comp_start, 0, ruler, config, state);
            }
            resp
        })
        .inner;

    // --- Bookmark ruler input (reimplemented over the widget ruler) ---
    // Ctrl+click clears the nearest bookmark (<=10px) and must NOT also scrub, so
    // we suppress the widget's matching Seek this frame.
    let modifiers = ui.input(|i| i.modifiers);
    let pointer = ui.input(|i| i.pointer.interact_pos());
    let primary_clicked = ui.input(|i| i.pointer.primary_clicked());
    let mut suppress_seek = false;
    if modifiers.ctrl
        && primary_clicked
        && let Some(pos) = pointer
        && response.ruler_rect.contains(pos)
    {
        suppress_seek = true;
        if let Some(bookmarks) = comp.attrs.get_map("bookmarks") {
            const THRESHOLD: f32 = 10.0;
            let mut candidates: Vec<(f32, u8)> = bookmarks
                .iter()
                .filter_map(|(slot_str, value)| {
                    let bm_frame = match value {
                        AttrValue::Int(f) => *f,
                        _ => return None,
                    };
                    let slot: u8 = slot_str.parse().ok()?;
                    let x = state
                        .track_view
                        .frame_to_x(bm_frame as f32, response.ruler_rect.min.x, &ett_cfg);
                    let dist = (pos.x - x).abs();
                    (dist <= THRESHOLD).then_some((dist, slot))
                })
                .collect();
            candidates.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            if let Some((_, slot)) = candidates.first() {
                dispatch(Box::new(
                    playa_engine::entities::comp_events::SetBookmarkEvent {
                        comp_uuid,
                        slot: *slot,
                        frame: None,
                    },
                ));
            }
        }
    }

    // --- Translate widget actions into playa events ---
    for action in &response.actions {
        match *action {
            TimelineAction::Seek { frame } => {
                if !suppress_seek {
                    dispatch(Box::new(SetFrameEvent(frame as i32)));
                }
            }
            TimelineAction::Select {
                id,
                additive,
                range,
            } => {
                let Some(uuid) = id_map.get(&id).copied() else {
                    continue;
                };
                let Some(idx) = comp.uuid_to_idx(uuid) else {
                    continue;
                };
                let mods = egui::Modifiers {
                    ctrl: additive,
                    shift: range,
                    ..Default::default()
                };
                let all = comp.layers_uuids_vec();
                let (sel, anchor) = compute_layer_selection(
                    &comp.layer_selection,
                    comp.layer_selection_anchor,
                    uuid,
                    idx,
                    mods,
                    &all,
                );
                dispatch(Box::new(CompSelectionChangedEvent {
                    comp_uuid,
                    selection: sel.clone(),
                    anchor,
                }));
                dispatch(Box::new(SelectionFocusEvent(sel)));
            }
            TimelineAction::ClearSelection => {
                dispatch(Box::new(CompSelectionChangedEvent {
                    comp_uuid,
                    selection: vec![],
                    anchor: None,
                }));
                dispatch(Box::new(SelectionFocusEvent(vec![])));
            }
            TimelineAction::MoveClip {
                id,
                new_start,
                new_track,
            } => {
                let Some(uuid) = id_map.get(&id).copied() else {
                    continue;
                };
                let Some(idx) = comp.uuid_to_idx(uuid) else {
                    continue;
                };
                dispatch(Box::new(MoveAndReorderLayerEvent {
                    comp_uuid,
                    layer_idx: idx,
                    new_start: new_start as i32,
                    new_idx: new_track,
                }));
            }
            TimelineAction::TrimStart { id, delta } => {
                let Some(uuid) = id_map.get(&id).copied() else {
                    continue;
                };
                let Some(idx) = comp.uuid_to_idx(uuid) else {
                    continue;
                };
                // play_start is in timeline frames; widget TrimStart delta is too.
                let new_play_start = comp.layers[idx].attrs.layer_start() + delta as i32;
                dispatch(Box::new(SetLayerPlayStartEvent {
                    comp_uuid,
                    layer_idx: idx,
                    new_play_start,
                }));
            }
            TimelineAction::TrimEnd { id, delta } => {
                let Some(uuid) = id_map.get(&id).copied() else {
                    continue;
                };
                let Some(idx) = comp.uuid_to_idx(uuid) else {
                    continue;
                };
                // SIGN FLIP: the widget already negates TrimEnd delta (positive =
                // more trimmed off the tail => play_end decreases). playa's event
                // wants `layer_end() + px_delta` with `px_delta = -delta`, hence
                // `- delta`.
                let new_play_end = comp.layers[idx].attrs.layer_end() - delta as i32;
                dispatch(Box::new(SetLayerPlayEndEvent {
                    comp_uuid,
                    layer_idx: idx,
                    new_play_end,
                }));
            }
            TimelineAction::Slide { id, delta } => {
                let Some(uuid) = id_map.get(&id).copied() else {
                    continue;
                };
                let Some(idx) = comp.uuid_to_idx(uuid) else {
                    continue;
                };
                let attrs = &comp.layers[idx].attrs;
                let a_in = attrs.get_i32_or_zero(A_IN);
                let a_trim_in = attrs.get_i32_or_zero(A_TRIM_IN);
                let a_trim_out = attrs.get_i32_or_zero(A_TRIM_OUT);
                let speed = Speed::new(attrs.get_float_or(A_SPEED, 1.0));
                // Widget Slide.delta is timeline frames; playa trims are SOURCE
                // frames, so speed-convert at apply time.
                let trim_delta = speed.scale_timeline_to_src(delta as i32, Round::Round);
                dispatch(Box::new(SlideLayerEvent {
                    comp_uuid,
                    layer_idx: idx,
                    new_in: a_in + delta as i32,
                    new_trim_in: (a_trim_in - trim_delta).max(0),
                    new_trim_out: (a_trim_out + trim_delta).max(0),
                }));
            }
        }
    }

    // --- Double-click a bar: dive into the layer's source comp ---
    if let Some(id) = response.double_clicked
        && let Some(uuid) = id_map.get(&id).copied()
        && let Some(idx) = comp.uuid_to_idx(uuid)
    {
        let layer = &comp.layers[idx];
        let local_frame = layer.parent_to_local(comp.frame());
        dispatch(Box::new(ProjectActiveChangedEvent::with_frame(
            layer.source_uuid(),
            local_frame,
        )));
    }

    // --- Hover highlight: mirror the widget's hovered clip into comp.hovered_layer
    // (only while the pointer is over the track area, so the viewport keeps owning
    // hover elsewhere). ---
    if timeline_hover_highlight
        && let Some(p) = ui.ctx().pointer_hover_pos()
        && response.track_rect.contains(p)
    {
        let want = response.hovered.and_then(|id| id_map.get(&id).copied());
        if comp.hovered_layer != want {
            dispatch(Box::new(HoverLayerEvent {
                comp_uuid,
                layer_uuid: want,
            }));
        }
    }

    // --- Project -> timeline drop (GlobalDragState::ProjectItem) ---
    let global_drag: Option<GlobalDragState> =
        ui.ctx().data(|d| d.get_temp(global_drag_state_id()));
    if let Some(GlobalDragState::ProjectItem {
        source_uuid,
        duration,
    }) = global_drag.as_ref()
        && let Some(hover_pos) = ui
            .ctx()
            .input(|i| i.pointer.hover_pos().or_else(|| i.pointer.latest_pos()))
        && response.track_rect.contains(hover_pos)
    {
        let frame = state
            .track_view
            .x_to_frame(hover_pos.x, response.track_rect.left(), &ett_cfg)
            .round()
            .max(0.0) as i32;
        let dur = duration.unwrap_or(10).max(1);
        let row = ((hover_pos.y - response.track_rect.top()) / config.layer_height)
            .floor()
            .max(0.0) as usize;
        let insert_idx = row.min(comp.layers.len());
        let is_cycle = project.would_create_cycle(comp_id, *source_uuid);
        let row_y = response.track_rect.top() + row as f32 * config.layer_height;
        let thumb_rect =
            drop_preview_thumb_rect(frame, row_y, dur, response.track_rect, config, state);
        ui.ctx().data_mut(|d| {
            d.insert_temp(
                project_drag_snap_overlay_id(),
                ProjectDragSnapOverlay {
                    rect: thumb_rect,
                    is_cycle,
                },
            );
        });
        if ui.ctx().input(|i| i.pointer.primary_released()) {
            if !is_cycle {
                dispatch(Box::new(AddLayerEvent {
                    comp_uuid,
                    source_uuid: *source_uuid,
                    start_frame: frame,
                    insert_idx: Some(insert_idx),
                }));
            } else {
                log::warn!("Blocked cyclic dependency: {} -> {}", source_uuid, comp_id);
            }
            ui.ctx()
                .data_mut(|d| d.remove::<GlobalDragState>(global_drag_state_id()));
        }
    }

    // --- Whole-tab hover gate for bookmark hotkeys + input routing ---
    let timeline_hovered = ui
        .ctx()
        .pointer_hover_pos()
        .map(|p| tab_rect.contains(p))
        .unwrap_or(false);

    // Bookmark set (Shift+digit) / jump (digit) hotkeys. Zoom hotkeys (Ctrl+wheel,
    // +/-) and pan are handled inside the widget now, so they are not re-handled.
    if timeline_hovered {
        let current_frame = comp.frame();
        ui.ctx().input(|i| {
            // Shift+0-9 set bookmark (Shift+digit yields symbols on US layouts).
            let shift_symbols = [
                (')', 0u8),
                ('!', 1),
                ('@', 2),
                ('#', 3),
                ('$', 4),
                ('%', 5),
                ('^', 6),
                ('&', 7),
                ('*', 8),
                ('(', 9),
            ];
            for &(sym, slot) in &shift_symbols {
                if i
                    .events
                    .iter()
                    .any(|e| matches!(e, egui::Event::Text(s) if s.chars().next() == Some(sym)))
                {
                    dispatch(Box::new(
                        playa_engine::entities::comp_events::SetBookmarkEvent {
                            comp_uuid,
                            slot,
                            frame: Some(current_frame),
                        },
                    ));
                }
            }
            // 0-9 jump to bookmark.
            let digit_keys = [
                (egui::Key::Num0, 0u8),
                (egui::Key::Num1, 1),
                (egui::Key::Num2, 2),
                (egui::Key::Num3, 3),
                (egui::Key::Num4, 4),
                (egui::Key::Num5, 5),
                (egui::Key::Num6, 6),
                (egui::Key::Num7, 7),
                (egui::Key::Num8, 8),
                (egui::Key::Num9, 9),
            ];
            for (key, slot) in digit_keys {
                if i.key_pressed(key) && !i.modifiers.shift {
                    dispatch(Box::new(
                        playa_engine::entities::comp_events::JumpToBookmarkEvent {
                            comp_uuid,
                            slot,
                        },
                    ));
                }
            }
        });
    }

    // --- Read widget view changes back into playa's canonical state + notify ---
    if state.track_view.zoom != state.zoom {
        state.zoom = state.track_view.zoom;
        dispatch(Box::new(TimelineZoomChangedEvent(state.zoom)));
    }
    if state.track_view.pan_offset != state.pan_offset {
        state.pan_offset = state.track_view.pan_offset;
        dispatch(Box::new(TimelinePanChangedEvent(state.pan_offset)));
    }

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
                        painter.rect_filled(run_rect, 0.0, frame_status_paint_rgba(prev_status));
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
        let x_start =
            super::timeline_helpers::frame_to_screen_x(start_frame as f32, base_x, config, state);
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
            let run_rect =
                Rect::from_min_max(Pos2::new(x_start, rect.min.y), Pos2::new(x_end, rect.max.y));
            painter.rect_filled(run_rect, 0.0, frame_status_paint_rgba(status));
        }
    }
}
