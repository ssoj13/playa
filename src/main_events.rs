//! Application event handling - extracted from main.rs for clarity.
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

/// Strip trailing numbers and extension from filename
/// "clipper_runs_0017.tga" -> "clipper_runs"
/// "shot_001" -> "shot"
fn strip_trailing_numbers(name: &str) -> String {
    // Remove extension if present
    let name = name.rsplit_once('.').map(|(n, _)| n).unwrap_or(name);
    // Remove trailing _NNNN or NNNN pattern
    let name = name.trim_end_matches(|c: char| c.is_ascii_digit());
    let name = name.trim_end_matches('_');
    if name.is_empty() { "layer".to_string() } else { name.to_string() }
}

/// Extract suffix number from layer name if it matches base pattern
/// "clipper_runs_3" with base "clipper_runs" -> Some(3)
fn extract_suffix_number(name: &str, base: &str) -> Option<u32> {
    if !name.starts_with(base) { return None; }
    let suffix = &name[base.len()..];
    let suffix = suffix.trim_start_matches('_');
    suffix.parse().ok()
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
            project.modify_comp(comp_uuid, |comp| {
                let current = comp.frame();
                let edges = comp.get_child_edges_near(current);
                if !edges.is_empty() {
                    if let Some((frame, _)) = edges.iter().rev().find(|(f, _)| *f < current) {
                        comp.set_frame(*frame);
                    } else if let Some((frame, _)) = edges.last() {
                        comp.set_frame(*frame);
                    }
                }
            });
        }
        return Some(result);
    }
    if downcast_event::<JumpToNextEdgeEvent>(&event).is_some() {
        if let Some(comp_uuid) = player.active_comp() {
            project.modify_comp(comp_uuid, |comp| {
                let current = comp.frame();
                let edges = comp.get_child_edges_near(current);
                if !edges.is_empty() {
                    if let Some((frame, _)) = edges.iter().find(|(f, _)| *f > current) {
                        comp.set_frame(*frame);
                    } else if let Some((frame, _)) = edges.first() {
                        comp.set_frame(*frame);
                    }
                }
            });
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
        player.increase_fps_base();
        if let Some(comp_uuid) = player.active_comp() {
            let fps = player.fps_base();
            project.modify_comp(comp_uuid, |comp| comp.set_fps(fps));
        }
        return Some(result);
    }
    if downcast_event::<DecreaseFPSBaseEvent>(&event).is_some() {
        player.decrease_fps_base();
        if let Some(comp_uuid) = player.active_comp() {
            let fps = player.fps_base();
            project.modify_comp(comp_uuid, |comp| comp.set_fps(fps));
        }
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
            if !project.media.read().unwrap().contains_key(&a) {
                let first = project.comps_order().first().cloned();
                player.set_active_comp(first, project);
            }
        }
        return Some(result);
    }
    if downcast_event::<ClearAllMediaEvent>(&event).is_some() {
        project.media.write().unwrap().clear();
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
            let media = project.media.read().unwrap();
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
            let media = project.media.read().unwrap();
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
        // Get source comp info for naming and duration
        let source_info = project.get_comp(e.source_uuid).map(|s| {
            (s.frame_count(), s.dim(), s.name().to_string())
        });

        let add_result = {
            let mut media = project.media.write().unwrap();
            if let Some(comp) = media.get_mut(&e.comp_uuid) {
                if let Some(target_row) = e.target_row {
                    let (duration, source_dim, _) = source_info.clone().unwrap_or((1, (64, 64), String::new()));
                    comp.add_child_with_duration(e.source_uuid, e.start_frame, duration, Some(target_row), source_dim)
                } else {
                    comp.add_child(e.source_uuid, e.start_frame, project)
                }
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

            // Auto-name the new layer based on source name
            if let Some((_, _, source_name)) = source_info {
                let base_name = strip_trailing_numbers(&source_name);
                project.modify_comp(e.comp_uuid, |parent_comp| {
                    // Find next available number for this base name
                    let mut max_num = 0;
                    for (_, attrs) in parent_comp.get_children() {
                        if let Some(name) = attrs.get_str("name") {
                            if let Some(num) = extract_suffix_number(name, &base_name) {
                                max_num = max_num.max(num);
                            }
                        }
                    }
                    let new_name = format!("{}_{}", base_name, max_num + 1);
                    // Set name on the last added child (most recent)
                    if let Some((child_uuid, _)) = parent_comp.get_children().last() {
                        let child_uuid = *child_uuid;
                        if let Some(attrs) = parent_comp.children_attrs_get_mut(&child_uuid) {
                            attrs.set("name", crate::entities::AttrValue::Str(new_name));
                        }
                    }
                });
            }
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
                let dragged_start = comp.child_start(dragged_uuid);
                let delta = e.new_start - dragged_start;
                let selection_indices = comp.uuids_to_indices(&comp.layer_selection);
                let _ = comp.move_layers(&selection_indices, delta, Some(e.new_idx));
            } else {
                let dragged_start = comp.child_start(dragged_uuid);
                let delta = e.new_start - dragged_start;
                let _ = comp.move_layers(&[e.layer_idx], delta, Some(e.new_idx));
            }
        });
        return Some(result);
    }
    if let Some(e) = downcast_event::<SetLayerPlayStartEvent>(&event) {
        project.modify_comp(e.comp_uuid, |comp| {
            let dragged_uuid = comp.idx_to_uuid(e.layer_idx).unwrap_or_default();
            if comp.is_multi_selected(dragged_uuid) {
                let dragged_ps = comp.child_play_start(dragged_uuid);
                let delta = e.new_play_start - dragged_ps;
                let selection_indices = comp.uuids_to_indices(&comp.layer_selection);
                let _ = comp.trim_layers(&selection_indices, delta, true);
            } else {
                let delta = {
                    let current = comp.children.get(e.layer_idx)
                        .map(|(_u, attrs)| attrs.get_i32("trim_in").unwrap_or(attrs.get_i32("in").unwrap_or(0)))
                        .unwrap_or(0);
                    e.new_play_start - current
                };
                let _ = comp.trim_layers(&[e.layer_idx], delta, true);
            }
        });
        return Some(result);
    }
    if let Some(e) = downcast_event::<SetLayerPlayEndEvent>(&event) {
        project.modify_comp(e.comp_uuid, |comp| {
            let dragged_uuid = comp.idx_to_uuid(e.layer_idx).unwrap_or_default();
            if comp.is_multi_selected(dragged_uuid) {
                let dragged_pe = comp.child_play_end(dragged_uuid);
                let delta = e.new_play_end - dragged_pe;
                let selection_indices = comp.uuids_to_indices(&comp.layer_selection);
                let _ = comp.trim_layers(&selection_indices, delta, false);
            } else {
                let delta = {
                    let current = comp.children.get(e.layer_idx)
                        .map(|(_u, attrs)| attrs.get_i32("trim_out").unwrap_or(attrs.get_i32("out").unwrap_or(0)))
                        .unwrap_or(0);
                    e.new_play_end - current
                };
                let _ = comp.trim_layers(&[e.layer_idx], delta, false);
            }
        });
        return Some(result);
    }
    if let Some(e) = downcast_event::<LayerAttributesChangedEvent>(&event) {
        project.modify_comp(e.comp_uuid, |comp| {
            use crate::entities::AttrValue;
            comp.set_child_attrs(&e.layer_uuid, &[
                ("visible", AttrValue::Bool(e.visible)),
                ("opacity", AttrValue::Float(e.opacity)),
                ("blend_mode", AttrValue::Str(e.blend_mode.clone())),
                ("speed", AttrValue::Float(e.speed)),
            ]);
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
                let child_start = comp.child_start(layer_uuid);
                let delta = current_frame - play_start;
                let _ = comp.move_child(layer_idx, child_start + delta);
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
                let child_start = comp.child_start(layer_uuid);
                let delta = current_frame - play_end;
                let _ = comp.move_child(layer_idx, child_start + delta);
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
                let _ = comp.set_child_play_start(layer_idx, current_frame);
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
                let _ = comp.set_child_play_end(layer_idx, current_frame);
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
