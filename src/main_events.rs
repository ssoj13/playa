//! Application event handling - extracted from main.rs for clarity.
//!
//! # Layer Attribute Changes
//!
//! The [`LayerAttributesChangedEvent`] is emitted by the timeline outline panel
//! when the user modifies visibility, opacity, blend_mode, or speed via the UI.
//!
//! The handler uses [`Comp::set_child_attrs`] which automatically:
//! 1. Updates the attribute values
//! 2. Marks the comp dirty
//! 3. Emits [`AttrsChangedEvent`] to invalidate frame cache
//!
//! This ensures consistent behavior with the Attribute Editor panel, which also
//! uses `set_child_attrs` for modifications.
//!
//! # Important: Event Downcasting Bug (Fixed 2025-12-05)
//!
//! When using `downcast_event<E>(&event)` where `event: &BoxedEvent` (i.e., `&Box<dyn Event>`),
//! be aware of Rust's method resolution with blanket implementations.
//!
//! The blanket impl `impl<T: Any + Send + Sync + 'static> Event for T` means that
//! `Box<dyn Event>` ALSO implements `Event`. When calling `event.as_any()`, Rust's
//! method resolution may pick the `Box`'s blanket impl instead of the inner type's impl.
//!
//! This causes `as_any()` to return `&dyn Any` containing `Box<dyn Event>`'s TypeId,
//! not the original event type's TypeId, making all downcasts fail!
//!
//! **Fix**: In `downcast_event`, use explicit deref: `(**event).as_any()` to force
//! the call through `dyn Event`'s vtable, which correctly returns the original type's TypeId.
//!
//! See `event_bus.rs::downcast_event()` for the corrected implementation.

use log::debug;
use std::path::PathBuf;
use uuid::Uuid;

use crate::dialogs::encode::EncodeDialog;
use crate::entities::Project;
use crate::core::event_bus::{BoxedEvent, downcast_event};
use crate::core::player::Player;
use crate::core::player_events::*;
use crate::core::project_events::*;
use crate::entities::comp_events::*;
use crate::widgets::timeline::timeline_events::*;
use crate::widgets::viewport::viewport_events::*;
use crate::dialogs::prefs::prefs_events::*;

/// Jump to next/prev layer edge in composition
/// direction > 0: next edge, direction < 0: prev edge
fn jump_to_edge(comp: &mut crate::entities::Comp, forward: bool) {
    let current = comp.frame();
    let edges = comp.get_child_edges();
    if edges.is_empty() {
        return;
    }
    if forward {
        // Find first edge after current, or wrap to first
        if let Some((frame, _)) = edges.iter().find(|(f, _)| *f > current) {
            comp.set_frame(*frame);
        } else if let Some((frame, _)) = edges.first() {
            comp.set_frame(*frame);
        }
    } else {
        // Find last edge before current, or wrap to last
        if let Some((frame, _)) = edges.iter().rev().find(|(f, _)| *f < current) {
            comp.set_frame(*frame);
        } else if let Some((frame, _)) = edges.last() {
            comp.set_frame(*frame);
        }
    }
}

/// Adjust base FPS up or down
fn adjust_fps_base(player: &mut Player, project: &mut Project, increase: bool) {
    if increase {
        player.increase_fps_base();
    } else {
        player.decrease_fps_base();
    }
    if let Some(comp_uuid) = player.active_comp() {
        let fps = player.fps_base();
        project.modify_comp(comp_uuid, |comp| comp.set_fps(fps));
    }
}

/// Result of handling an app event - may contain deferred actions
#[derive(Default)]
pub struct EventResult {
    pub load_project: Option<PathBuf>,
    pub save_project: Option<PathBuf>,
    pub load_sequences: Option<Vec<PathBuf>>,
    pub new_comp: Option<(String, f32)>,
    pub enqueue_frames: Option<usize>,
    pub quick_save: bool,
    pub show_open_dialog: bool,
}

/// Handle a single app event (called from main event loop).
/// Returns Some(result) if event was handled, None otherwise.
pub fn handle_app_event(
    event: &BoxedEvent,
    player: &mut Player,
    project: &mut Project,
    timeline_state: &mut crate::widgets::timeline::TimelineState,
    viewport_state: &mut crate::widgets::viewport::ViewportState,
    settings: &mut crate::dialogs::prefs::AppSettings,
    show_help: &mut bool,
    show_playlist: &mut bool,
    show_settings: &mut bool,
    show_encode_dialog: &mut bool,
    show_attributes_editor: &mut bool,
    encode_dialog: &mut Option<EncodeDialog>,
    is_fullscreen: &mut bool,
    fullscreen_dirty: &mut bool,
    reset_settings_pending: &mut bool,
) -> Option<EventResult> {
    let mut result = EventResult::default();
    // === Playback Control ===
    if downcast_event::<TogglePlayPauseEvent>(&event).is_some() {
        let was_playing = player.is_playing();
        player.set_is_playing(!was_playing);
        if player.is_playing() {
            debug!("TogglePlayPause: starting playback at frame {}", player.current_frame(project));
            player.last_frame_time = Some(std::time::Instant::now());
        } else {
            debug!("TogglePlayPause: pausing at frame {}", player.current_frame(project));
            player.last_frame_time = None;
            player.set_fps_play(player.fps_base());
        }
        return Some(result);
    }
    if downcast_event::<StopEvent>(&event).is_some() {
        player.stop();
        return Some(result);
    }
    if let Some(e) = downcast_event::<SetFrameEvent>(&event) {
        debug!("SetFrame: moving to frame {}", e.0);
        if let Some(comp_uuid) = player.active_comp() {
            project.modify_comp(comp_uuid, |comp| {
                comp.set_frame(e.0);
            });
            result.enqueue_frames = Some(10);
        }
        return Some(result);
    }
    if downcast_event::<StepForwardEvent>(&event).is_some() {
        player.step(1, project);
        return Some(result);
    }
    if downcast_event::<StepBackwardEvent>(&event).is_some() {
        player.step(-1, project);
        return Some(result);
    }
    if downcast_event::<StepForwardLargeEvent>(&event).is_some() {
        player.step(crate::core::player::FRAME_JUMP_STEP, project);
        return Some(result);
    }
    if downcast_event::<StepBackwardLargeEvent>(&event).is_some() {
        player.step(-crate::core::player::FRAME_JUMP_STEP, project);
        return Some(result);
    }
    if downcast_event::<JumpToStartEvent>(&event).is_some() {
        player.to_start(project);
        return Some(result);
    }
    if downcast_event::<JumpToEndEvent>(&event).is_some() {
        player.to_end(project);
        return Some(result);
    }
    if downcast_event::<JumpToPrevEdgeEvent>(&event).is_some() {
        if let Some(comp_uuid) = player.active_comp() {
            project.modify_comp(comp_uuid, |comp| jump_to_edge(comp, false));
        }
        return Some(result);
    }
    if downcast_event::<JumpToNextEdgeEvent>(&event).is_some() {
        if let Some(comp_uuid) = player.active_comp() {
            project.modify_comp(comp_uuid, |comp| jump_to_edge(comp, true));
        }
        return Some(result);
    }
    if downcast_event::<JogForwardEvent>(&event).is_some() {
        player.jog_forward();
        return Some(result);
    }
    if downcast_event::<JogBackwardEvent>(&event).is_some() {
        player.jog_backward();
        return Some(result);
    }

    // === FPS Control ===
    if downcast_event::<IncreaseFPSBaseEvent>(&event).is_some() {
        adjust_fps_base(player, project, true);
        return Some(result);
    }
    if downcast_event::<DecreaseFPSBaseEvent>(&event).is_some() {
        adjust_fps_base(player, project, false);
        return Some(result);
    }

    // === Play Range Control ===
    if downcast_event::<SetPlayRangeStartEvent>(&event).is_some() {
        if let Some(comp_uuid) = player.active_comp() {
            project.modify_comp(comp_uuid, |comp| {
                let current = comp.frame();
                comp.set_comp_play_start(current);
            });
        }
        return Some(result);
    }
    if downcast_event::<SetPlayRangeEndEvent>(&event).is_some() {
        if let Some(comp_uuid) = player.active_comp() {
            project.modify_comp(comp_uuid, |comp| {
                let current = comp.frame();
                comp.set_comp_play_end(current);
            });
        }
        return Some(result);
    }
    if downcast_event::<ResetPlayRangeEvent>(&event).is_some() {
        if let Some(comp_uuid) = player.active_comp() {
            project.modify_comp(comp_uuid, |comp| {
                let start = comp._in();
                let end = comp._out();
                comp.set_comp_play_start(start);
                comp.set_comp_play_end(end);
            });
        }
        return Some(result);
    }
    if downcast_event::<ToggleLoopEvent>(&event).is_some() {
        settings.loop_enabled = !settings.loop_enabled;
        player.set_loop_enabled(settings.loop_enabled);
        return Some(result);
    }
    if let Some(e) = downcast_event::<SetLoopEvent>(&event) {
        settings.loop_enabled = e.0;
        player.set_loop_enabled(e.0);
        return Some(result);
    }

    // === Project Management ===
    if let Some(e) = downcast_event::<AddClipEvent>(&event) {
        result.load_sequences = Some(vec![e.0.clone()]);
        return Some(result);
    }
    if let Some(e) = downcast_event::<AddClipsEvent>(&event) {
        result.load_sequences = Some(e.0.clone());
        return Some(result);
    }
    if let Some(e) = downcast_event::<AddCompEvent>(&event) {
        result.new_comp = Some((e.name.clone(), e.fps));
        return Some(result);
    }
    if let Some(e) = downcast_event::<SaveProjectEvent>(&event) {
        result.save_project = Some(e.0.clone());
        return Some(result);
    }
    if let Some(e) = downcast_event::<LoadProjectEvent>(&event) {
        result.load_project = Some(e.0.clone());
        return Some(result);
    }
    if downcast_event::<QuickSaveEvent>(&event).is_some() {
        result.quick_save = true;
        return Some(result);
    }
    if downcast_event::<OpenProjectDialogEvent>(&event).is_some() {
        result.show_open_dialog = true;
        return Some(result);
    }
    if let Some(e) = downcast_event::<RemoveMediaEvent>(&event) {
        let uuid = e.0;
        let was_active = player.active_comp() == Some(uuid);
        project.remove_media_with_cleanup(uuid);
        if was_active {
            let first = project.comps_order().first().cloned();
            player.set_active_comp(first, project);
        }
        return Some(result);
    }
    if downcast_event::<RemoveSelectedMediaEvent>(&event).is_some() {
        let selection: Vec<Uuid> = project.selection();
        let active = player.active_comp();
        for uuid in selection {
            project.remove_media_with_cleanup(uuid);
        }
        // Fix active if deleted
        if let Some(a) = active {
            if !project.media.read().expect("media lock poisoned").contains_key(&a) {
                let first = project.comps_order().first().cloned();
                player.set_active_comp(first, project);
            }
        }
        return Some(result);
    }
    if downcast_event::<ClearAllMediaEvent>(&event).is_some() {
        project.media.write().expect("media lock poisoned").clear();
        project.set_comps_order(Vec::new());
        project.set_selection(Vec::new());
        player.set_active_comp(None, project);
        return Some(result);
    }
    if let Some(e) = downcast_event::<SelectMediaEvent>(&event) {
        player.set_active_comp(Some(e.0), project); // also resets selection
        project.selection_anchor = project.comps_order().iter().position(|u| *u == e.0);
        return Some(result);
    }

    // === Selection ===
    if let Some(e) = downcast_event::<ProjectSelectionChangedEvent>(&event) {
        project.set_selection(e.selection.clone());
        project.selection_anchor = e.anchor.or_else(|| {
            let sel = project.selection();
            let order = project.comps_order();
            sel.last().and_then(|u| order.iter().position(|x| x == u))
        });
        return Some(result);
    }
    if let Some(e) = downcast_event::<ProjectActiveChangedEvent>(&event) {
        player.set_active_comp(Some(e.0), project); // also resets selection
        project.selection_anchor = project.comps_order().iter().position(|u| *u == e.0);
        return Some(result);
    }
    if downcast_event::<ProjectPreviousCompEvent>(&event).is_some() {
        if let Some(prev) = player.previous_comp() {
            player.set_active_comp(Some(prev), project);
            project.selection_anchor = project.comps_order().iter().position(|u| *u == prev);
        }
        return Some(result);
    }
    if let Some(e) = downcast_event::<CompSelectionChangedEvent>(&event) {
        project.modify_comp(e.comp_uuid, |comp| {
            comp.layer_selection = e.selection.clone();
            comp.layer_selection_anchor = e.anchor;
        });
        return Some(result);
    }

    // === UI State ===
    if downcast_event::<ToggleHelpEvent>(&event).is_some() {
        *show_help = !*show_help;
        return Some(result);
    }
    if downcast_event::<TogglePlaylistEvent>(&event).is_some() {
        *show_playlist = !*show_playlist;
        return Some(result);
    }
    if downcast_event::<ToggleSettingsEvent>(&event).is_some() {
        *show_settings = !*show_settings;
        return Some(result);
    }
    if downcast_event::<ToggleAttributeEditorEvent>(&event).is_some() {
        *show_attributes_editor = !*show_attributes_editor;
        return Some(result);
    }
    if downcast_event::<ToggleEncodeDialogEvent>(&event).is_some() {
        *show_encode_dialog = !*show_encode_dialog;
        if *show_encode_dialog && encode_dialog.is_none() {
            debug!("[ToggleEncodeDialog] Opening encode dialog");
            *encode_dialog = Some(EncodeDialog::load_from_settings(&settings.encode_dialog));
        }
        return Some(result);
    }
    if downcast_event::<ToggleFullscreenEvent>(&event).is_some() {
        *is_fullscreen = !*is_fullscreen;
        *fullscreen_dirty = true;
        return Some(result);
    }
    if downcast_event::<ToggleFrameNumbersEvent>(&event).is_some() {
        settings.show_frame_numbers = !settings.show_frame_numbers;
        return Some(result);
    }
    if downcast_event::<ResetSettingsEvent>(&event).is_some() {
        *reset_settings_pending = true;
        return Some(result);
    }

    // === Timeline State ===
    if let Some(e) = downcast_event::<TimelineZoomChangedEvent>(&event) {
        timeline_state.zoom = e.0.clamp(0.1, 20.0);
        return Some(result);
    }
    if let Some(e) = downcast_event::<TimelinePanChangedEvent>(&event) {
        timeline_state.pan_offset = e.0;
        return Some(result);
    }
    if let Some(e) = downcast_event::<TimelineSnapChangedEvent>(&event) {
        timeline_state.snap_enabled = e.0;
        return Some(result);
    }
    if let Some(e) = downcast_event::<TimelineLockWorkAreaChangedEvent>(&event) {
        timeline_state.lock_work_area = e.0;
        return Some(result);
    }
    if let Some(e) = downcast_event::<TimelineFitAllEvent>(&event) {
        if let Some(comp_uuid) = player.active_comp() {
            let media = project.media.read().expect("media lock poisoned");
            if let Some(comp) = media.get(&comp_uuid) {
                let (min_frame, max_frame) = comp.play_range(true);
                let duration = (max_frame - min_frame + 1).max(1);
                let pixels_per_frame = e.0 / duration as f32;
                let default_ppf = 2.0;
                let zoom = (pixels_per_frame / default_ppf).clamp(0.1, 20.0);
                timeline_state.zoom = zoom;
                timeline_state.pan_offset = min_frame as f32;
            }
        }
        return Some(result);
    }
    if downcast_event::<TimelineFitEvent>(&event).is_some() {
        let canvas_width = timeline_state.last_canvas_width;
        // Recursive call via TimelineFitAllEvent
        if let Some(comp_uuid) = player.active_comp() {
            let media = project.media.read().expect("media lock poisoned");
            if let Some(comp) = media.get(&comp_uuid) {
                let (min_frame, max_frame) = comp.play_range(true);
                let duration = (max_frame - min_frame + 1).max(1);
                let pixels_per_frame = canvas_width / duration as f32;
                let default_ppf = 2.0;
                let zoom = (pixels_per_frame / default_ppf).clamp(0.1, 20.0);
                timeline_state.zoom = zoom;
                timeline_state.pan_offset = min_frame as f32;
            }
        }
        return Some(result);
    }
    if downcast_event::<TimelineResetZoomEvent>(&event).is_some() {
        timeline_state.zoom = 1.0;
        return Some(result);
    }

    // === Viewport State ===
    if let Some(e) = downcast_event::<ZoomViewportEvent>(&event) {
        viewport_state.zoom *= e.0;
        return Some(result);
    }
    if downcast_event::<ResetViewportEvent>(&event).is_some() {
        viewport_state.reset();
        return Some(result);
    }
    if downcast_event::<FitViewportEvent>(&event).is_some() {
        viewport_state.set_mode_fit();
        return Some(result);
    }
    if downcast_event::<Viewport100Event>(&event).is_some() {
        viewport_state.set_mode_100();
        return Some(result);
    }

    // === Layer Operations ===
    if let Some(e) = downcast_event::<AddLayerEvent>(&event) {
        // Get source info and generate name BEFORE write lock
        let source_info = project.get_comp(e.source_uuid).map(|s| {
            let name = project.gen_name(s.name());
            (s.frame_count(), s.dim(), name)
        });

        let add_result = {
            let mut media = project.media.write().expect("media lock poisoned");
            if let Some(comp) = media.get_mut(&e.comp_uuid) {
                let (duration, source_dim, name) = source_info.unwrap_or((1, (64, 64), "layer_1".to_string()));
                comp.add_child_layer(e.source_uuid, &name, e.start_frame, duration, e.target_row, source_dim)
            } else {
                Err(anyhow::anyhow!("Parent comp not found"))
            }
        };

        if add_result.is_ok() {
            project.modify_comp(e.source_uuid, |child_comp| {
                if child_comp.get_parent() != Some(e.comp_uuid) {
                    child_comp.set_parent(Some(e.comp_uuid));
                }
            });
        } else if let Err(err) = add_result {
            log::error!("Failed to add layer: {}", err);
        }
        return Some(result);
    }
    if let Some(e) = downcast_event::<RemoveLayerEvent>(&event) {
        let child_data = project.get_comp(e.comp_uuid).and_then(|comp| {
            comp.get_children().get(e.layer_idx).map(|(child_uuid, attrs)| {
                let source_uuid = attrs.get_str("uuid").and_then(|s| Uuid::parse_str(s).ok());
                (*child_uuid, source_uuid)
            })
        });

        if let Some((child_uuid, source_uuid_opt)) = child_data {
            if let Some(source_uuid) = source_uuid_opt {
                project.modify_comp(source_uuid, |child_comp| {
                    if child_comp.get_parent() == Some(e.comp_uuid) {
                        child_comp.set_parent(None);
                    }
                });
            }
            project.modify_comp(e.comp_uuid, |comp| {
                if comp.has_child(child_uuid) {
                    comp.remove_child(child_uuid);
                }
            });
        }
        return Some(result);
    }
    if downcast_event::<RemoveSelectedLayerEvent>(&event).is_some() {
        if let Some(active_uuid) = player.active_comp() {
            project.modify_comp(active_uuid, |comp| {
                let to_remove: Vec<Uuid> = comp.layer_selection.clone();
                for child_uuid in to_remove {
                    comp.remove_child(child_uuid);
                }
                comp.layer_selection.clear();
                comp.layer_selection_anchor = None;
            });
        }
        return Some(result);
    }
    if let Some(e) = downcast_event::<ReorderLayerEvent>(&event) {
        project.modify_comp(e.comp_uuid, |comp| {
            let children = comp.get_children();
            if e.from_idx != e.to_idx && e.from_idx < children.len() && e.to_idx < children.len() {
                let mut reordered = comp.children.clone();
                let child_uuid = reordered.remove(e.from_idx);
                reordered.insert(e.to_idx, child_uuid);
                comp.children = reordered;
                comp.attrs.mark_dirty();
            }
        });
        return Some(result);
    }
    if let Some(e) = downcast_event::<MoveAndReorderLayerEvent>(&event) {
        project.modify_comp(e.comp_uuid, |comp| {
            let dragged_uuid = comp.idx_to_uuid(e.layer_idx).unwrap_or_default();
            if comp.is_multi_selected(dragged_uuid) {
                let dragged_in = comp.child_in(dragged_uuid);
                let delta = e.new_start - dragged_in;
                let selection_indices = comp.uuids_to_indices(&comp.layer_selection);
                let _ = comp.move_layers(&selection_indices, delta, Some(e.new_idx));
            } else {
                let dragged_in = comp.child_in(dragged_uuid);
                let delta = e.new_start - dragged_in;
                let _ = comp.move_layers(&[e.layer_idx], delta, Some(e.new_idx));
            }
        });
        return Some(result);
    }
    if let Some(e) = downcast_event::<SetLayerPlayStartEvent>(&event) {
        project.modify_comp(e.comp_uuid, |comp| {
            let dragged_uuid = comp.idx_to_uuid(e.layer_idx).unwrap_or_default();
            if comp.is_multi_selected(dragged_uuid) {
                let dragged_ps = comp.child_start(dragged_uuid);
                let delta = e.new_play_start - dragged_ps;
                let selection_indices = comp.uuids_to_indices(&comp.layer_selection);
                let _ = comp.trim_layers(&selection_indices, delta, true);
            } else {
                let current = comp.children.get(e.layer_idx)
                    .map(|(_u, attrs)| attrs.layer_start())
                    .unwrap_or(0);
                let delta = e.new_play_start - current;
                let _ = comp.trim_layers(&[e.layer_idx], delta, true);
            }
        });
        return Some(result);
    }
    if let Some(e) = downcast_event::<SetLayerPlayEndEvent>(&event) {
        project.modify_comp(e.comp_uuid, |comp| {
            let dragged_uuid = comp.idx_to_uuid(e.layer_idx).unwrap_or_default();
            if comp.is_multi_selected(dragged_uuid) {
                let dragged_pe = comp.child_end(dragged_uuid);
                let delta = e.new_play_end - dragged_pe;
                let selection_indices = comp.uuids_to_indices(&comp.layer_selection);
                let _ = comp.trim_layers(&selection_indices, delta, false);
            } else {
                let current = comp.children.get(e.layer_idx)
                    .map(|(_u, attrs)| attrs.layer_end())
                    .unwrap_or(0);
                let delta = e.new_play_end - current;
                let _ = comp.trim_layers(&[e.layer_idx], delta, false);
            }
        });
        return Some(result);
    }
    if let Some(e) = downcast_event::<LayerAttributesChangedEvent>(&event) {
        project.modify_comp(e.comp_uuid, |comp| {
            use crate::entities::AttrValue;
            // Apply to all targeted layers (multi-selection support)
            for layer_uuid in &e.layer_uuids {
                comp.set_child_attrs(layer_uuid, &[
                    ("visible", AttrValue::Bool(e.visible)),
                    ("opacity", AttrValue::Float(e.opacity)),
                    ("blend_mode", AttrValue::Str(e.blend_mode.clone())),
                    ("speed", AttrValue::Float(e.speed)),
                ]);
            }
        });
        return Some(result);
    }
    if let Some(e) = downcast_event::<AlignLayersStartEvent>(&event) {
        project.modify_comp(e.0, |comp| {
            let current_frame = comp.frame();
            let selected = comp.layer_selection.clone();
            for layer_uuid in selected {
                let Some(layer_idx) = comp.uuid_to_idx(layer_uuid) else { continue };
                let (play_start, _) = comp.child_work_area_abs(layer_uuid).unwrap_or_else(|| {
                    (comp.child_start(layer_uuid), comp.child_end(layer_uuid))
                });
                let layer_in = comp.child_in(layer_uuid);
                let delta = current_frame - play_start;
                let _ = comp.move_child(layer_idx, layer_in + delta);
            }
        });
        return Some(result);
    }
    if let Some(e) = downcast_event::<AlignLayersEndEvent>(&event) {
        project.modify_comp(e.0, |comp| {
            let current_frame = comp.frame();
            let selected = comp.layer_selection.clone();
            for layer_uuid in selected {
                let Some(layer_idx) = comp.uuid_to_idx(layer_uuid) else { continue };
                let (_, play_end) = comp.child_work_area_abs(layer_uuid).unwrap_or_else(|| {
                    (comp.child_start(layer_uuid), comp.child_end(layer_uuid))
                });
                let layer_in = comp.child_in(layer_uuid);
                let delta = current_frame - play_end;
                let _ = comp.move_child(layer_idx, layer_in + delta);
            }
        });
        return Some(result);
    }
    if let Some(e) = downcast_event::<TrimLayersStartEvent>(&event) {
        project.modify_comp(e.0, |comp| {
            let current_frame = comp.frame();
            let selected = comp.layer_selection.clone();
            for layer_uuid in selected {
                let Some(layer_idx) = comp.uuid_to_idx(layer_uuid) else { continue };
                let _ = comp.set_child_start(layer_idx, current_frame);
            }
        });
        return Some(result);
    }
    if let Some(e) = downcast_event::<TrimLayersEndEvent>(&event) {
        project.modify_comp(e.0, |comp| {
            let current_frame = comp.frame();
            let selected = comp.layer_selection.clone();
            for layer_uuid in selected {
                let Some(layer_idx) = comp.uuid_to_idx(layer_uuid) else { continue };
                let _ = comp.set_child_end(layer_idx, current_frame);
            }
        });
        return Some(result);
    }
    if let Some(e) = downcast_event::<MoveLayerEvent>(&event) {
        project.modify_comp(e.comp_uuid, |comp| {
            let _ = comp.move_child(e.layer_idx, e.new_start);
        });
        return Some(result);
    }

    // === Layer Clipboard Operations ===
    if let Some(e) = downcast_event::<DuplicateLayersEvent>(&event) {
        debug!("DuplicateLayersEvent: comp={}", e.comp_uuid);
        // Duplicate selected layers, insert copies above originals
        let layers_to_dup: Vec<(Uuid, crate::entities::Attrs, i32)> = project
            .get_comp(e.comp_uuid)
            .map(|comp| {
                comp.layer_selection
                    .iter()
                    .filter_map(|uuid| {
                        comp.children_attrs_get(uuid).map(|attrs| {
                            let start = attrs.get_i32("in").unwrap_or(0);
                            (*uuid, attrs.clone(), start)
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        if layers_to_dup.is_empty() {
            debug!("Duplicate: no layers selected");
        } else {
            debug!("Duplicating {} layers", layers_to_dup.len());
            // Generate names before taking write lock
            let names: Vec<String> = layers_to_dup
                .iter()
                .map(|(_, attrs, _)| {
                    let src_name = attrs.get_str("name").unwrap_or("layer");
                    project.gen_name(src_name)
                })
                .collect();

            // Collect new UUIDs to select only duplicated layers
            let mut new_uuids: Vec<Uuid> = Vec::new();

            project.modify_comp(e.comp_uuid, |comp| {
                // Clear selection - will select only new layers
                comp.layer_selection.clear();

                for ((src_uuid, mut attrs, _start), new_name) in layers_to_dup.into_iter().zip(names) {
                    // Find insert position (above original)
                    let insert_idx = comp.uuid_to_idx(src_uuid).unwrap_or(0);
                    // Update attrs with new name
                    attrs.set("name", crate::entities::AttrValue::Str(new_name.clone()));
                    // Insert new layer
                    let new_uuid = Uuid::new_v4();
                    comp.children.insert(insert_idx, (new_uuid, attrs));
                    new_uuids.push(new_uuid);
                    debug!("  Duplicated -> {} at idx {}", new_name, insert_idx);
                }
                // Select only the new duplicated layers
                comp.layer_selection = new_uuids;
                comp.attrs.mark_dirty();
            });
        }
        return Some(result);
    }
    if let Some(e) = downcast_event::<CopyLayersEvent>(&event) {
        debug!("CopyLayersEvent: comp={}", e.comp_uuid);
        // Copy selected layers to clipboard
        if let Some(comp) = project.get_comp(e.comp_uuid) {
            if comp.layer_selection.is_empty() {
                debug!("Copy: no layers selected");
            } else {
                let mut clipboard_items: Vec<crate::widgets::timeline::ClipboardLayer> = Vec::new();
                for uuid in &comp.layer_selection {
                    if let Some(attrs) = comp.children_attrs_get(uuid) {
                        let source_uuid = attrs
                            .get_str("uuid")
                            .and_then(|s| Uuid::parse_str(s).ok())
                            .unwrap_or(*uuid);
                        let original_start = attrs.get_i32("in").unwrap_or(0);
                        let name = attrs.get_str("name").unwrap_or("?");
                        debug!("  Copy layer '{}' at frame {}", name, original_start);
                        clipboard_items.push(crate::widgets::timeline::ClipboardLayer {
                            source_uuid,
                            attrs: attrs.clone(),
                            original_start,
                        });
                    }
                }
                // Sort by original_start for consistent paste order
                clipboard_items.sort_by_key(|item| item.original_start);
                timeline_state.clipboard = clipboard_items;
                debug!("Copied {} layers to clipboard", timeline_state.clipboard.len());
            }
        } else {
            debug!("Copy: comp not found");
        }
        return Some(result);
    }
    if let Some(e) = downcast_event::<PasteLayersEvent>(&event) {
        debug!("PasteLayersEvent: comp={}, frame={}", e.comp_uuid, e.target_frame);
        // Paste layers from clipboard at target frame
        if timeline_state.clipboard.is_empty() {
            debug!("Paste: clipboard is empty");
        } else {
            // Calculate offset from first layer's original position
            let first_start = timeline_state.clipboard.first().map(|l| l.original_start).unwrap_or(0);
            let offset = e.target_frame - first_start;
            debug!("Pasting {} layers with offset {}", timeline_state.clipboard.len(), offset);

            // Generate names before taking write lock
            let names: Vec<String> = timeline_state
                .clipboard
                .iter()
                .map(|item| {
                    let src_name = item.attrs.get_str("name").unwrap_or("layer");
                    project.gen_name(src_name)
                })
                .collect();

            let clipboard_copy = timeline_state.clipboard.clone();
            project.modify_comp(e.comp_uuid, |comp| {
                comp.layer_selection.clear();
                let mut insert_idx = 0; // Track insert position to maintain order
                for (item, new_name) in clipboard_copy.into_iter().zip(names) {
                    let mut attrs = item.attrs.clone();
                    // Update name
                    attrs.set("name", crate::entities::AttrValue::Str(new_name.clone()));
                    // Shift both in and out by offset to preserve duration
                    let old_in = attrs.get_i32("in").unwrap_or(0);
                    let old_out = attrs.get_i32("out").unwrap_or(old_in + 100);
                    let new_in = old_in + offset;
                    let new_out = old_out + offset;
                    attrs.set("in", crate::entities::AttrValue::Int(new_in));
                    attrs.set("out", crate::entities::AttrValue::Int(new_out));
                    // Insert at tracked position (maintains order)
                    let new_uuid = Uuid::new_v4();
                    comp.children.insert(insert_idx, (new_uuid, attrs));
                    insert_idx += 1; // Next layer goes after this one
                    // Select pasted layer
                    comp.layer_selection.push(new_uuid);
                    debug!("  Pasted '{}' at frames {}..{}", new_name, new_in, new_out);
                }
                comp.attrs.mark_dirty();
            });
            debug!("Paste complete: {} layers", timeline_state.clipboard.len());
        }
        return Some(result);
    }
    if let Some(e) = downcast_event::<SelectAllLayersEvent>(&event) {
        debug!("SelectAllLayersEvent: comp={}", e.comp_uuid);
        project.modify_comp(e.comp_uuid, |comp| {
            let all_uuids: Vec<Uuid> = comp.children.iter().map(|(u, _)| *u).collect();
            debug!("Selecting all {} layers", all_uuids.len());
            comp.layer_selection = all_uuids;
            comp.layer_selection_anchor = comp.children.first().map(|(u, _)| *u);
        });
        return Some(result);
    }
    if let Some(e) = downcast_event::<ClearLayerSelectionEvent>(&event) {
        debug!("ClearLayerSelectionEvent: comp={}", e.comp_uuid);
        project.modify_comp(e.comp_uuid, |comp| {
            debug!("Clearing {} selected layers", comp.layer_selection.len());
            comp.layer_selection.clear();
            comp.layer_selection_anchor = None;
        });
        return Some(result);
    }

    // === Comp Play Area ===
    if let Some(e) = downcast_event::<SetCompPlayStartEvent>(&event) {
        project.modify_comp(e.comp_uuid, |comp| {
            comp.set_comp_play_start(e.frame);
        });
        return Some(result);
    }
    if let Some(e) = downcast_event::<SetCompPlayEndEvent>(&event) {
        project.modify_comp(e.comp_uuid, |comp| {
            comp.set_comp_play_end(e.frame);
        });
        return Some(result);
    }
    if let Some(e) = downcast_event::<ResetCompPlayAreaEvent>(&event) {
        project.modify_comp(e.0, |comp| {
            let start = comp._in();
            let end = comp._out();
            comp.set_comp_play_start(start);
            comp.set_comp_play_end(end);
        });
        return Some(result);
    }

    // Event not handled
    None
}
