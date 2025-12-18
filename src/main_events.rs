//! Application event handling - extracted from main.rs for clarity.
//!
//! # Dirty Tracking & Event Architecture
//!
//! This app uses event-driven architecture. UI never computes frames directly.
//! Instead, changes emit events that trigger background work in workers.
//!
//! ## Two separate `mark_dirty()` systems:
//!
//! 1. **`comp.attrs.mark_dirty()`** - marks CompNode for recomputation.
//!    Called inside `project.modify_comp()` closures after changing comp data.
//!    `modify_comp()` then checks `is_dirty()` and emits `AttrsChangedEvent`
//!    which invalidates frame cache and triggers re-render.
//!
//! 2. **`node_editor_state.mark_dirty()`** - marks graph editor UI for redraw.
//!    Completely separate system, only affects node graph visualization.
//!    Does NOT affect frame computation or caching.
//!
//! ## When to use `comp.attrs.mark_dirty()` inside `modify_comp()`:
//!
//! - Direct field changes: `comp.layers = ...`, `comp.layers.insert/remove`
//! - Direct layer attr changes: `layer.attrs.set(...)`
//!
//! NOT needed when using CompNode methods that already call mark_dirty():
//! - `comp.move_layers()`, `comp.trim_layers()`, `comp.set_child_attrs()`
//! - `comp.add_layer()`, `comp.remove_layer()`
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

use log::trace;
use std::path::PathBuf;
use uuid::Uuid;

use crate::dialogs::encode::EncodeDialog;
use crate::entities::Project;
use crate::entities::node::Node;
use crate::core::event_bus::{BoxedEvent, downcast_event};
use crate::core::player::Player;
use crate::core::player_events::*;
use crate::widgets::project::project_events::*;
use crate::entities::comp_events::*;
use crate::widgets::timeline::timeline_events::*;
use crate::widgets::viewport::viewport_events::*;
use crate::widgets::viewport::tool::SetToolEvent;
use crate::widgets::node_editor::node_events::*;
use crate::dialogs::prefs::prefs_events::*;
use crate::entities::keys::{A_IN, A_OUT, A_SPEED, A_TRIM_IN, A_TRIM_OUT};

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

/// Scan folder for image sequences using scanseq.
/// Returns first frame of each detected sequence for Comp::detect_from_paths.
fn scan_folder_for_media(root: &std::path::Path) -> Vec<PathBuf> {
    use scanseq::core::{Scanner, scan_files, VIDEO_EXTS};

    let mut all_paths: Vec<PathBuf> = Vec::new();

    // Use scanseq for image sequences (min_len=5 to filter short sequences)
    let scanner = Scanner::path(root)
        .recursive(true)
        .min_len(5)
        .scan();

    trace!("scanseq found {} sequences in {:.1}ms",
        scanner.len(),
        scanner.result.elapsed_ms
    );

    // Add first file of each sequence
    for seq in scanner.iter() {
        all_paths.push(PathBuf::from(seq.first_file()));
    }

    // Also scan for video files
    match scan_files(&[root], true, VIDEO_EXTS) {
        Ok(videos) => {
            trace!("scanseq found {} video files", videos.len());
            all_paths.extend(videos);
        }
        Err(e) => {
            trace!("Failed to scan videos: {}", e);
        }
    }

    // Sort for deterministic order
    all_paths.sort();
    all_paths
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
    pub new_camera: Option<String>,
    pub new_text: Option<(String, String)>,
    pub enqueue_frames: bool,
    pub quick_save: bool,
    pub show_open_dialog: bool,
    /// Update AE panel focus (SelectionFocusEvent)
    pub ae_focus_update: Option<Vec<Uuid>>,
}

impl EventResult {
    /// Merge another result into this one (accumulates multiple event results)
    pub fn merge(&mut self, other: EventResult) {
        // Last write wins for single values
        if other.load_project.is_some() {
            self.load_project = other.load_project;
        }
        if other.save_project.is_some() {
            self.save_project = other.save_project;
        }
        if other.new_comp.is_some() {
            self.new_comp = other.new_comp;
        }
        if other.new_camera.is_some() {
            self.new_camera = other.new_camera;
        }
        if other.new_text.is_some() {
            self.new_text = other.new_text;
        }
        self.enqueue_frames |= other.enqueue_frames;
        // Accumulate paths instead of overwriting
        if let Some(paths) = other.load_sequences {
            self.load_sequences.get_or_insert_with(Vec::new).extend(paths);
        }
        // Bool flags: set to true if any event sets them
        self.quick_save |= other.quick_save;
        self.show_open_dialog |= other.show_open_dialog;
        // AE focus: last write wins
        if other.ae_focus_update.is_some() {
            self.ae_focus_update = other.ae_focus_update;
        }
    }
}

/// Handle a single app event (called from main event loop).
/// Returns Some(result) if event was handled, None otherwise.
pub fn handle_app_event(
    event: &BoxedEvent,
    player: &mut Player,
    project: &mut Project,
    timeline_state: &mut crate::widgets::timeline::TimelineState,
    node_editor_state: &mut crate::widgets::node_editor::NodeEditorState,
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
    if downcast_event::<TogglePlayPauseEvent>(event).is_some() {
        let was_playing = player.is_playing();
        player.set_is_playing(!was_playing);
        if player.is_playing() {
            trace!("TogglePlayPause: starting playback at frame {}", player.current_frame(project));
            player.last_frame_time = Some(std::time::Instant::now());
        } else {
            trace!("TogglePlayPause: pausing at frame {}", player.current_frame(project));
            player.last_frame_time = None;
            player.set_fps_play(player.fps_base());
        }
        return Some(result);
    }
    if downcast_event::<StopEvent>(event).is_some() {
        player.stop();
        return Some(result);
    }
    if let Some(e) = downcast_event::<SetFrameEvent>(event) {
        trace!("SetFrame: moving to frame {}", e.0);
        if let Some(comp_uuid) = player.active_comp() {
            // Get old frame before setting new one (for distance calculation)
            let old_frame = project.with_comp(comp_uuid, |comp| comp.frame()).unwrap_or(e.0);
            let distance = (e.0 - old_frame).abs();
            
            // Big jump (scrub/seek) vs sequential (playback):
            // - distance > 1: user jumped to new position, cancel old preload tasks
            // - distance <= 1: sequential playback, keep loading frames ahead
            if distance > 1 {
                if let Some(manager) = project.cache_manager() {
                    manager.increment_epoch();
                    trace!("SetFrame: jump detected (distance={}), epoch incremented", distance);
                }
            }
            
            project.modify_comp(comp_uuid, |comp| {
                comp.set_frame(e.0);
            });
            result.enqueue_frames = true;
        }
        return Some(result);
    }
    if downcast_event::<StepForwardEvent>(event).is_some() {
        player.step(1, project);
        return Some(result);
    }
    if downcast_event::<StepBackwardEvent>(event).is_some() {
        player.step(-1, project);
        return Some(result);
    }
    if downcast_event::<StepForwardLargeEvent>(event).is_some() {
        player.step(crate::core::player::FRAME_JUMP_STEP, project);
        return Some(result);
    }
    if downcast_event::<StepBackwardLargeEvent>(event).is_some() {
        player.step(-crate::core::player::FRAME_JUMP_STEP, project);
        return Some(result);
    }
    if downcast_event::<JumpToStartEvent>(event).is_some() {
        player.to_start(project);
        return Some(result);
    }
    if downcast_event::<JumpToEndEvent>(event).is_some() {
        player.to_end(project);
        return Some(result);
    }
    if downcast_event::<JumpToPrevEdgeEvent>(event).is_some() {
        if let Some(comp_uuid) = player.active_comp() {
            project.modify_comp(comp_uuid, |comp| jump_to_edge(comp, false));
        }
        return Some(result);
    }
    if downcast_event::<JumpToNextEdgeEvent>(event).is_some() {
        if let Some(comp_uuid) = player.active_comp() {
            project.modify_comp(comp_uuid, |comp| jump_to_edge(comp, true));
        }
        return Some(result);
    }
    if downcast_event::<JogForwardEvent>(event).is_some() {
        player.jog_forward();
        return Some(result);
    }
    if downcast_event::<JogBackwardEvent>(event).is_some() {
        player.jog_backward();
        return Some(result);
    }

    // === FPS Control ===
    if downcast_event::<IncreaseFPSBaseEvent>(event).is_some() {
        adjust_fps_base(player, project, true);
        return Some(result);
    }
    if downcast_event::<DecreaseFPSBaseEvent>(event).is_some() {
        adjust_fps_base(player, project, false);
        return Some(result);
    }

    // === Play Range Control ===
    if downcast_event::<SetPlayRangeStartEvent>(event).is_some() {
        log::trace!("[B] SetPlayRangeStartEvent received, active_comp={:?}", player.active_comp());
        if let Some(comp_uuid) = player.active_comp() {
            project.modify_comp(comp_uuid, |comp| {
                let current = comp.frame();
                comp.set_comp_play_start(current);
            });
        }
        return Some(result);
    }
    if downcast_event::<SetPlayRangeEndEvent>(event).is_some() {
        log::trace!("[N] SetPlayRangeEndEvent received, active_comp={:?}", player.active_comp());
        if let Some(comp_uuid) = player.active_comp() {
            project.modify_comp(comp_uuid, |comp| {
                let current = comp.frame();
                comp.set_comp_play_end(current);
            });
        }
        return Some(result);
    }
    if downcast_event::<ResetPlayRangeEvent>(event).is_some() {
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
    if downcast_event::<ToggleLoopEvent>(event).is_some() {
        settings.loop_enabled = !settings.loop_enabled;
        player.set_loop_enabled(settings.loop_enabled);
        return Some(result);
    }
    if let Some(e) = downcast_event::<SetLoopEvent>(event) {
        settings.loop_enabled = e.0;
        player.set_loop_enabled(e.0);
        return Some(result);
    }

    // === Project Management ===
    if let Some(e) = downcast_event::<AddClipEvent>(event) {
        result.load_sequences = Some(vec![e.0.clone()]);
        return Some(result);
    }
    if let Some(e) = downcast_event::<AddClipsEvent>(event) {
        result.load_sequences = Some(e.0.clone());
        return Some(result);
    }
    // AddFolderEvent: scan directory recursively for media files
    if let Some(e) = downcast_event::<AddFolderEvent>(event) {
        trace!("AddFolderEvent: scanning {}", e.0.display());
        let media_files = scan_folder_for_media(&e.0);
        if !media_files.is_empty() {
            trace!("Found {} media files in folder", media_files.len());
            result.load_sequences = Some(media_files);
        } else {
            trace!("No media files found in folder");
        }
        return Some(result);
    }
    if let Some(e) = downcast_event::<AddCompEvent>(event) {
        result.new_comp = Some((e.name.clone(), e.fps));
        return Some(result);
    }
    if let Some(e) = downcast_event::<AddCameraEvent>(event) {
        result.new_camera = Some(e.name.clone());
        return Some(result);
    }
    if let Some(e) = downcast_event::<AddTextEvent>(event) {
        result.new_text = Some((e.name.clone(), e.text.clone()));
        return Some(result);
    }
    if let Some(e) = downcast_event::<SaveProjectEvent>(event) {
        result.save_project = Some(e.0.clone());
        return Some(result);
    }
    if let Some(e) = downcast_event::<LoadProjectEvent>(event) {
        result.load_project = Some(e.0.clone());
        return Some(result);
    }
    if downcast_event::<QuickSaveEvent>(event).is_some() {
        result.quick_save = true;
        return Some(result);
    }
    if downcast_event::<OpenProjectDialogEvent>(event).is_some() {
        result.show_open_dialog = true;
        return Some(result);
    }
    if let Some(e) = downcast_event::<RemoveMediaEvent>(event) {
        let uuid = e.0;
        let was_active = player.active_comp() == Some(uuid);
        project.del_comp(uuid);
        if was_active {
            let first = project.order().first().cloned();
            player.set_active_comp(first, project);
            if let Some(f) = first {
                node_editor_state.set_comp(f);
            }
        }
        return Some(result);
    }
    if downcast_event::<RemoveSelectedMediaEvent>(event).is_some() {
        let selection: Vec<Uuid> = project.selection();
        let active = player.active_comp();
        for uuid in selection {
            project.del_comp(uuid);
        }
        // Fix active if deleted
        if let Some(a) = active
            && !project.media.read().expect("media lock poisoned").contains_key(&a) {
                let first = project.order().first().cloned();
                player.set_active_comp(first, project);
                if let Some(f) = first {
                    node_editor_state.set_comp(f);
                }
            }
        return Some(result);
    }
    if downcast_event::<ClearAllMediaEvent>(event).is_some() {
        project.media.write().expect("media lock poisoned").clear();
        project.set_order(Vec::new());
        project.set_selection(Vec::new());
        player.set_active_comp(None, project);
        return Some(result);
    }
    if let Some(e) = downcast_event::<SelectMediaEvent>(event) {
        player.set_active_comp(Some(e.0), project); // also resets selection
        project.selection_anchor = project.order().iter().position(|u| *u == e.0);
        node_editor_state.set_comp(e.0);
        return Some(result);
    }

    // === Selection ===
    if let Some(e) = downcast_event::<ProjectSelectionChangedEvent>(event) {
        project.set_selection(e.selection.clone());
        project.selection_anchor = e.anchor.or_else(|| {
            let sel = project.selection();
            let order = project.order();
            sel.last().and_then(|u| order.iter().position(|x| x == u))
        });
        return Some(result);
    }
    // SelectionFocusEvent: update AE panel focus
    if let Some(e) = downcast_event::<SelectionFocusEvent>(event) {
        log::trace!("SelectionFocusEvent -> ae_focus={:?}", e.0);
        result.ae_focus_update = Some(e.0.clone());
        return Some(result);
    }
    if let Some(e) = downcast_event::<ProjectActiveChangedEvent>(event) {
        player.set_active_comp(Some(e.uuid), project); // also resets selection
        project.selection_anchor = project.order().iter().position(|u| *u == e.uuid);
        node_editor_state.set_comp(e.uuid);
        
        // If target_frame specified (dive-into-comp), set frame in new comp
        if let Some(local_frame) = e.target_frame {
            // Add child comp's "in" offset to get absolute frame
            project.modify_comp(e.uuid, |comp| {
                let comp_in = comp.attrs().get_i32(A_IN).unwrap_or(0);
                comp.set_frame(comp_in + local_frame);
            });
        }
        return Some(result);
    }
    if downcast_event::<ProjectPreviousCompEvent>(event).is_some() {
        if let Some(prev) = player.previous_comp() {
            player.set_active_comp(Some(prev), project);
            project.selection_anchor = project.order().iter().position(|u| *u == prev);
            node_editor_state.set_comp(prev);
        }
        return Some(result);
    }
    if let Some(e) = downcast_event::<CompSelectionChangedEvent>(event) {
        project.modify_comp(e.comp_uuid, |comp| {
            comp.layer_selection = e.selection.clone();
            comp.layer_selection_anchor = e.anchor;
        });
        return Some(result);
    }

    // === UI State ===
    if downcast_event::<ToggleHelpEvent>(event).is_some() {
        *show_help = !*show_help;
        return Some(result);
    }
    if downcast_event::<TogglePlaylistEvent>(event).is_some() {
        *show_playlist = !*show_playlist;
        return Some(result);
    }
    if downcast_event::<ToggleSettingsEvent>(event).is_some() {
        *show_settings = !*show_settings;
        return Some(result);
    }
    if downcast_event::<ToggleAttributeEditorEvent>(event).is_some() {
        *show_attributes_editor = !*show_attributes_editor;
        return Some(result);
    }
    if let Some(e) = downcast_event::<SetGizmoPrefsEvent>(event) {
        project.set_gizmo_prefs(&e.0);
        return Some(result);
    }
    if downcast_event::<ToggleEncodeDialogEvent>(event).is_some() {
        *show_encode_dialog = !*show_encode_dialog;
        if *show_encode_dialog && encode_dialog.is_none() {
            trace!("[ToggleEncodeDialog] Opening encode dialog");
            *encode_dialog = Some(EncodeDialog::load_from_settings(&settings.encode_dialog));
        }
        return Some(result);
    }
    if downcast_event::<ToggleFullscreenEvent>(event).is_some() {
        *is_fullscreen = !*is_fullscreen;
        *fullscreen_dirty = true;
        return Some(result);
    }
    if downcast_event::<ToggleFrameNumbersEvent>(event).is_some() {
        settings.show_frame_numbers = !settings.show_frame_numbers;
        return Some(result);
    }
    if downcast_event::<ResetSettingsEvent>(event).is_some() {
        *reset_settings_pending = true;
        return Some(result);
    }

    // === Timeline State ===
    if let Some(e) = downcast_event::<TimelineZoomChangedEvent>(event) {
        timeline_state.zoom = e.0.clamp(0.1, 20.0);
        return Some(result);
    }
    if let Some(e) = downcast_event::<TimelinePanChangedEvent>(event) {
        timeline_state.pan_offset = e.0;
        return Some(result);
    }
    if let Some(e) = downcast_event::<TimelineSnapChangedEvent>(event) {
        timeline_state.snap_enabled = e.0;
        return Some(result);
    }
    if let Some(e) = downcast_event::<TimelineLockWorkAreaChangedEvent>(event) {
        timeline_state.lock_work_area = e.0;
        return Some(result);
    }
    if let Some(e) = downcast_event::<TimelineFitAllEvent>(event) {
        if let Some(comp_uuid) = player.active_comp() {
            let media = project.media.read().expect("media lock poisoned");
            if let Some(comp) = media.get(&comp_uuid) {
                let (min_frame, max_frame) = comp.bounds(true, false);
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
    if let Some(e) = downcast_event::<TimelineFitEvent>(event) {
        let canvas_width = timeline_state.last_canvas_width;
        if let Some(comp_uuid) = player.active_comp() {
            let media = project.media.read().expect("media lock poisoned");
            if let Some(comp) = media.get(&comp_uuid) {
                // If selected_only, use selection bounds (falls back to all if none selected)
                let (min_frame, max_frame) = comp.bounds(true, e.selected_only);
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
    // Fit to work area (play range set by B/N). Defaults to full comp if not trimmed.
    if downcast_event::<TimelineFitWorkAreaEvent>(event).is_some() {
        let canvas_width = timeline_state.last_canvas_width;
        if let Some(comp_uuid) = player.active_comp() {
            let media = project.media.read().expect("media lock poisoned");
            if let Some(comp) = media.get(&comp_uuid) {
                let (min_frame, max_frame) = comp.play_range(true); // use_work_area=true
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
    // Timeline zoom in/out via keyboard
    if downcast_event::<TimelineZoomInEvent>(event).is_some() {
        timeline_state.zoom = (timeline_state.zoom * 1.2).clamp(0.1, 20.0);
        return Some(result);
    }
    if downcast_event::<TimelineZoomOutEvent>(event).is_some() {
        timeline_state.zoom = (timeline_state.zoom / 1.2).clamp(0.1, 20.0);
        return Some(result);
    }

    // === Node Editor State ===
    if downcast_event::<NodeEditorFitAllEvent>(event).is_some() {
        log::trace!("[EVENT] NodeEditorFitAllEvent → setting fit_all_requested=true");
        node_editor_state.fit_all_requested = true;
        return Some(result);
    }
    if downcast_event::<NodeEditorFitSelectedEvent>(event).is_some() {
        log::trace!("[EVENT] NodeEditorFitSelectedEvent → setting fit_selected_requested=true");
        node_editor_state.fit_selected_requested = true;
        return Some(result);
    }
    if downcast_event::<NodeEditorLayoutEvent>(event).is_some() {
        node_editor_state.layout_requested = true;
        return Some(result);
    }

    // === Viewport State ===
    if let Some(e) = downcast_event::<ZoomViewportEvent>(event) {
        viewport_state.zoom *= e.0;
        return Some(result);
    }
    if downcast_event::<ResetViewportEvent>(event).is_some() {
        viewport_state.reset();
        return Some(result);
    }
    if downcast_event::<FitViewportEvent>(event).is_some() {
        viewport_state.set_mode_fit();
        return Some(result);
    }
    if downcast_event::<Viewport100Event>(event).is_some() {
        viewport_state.set_mode_100();
        return Some(result);
    }
    // Tool change (Q/W/E/R)
    if let Some(e) = downcast_event::<SetToolEvent>(event) {
        project.set_tool(e.0.as_str());
        return Some(result);
    }

    // === Layer Operations ===
    if let Some(e) = downcast_event::<AddLayerEvent>(event) {
        // Get source info and generate name BEFORE write lock
        // Use play_range() to get trimmed duration (respects B/N trim points)
        let source_info = project.with_node(e.source_uuid, |s| {
            let name = project.gen_name(s.name());
            let (start, end) = s.play_range(true);
            let trimmed_duration = (end - start + 1).max(1);
            (trimmed_duration, s.dim(), name)
        });

        let add_result = {
            let mut media = project.media.write().expect("media lock poisoned");
            if let Some(arc_node) = media.get_mut(&e.comp_uuid) {
                // Arc::make_mut: copy-on-write for mutation
                let node = std::sync::Arc::make_mut(arc_node);
                let (duration, source_dim, name) = source_info.unwrap_or((1, (64, 64), "layer_1".to_string()));
                node.add_child_layer(e.source_uuid, &name, e.start_frame, duration, e.insert_idx, source_dim)
            } else {
                Err(anyhow::anyhow!("Parent comp not found"))
            }
        };

        if let Err(err) = add_result {
            log::error!("Failed to add layer: {}", err);
        } else {
            // Graph editor UI redraw (NOT comp dirty - add_layer() handles that)
            node_editor_state.mark_dirty();
        }
        return Some(result);
    }
    if let Some(e) = downcast_event::<RemoveLayerEvent>(event) {
        let child_uuid = project.with_comp(e.comp_uuid, |comp| {
            comp.get_children().get(e.layer_idx).map(|(child_uuid, _)| *child_uuid)
        }).flatten();

        if let Some(child_uuid) = child_uuid {
            project.modify_comp(e.comp_uuid, |comp| {
                // remove_child() calls mark_dirty() internally
                comp.remove_child(child_uuid);
            });
            // Graph editor UI redraw (separate from comp dirty)
            node_editor_state.mark_dirty();
        }
        return Some(result);
    }
    if downcast_event::<RemoveSelectedLayerEvent>(event).is_some() {
        if let Some(active_uuid) = player.active_comp() {
            project.modify_comp(active_uuid, |comp| {
                let to_remove: Vec<Uuid> = comp.layer_selection.clone();
                for child_uuid in to_remove {
                    comp.remove_child(child_uuid);
                }
                comp.layer_selection.clear();
                comp.layer_selection_anchor = None;
            });
            // Graph editor UI redraw (separate from comp dirty)
            node_editor_state.mark_dirty();
        }
        return Some(result);
    }
    if let Some(e) = downcast_event::<ReorderLayerEvent>(event) {
        project.modify_comp(e.comp_uuid, |comp| {
            let children = comp.get_children();
            if e.from_idx != e.to_idx && e.from_idx < children.len() && e.to_idx < children.len() {
                let mut reordered = comp.layers.clone();
                let layer = reordered.remove(e.from_idx);
                reordered.insert(e.to_idx, layer);
                comp.layers = reordered;
                // Direct field change requires explicit mark_dirty().
                // modify_comp() will then emit AttrsChangedEvent.
                comp.attrs.mark_dirty();
            }
        });
        // No node_editor_state.mark_dirty() - reorder doesn't change graph structure
        return Some(result);
    }
    if let Some(e) = downcast_event::<MoveAndReorderLayerEvent>(event) {
        log::trace!(
            "[EVENT] MoveAndReorder received: layer_idx={} new_start={} new_idx={}",
            e.layer_idx, e.new_start, e.new_idx
        );
        project.modify_comp(e.comp_uuid, |comp| {
            let dragged_uuid = comp.idx_to_uuid(e.layer_idx).unwrap_or_default();
            let dragged_in = comp.child_in(dragged_uuid).unwrap_or(0);
            let delta = e.new_start - dragged_in;
            if comp.layer_selection.contains(&dragged_uuid) && comp.is_multi_selected() {
                let selection = comp.layer_selection.clone();
                // move_layers() calls mark_dirty() internally
                comp.move_layers(&selection, delta);
            } else {
                comp.move_layers(&[dragged_uuid], delta);
            }
            // Vertical reorder: move layer to new index
            if e.layer_idx != e.new_idx && e.new_idx < comp.layers.len() {
                let layer = comp.layers.remove(e.layer_idx);
                let insert_idx = e.new_idx.min(comp.layers.len());
                comp.layers.insert(insert_idx, layer);
                // Direct field change (layers.remove/insert) requires explicit mark_dirty().
                // Note: move_layers() above already marked dirty, but this is for the reorder part.
                comp.attrs.mark_dirty();
            }
        });
        // No node_editor_state.mark_dirty() - doesn't change graph structure
        return Some(result);
    }
    if let Some(e) = downcast_event::<SetLayerPlayStartEvent>(event) {
        project.modify_comp(e.comp_uuid, |comp| {
            let dragged_uuid = comp.idx_to_uuid(e.layer_idx).unwrap_or_default();
            let dragged_ps = comp.child_start(dragged_uuid).unwrap_or(0);
            let delta = e.new_play_start - dragged_ps;
            if comp.layer_selection.contains(&dragged_uuid) && comp.is_multi_selected() {
                let selection = comp.layer_selection.clone();
                comp.trim_layers(&selection, A_IN, delta);
            } else {
                comp.trim_layers(&[dragged_uuid], A_IN, delta);
            }
        });

        return Some(result);
    }
    if let Some(e) = downcast_event::<SetLayerPlayEndEvent>(event) {
        project.modify_comp(e.comp_uuid, |comp| {
            let dragged_uuid = comp.idx_to_uuid(e.layer_idx).unwrap_or_default();
            let dragged_pe = comp.child_end(dragged_uuid).unwrap_or(0);
            let delta = e.new_play_end - dragged_pe;
            if comp.layer_selection.contains(&dragged_uuid) && comp.is_multi_selected() {
                let selection = comp.layer_selection.clone();
                comp.trim_layers(&selection, A_OUT, delta);
            } else {
                comp.trim_layers(&[dragged_uuid], A_OUT, delta);
            }
        });

        return Some(result);
    }
    // Slide layer: move "in" while compensating trim_in/trim_out
    if let Some(e) = downcast_event::<SlideLayerEvent>(event) {
        project.modify_comp(e.comp_uuid, |comp| {
            use crate::entities::AttrValue;
            if let Some(uuid) = comp.idx_to_uuid(e.layer_idx) {
                comp.set_child_attrs(uuid, vec![
                    (A_IN, AttrValue::Int(e.new_in)),
                    (A_TRIM_IN, AttrValue::Int(e.new_trim_in)),
                    (A_TRIM_OUT, AttrValue::Int(e.new_trim_out)),
                ]);
                log::trace!(
                    "[SLIDE] layer {} -> in={}, trim_in={}, trim_out={}",
                    e.layer_idx, e.new_in, e.new_trim_in, e.new_trim_out
                );
            }
        });

        return Some(result);
    }
    // Reset trims to zero for selected layers (Ctrl+R)
    if let Some(e) = downcast_event::<ResetTrimsEvent>(event) {
        project.modify_comp(e.comp_uuid, |comp| {
            use crate::entities::AttrValue;
            for layer_uuid in comp.layer_selection.clone() {
                if let Some(layer) = comp.get_layer_mut(layer_uuid) {
                    let old_trim_in = layer.attrs.get_i32_or_zero(A_TRIM_IN);
                    let old_trim_out = layer.attrs.get_i32_or_zero(A_TRIM_OUT);
                    // Direct layer.attrs.set() doesn't mark comp dirty
                    layer.attrs.set(A_TRIM_IN, AttrValue::Int(0));
                    layer.attrs.set(A_TRIM_OUT, AttrValue::Int(0));
                    log::trace!(
                        "[RESET TRIMS] layer {} -> trim_in: {} -> 0, trim_out: {} -> 0",
                        layer_uuid, old_trim_in, old_trim_out
                    );
                }
            }
            // Direct layer.attrs.set() requires explicit mark_dirty() on comp.
            // modify_comp() will then emit AttrsChangedEvent.
            comp.attrs.mark_dirty();
        });

        return Some(result);
    }
    // NOTE: SelectAllLayersEvent and ClearLayerSelectionEvent handlers
    // are below (after clipboard events) with proper layer_selection_anchor handling
    if let Some(e) = downcast_event::<LayerAttributesChangedEvent>(event) {
        log::trace!("[LayerAttrsChanged] comp={}, layers={:?}, opacity={}", e.comp_uuid, e.layer_uuids, e.opacity);
        project.modify_comp(e.comp_uuid, |comp| {
            use crate::entities::AttrValue;
            // Apply to all targeted layers (multi-selection support)
            for layer_uuid in &e.layer_uuids {
                comp.set_child_attrs(*layer_uuid, vec![
                    ("visible", AttrValue::Bool(e.visible)),
                    ("solo", AttrValue::Bool(e.solo)),
                    ("opacity", AttrValue::Float(e.opacity)),
                    ("blend_mode", AttrValue::Str(e.blend_mode.clone())),
                    (A_SPEED, AttrValue::Float(e.speed)),
                ]);
            }
        });
        // Emit AttrsChangedEvent to trigger cache invalidation

        return Some(result);
    }
    // Generic layer attrs change (from Attribute Editor)
    if let Some(e) = downcast_event::<SetLayerAttrsEvent>(event) {
        log::trace!("[SetLayerAttrs] comp={}, layers={:?}, attrs={:?}", e.comp_uuid, e.layer_uuids, e.attrs);
        project.modify_comp(e.comp_uuid, |comp| {
            let values: Vec<(&str, crate::entities::AttrValue)> = e.attrs.iter()
                .map(|(k, v)| (k.as_str(), v.clone()))
                .collect();
            for layer_uuid in &e.layer_uuids {
                comp.set_child_attrs(*layer_uuid, values.clone());
            }
        });

        return Some(result);
    }
    // Batch per-layer transform update (from viewport gizmo)
    if let Some(e) = downcast_event::<crate::entities::comp_events::SetLayerTransformsEvent>(event) {
        project.modify_comp(e.comp_uuid, |comp| {
            use crate::entities::AttrValue;
            for (layer_uuid, pos, rot, scale) in &e.updates {
                comp.set_child_attrs(
                    *layer_uuid,
                    vec![
                        ("position", AttrValue::Vec3(*pos)),
                        ("rotation", AttrValue::Vec3(*rot)),
                        ("scale", AttrValue::Vec3(*scale)),
                    ],
                );
            }
        });

        return Some(result);
    }
    if let Some(e) = downcast_event::<AlignLayersStartEvent>(event) {
        project.modify_comp(e.0, |comp| {
            let current_frame = comp.frame();
            let selected = comp.layer_selection.clone();
            for layer_uuid in selected {
                let Some(layer_idx) = comp.uuid_to_idx(layer_uuid) else { continue };
                let (play_start, _) = comp.child_work_area_abs(layer_uuid).unwrap_or_else(|| {
                    (comp.child_start(layer_uuid).unwrap_or(0), comp.child_end(layer_uuid).unwrap_or(0))
                });
                let layer_in = comp.child_in(layer_uuid).unwrap_or(0);
                let delta = current_frame - play_start;
                let _ = comp.move_child(layer_idx, layer_in + delta);
            }
        });

        return Some(result);
    }
    if let Some(e) = downcast_event::<AlignLayersEndEvent>(event) {
        project.modify_comp(e.0, |comp| {
            let current_frame = comp.frame();
            let selected = comp.layer_selection.clone();
            for layer_uuid in selected {
                let Some(layer_idx) = comp.uuid_to_idx(layer_uuid) else { continue };
                let (_, play_end) = comp.child_work_area_abs(layer_uuid).unwrap_or_else(|| {
                    (comp.child_start(layer_uuid).unwrap_or(0), comp.child_end(layer_uuid).unwrap_or(0))
                });
                let layer_in = comp.child_in(layer_uuid).unwrap_or(0);
                let delta = current_frame - play_end;
                let _ = comp.move_child(layer_idx, layer_in + delta);
            }
        });

        return Some(result);
    }
    if let Some(e) = downcast_event::<TrimLayersStartEvent>(event) {
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
    if let Some(e) = downcast_event::<TrimLayersEndEvent>(event) {
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
    if let Some(e) = downcast_event::<MoveLayerEvent>(event) {
        project.modify_comp(e.comp_uuid, |comp| {
            let _ = comp.move_child(e.layer_idx, e.new_start);
        });

        return Some(result);
    }

    // === Layer Clipboard Operations ===
    if let Some(e) = downcast_event::<DuplicateLayersEvent>(event) {
        trace!("DuplicateLayersEvent: comp={}", e.comp_uuid);
        // Duplicate selected layers, insert copies above originals
        // Collect (layer_uuid, source_uuid, attrs_clone)
        let layers_to_dup: Vec<(Uuid, Uuid, crate::entities::Attrs)> = project
            .with_comp(e.comp_uuid, |comp| {
                comp.layer_selection
                    .iter()
                    .filter_map(|uuid| {
                        comp.get_layer(*uuid).map(|layer| {
                            (*uuid, layer.source_uuid(), layer.attrs.clone())
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        if layers_to_dup.is_empty() {
            trace!("Duplicate: no layers selected");
        } else {
            trace!("Duplicating {} layers", layers_to_dup.len());
            // Generate names before taking write lock
            let names: Vec<String> = layers_to_dup
                .iter()
                .map(|(_, _, attrs)| {
                    let src_name = attrs.get_str("name").unwrap_or("layer");
                    project.gen_name(src_name)
                })
                .collect();

            // Collect new UUIDs to select only duplicated layers
            let mut new_uuids: Vec<Uuid> = Vec::new();

            project.modify_comp(e.comp_uuid, |comp| {
                // Clear selection - will select only new layers
                comp.layer_selection.clear();

                for ((orig_uuid, source_uuid, mut attrs), new_name) in layers_to_dup.into_iter().zip(names) {
                    // Find insert position (above original)
                    let insert_idx = comp.uuid_to_idx(orig_uuid).unwrap_or(0);
                    // Update attrs with new name
                    attrs.set("name", crate::entities::AttrValue::Str(new_name.clone()));
                    // Create new Layer using from_attrs
                    let new_layer = crate::entities::comp_node::Layer::from_attrs(source_uuid, attrs);
                    let new_uuid = new_layer.uuid();
                    // Direct layers.insert() doesn't mark dirty
                    comp.layers.insert(insert_idx, new_layer);
                    new_uuids.push(new_uuid);
                    trace!("  Duplicated -> {} at idx {}", new_name, insert_idx);
                }
                // Select only the new duplicated layers
                comp.layer_selection = new_uuids;
                // Direct field changes (layers.insert, layer_selection) require explicit mark_dirty().
                // modify_comp() will then emit AttrsChangedEvent.
                comp.attrs.mark_dirty();
            });
        }
        return Some(result);
    }
    if let Some(e) = downcast_event::<CopyLayersEvent>(event) {
        trace!("CopyLayersEvent: comp={}", e.comp_uuid);
        // Copy selected layers to clipboard
        let clipboard_items = project.with_comp(e.comp_uuid, |comp| {
            if comp.layer_selection.is_empty() {
                trace!("Copy: no layers selected");
                return Vec::new();
            }
            let mut items: Vec<crate::widgets::timeline::ClipboardLayer> = Vec::new();
            for uuid in &comp.layer_selection {
                // Use get_layer() to access source_uuid field, not attrs
                if let Some(layer) = comp.get_layer(*uuid) {
                    let source_uuid = layer.source_uuid();
                    let original_start = layer.attrs.get_i32(A_IN).unwrap_or(0);
                    let name = layer.attrs.get_str("name").unwrap_or("?");
                    trace!("  Copy layer '{}' (source={}) at frame {}", name, source_uuid, original_start);
                    items.push(crate::widgets::timeline::ClipboardLayer {
                        source_uuid,
                        attrs: layer.attrs.clone(),
                        original_start,
                    });
                }
            }
            items.sort_by_key(|item| item.original_start);
            items
        });
        if let Some(items) = clipboard_items {
            trace!("Copied {} layers to clipboard", items.len());
            timeline_state.clipboard = items;
        } else {
            trace!("Copy: comp not found");
        }
        return Some(result);
    }
    if let Some(e) = downcast_event::<PasteLayersEvent>(event) {
        trace!("PasteLayersEvent: comp={}, frame={}", e.comp_uuid, e.target_frame);
        // Paste layers from clipboard at original frame positions (no offset)
        if timeline_state.clipboard.is_empty() {
            trace!("Paste: clipboard is empty");
        } else {
            // No offset - paste at original positions
            let offset = 0;
            trace!("Pasting {} layers at original positions", timeline_state.clipboard.len());

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
                    let old_in = attrs.get_i32(A_IN).unwrap_or(0);
                    let old_out = attrs.get_i32(A_OUT).unwrap_or(old_in + 100);
                    let new_in = old_in + offset;
                    let new_out = old_out + offset;
                    attrs.set(A_IN, crate::entities::AttrValue::Int(new_in));
                    attrs.set(A_OUT, crate::entities::AttrValue::Int(new_out));
                    // Create and insert new Layer at tracked position
                    let new_layer = crate::entities::comp_node::Layer::from_attrs(item.source_uuid, attrs);
                    let new_uuid = new_layer.uuid();
                    // Direct layers.insert() doesn't mark dirty
                    comp.layers.insert(insert_idx, new_layer);
                    insert_idx += 1;
                    comp.layer_selection.push(new_uuid);
                    trace!("  Pasted '{}' at frames {}..{}", new_name, new_in, new_out);
                }
                // Direct field changes (layers.insert, layer_selection) require explicit mark_dirty().
                // modify_comp() will then emit AttrsChangedEvent.
                comp.attrs.mark_dirty();
            });
            trace!("Paste complete: {} layers", timeline_state.clipboard.len());
        }
        return Some(result);
    }
    if let Some(e) = downcast_event::<SelectAllLayersEvent>(event) {
        trace!("SelectAllLayersEvent: comp={}", e.comp_uuid);
        project.modify_comp(e.comp_uuid, |comp| {
            let all_uuids: Vec<Uuid> = comp.layers.iter().map(|l| l.uuid()).collect();
            trace!("Selecting all {} layers", all_uuids.len());
            comp.layer_selection = all_uuids;
            comp.layer_selection_anchor = comp.layers.first().map(|l| l.uuid());
        });
        return Some(result);
    }
    if let Some(e) = downcast_event::<ClearLayerSelectionEvent>(event) {
        trace!("ClearLayerSelectionEvent: comp={}", e.comp_uuid);
        project.modify_comp(e.comp_uuid, |comp| {
            trace!("Clearing {} selected layers", comp.layer_selection.len());
            comp.layer_selection.clear();
            comp.layer_selection_anchor = None;
        });
        return Some(result);
    }

    // === Comp Play Area ===
    if let Some(e) = downcast_event::<SetCompPlayStartEvent>(event) {
        project.modify_comp(e.comp_uuid, |comp| {
            comp.set_comp_play_start(e.frame);
        });
        return Some(result);
    }
    if let Some(e) = downcast_event::<SetCompPlayEndEvent>(event) {
        project.modify_comp(e.comp_uuid, |comp| {
            comp.set_comp_play_end(e.frame);
        });
        return Some(result);
    }
    if let Some(e) = downcast_event::<ResetCompPlayAreaEvent>(event) {
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
