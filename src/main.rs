mod cli;
mod config;
mod dialogs;
mod entities;
mod events;
mod player;
mod ui;
mod utils;
mod widgets;
mod workers;

use clap::Parser;
use cli::Args;
use dialogs::encode::EncodeDialog;
use dialogs::prefs::{AppSettings, HotkeyHandler, render_settings_window};
use eframe::{egui, glow};
use egui_dock::{DockArea, DockState, NodeIndex, TabViewer};
use entities::Frame;
use entities::Project;
use log::{debug, error, info, warn};
use player::Player;
use std::path::PathBuf;
use std::sync::Arc;
use widgets::ae::AttributesState;
use widgets::status::StatusBar;
use widgets::viewport::{Shaders, ViewportRenderer, ViewportState};
use workers::Workers;

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
enum DockTab {
    Viewport,
    Timeline,
    Project,
    Attributes,
}

/// Main application state
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)]
struct PlayaApp {
    #[serde(skip)]
    frame: Option<Frame>,
    #[serde(skip)]
    displayed_frame: Option<i32>,
    #[serde(skip)]
    player: Player,
    #[serde(skip)]
    error_msg: Option<String>,
    #[serde(skip)]
    status_bar: StatusBar,
    #[serde(skip)]
    viewport_renderer: std::sync::Arc<std::sync::Mutex<ViewportRenderer>>,
    viewport_state: ViewportState,
    timeline_state: crate::widgets::timeline::TimelineState,
    #[serde(skip)]
    shader_manager: Shaders,
    /// Selected media item UUID in Project panel (persistent)
    selected_media_uuid: Option<String>,
    #[serde(skip)]
    last_render_time_ms: f32,
    settings: AppSettings,
    /// Persisted project (playlist); runtime player.project синхронизируется при save/load
    project: Project,
    #[serde(skip)]
    show_help: bool,
    #[serde(skip)]
    show_playlist: bool,
    #[serde(skip)]
    show_settings: bool,
    #[serde(skip)]
    show_encode_dialog: bool,
    #[serde(skip)]
    encode_dialog: Option<EncodeDialog>,
    #[serde(skip)]
    is_fullscreen: bool,
    #[serde(skip)]
    applied_mem_fraction: f64,
    #[serde(skip)]
    applied_workers: Option<usize>,
    #[serde(skip)]
    path_config: config::PathConfig,
    /// Global worker pool for background tasks (frame loading, encoding)
    #[serde(skip)]
    workers: Arc<Workers>,
    /// Event receiver for composition events (frame changes, layer updates)
    #[serde(skip)]
    comp_event_receiver: crossbeam::channel::Receiver<events::CompEvent>,
    /// Event sender for compositions (shared across all comps)
    #[serde(skip)]
    comp_event_sender: events::CompEventSender,
    /// Global event bus for application-wide events
    #[serde(skip)]
    event_bus: events::EventBus,
    #[serde(default = "PlayaApp::default_dock_state")]
    dock_state: DockState<DockTab>,
    /// Hotkey handler for context-aware keyboard shortcuts
    #[serde(skip)]
    hotkey_handler: HotkeyHandler,
    /// Currently focused window for input routing
    #[serde(skip)]
    focused_window: events::HotkeyWindow,
    /// Hover states for input routing
    #[serde(skip)]
    viewport_hovered: bool,
    #[serde(skip)]
    timeline_hovered: bool,
    #[serde(skip)]
    project_hovered: bool,
    attributes_state: AttributesState,
}

impl Default for PlayaApp {
    fn default() -> Self {
        let player = Player::new();
        let status_bar = StatusBar::new();

        // Create worker pool (75% of CPU cores for workers, 25% for UI thread)
        let num_workers = (num_cpus::get() * 3 / 4).max(1);
        let workers = Arc::new(Workers::new(num_workers));

        // Create event channel for composition events
        let (event_tx, event_rx) = crossbeam::channel::unbounded();
        let comp_event_sender = events::CompEventSender::new(event_tx);

        Self {
            frame: None,
            displayed_frame: None,
            player,
            error_msg: None,
            status_bar,
            viewport_renderer: std::sync::Arc::new(std::sync::Mutex::new(ViewportRenderer::new())),
            viewport_state: ViewportState::new(),
            timeline_state: crate::widgets::timeline::TimelineState::default(),
            shader_manager: Shaders::new(),
            selected_media_uuid: None,
            last_render_time_ms: 0.0,
            settings: AppSettings::default(),
            project: Project::new(),
            show_help: true,
            show_playlist: true,
            show_settings: false,
            show_encode_dialog: false,
            encode_dialog: None,
            is_fullscreen: false,
            applied_mem_fraction: 0.75,
            applied_workers: None,
            path_config: config::PathConfig::from_env_and_cli(None),
            workers,
            comp_event_receiver: event_rx,
            comp_event_sender,
            event_bus: events::EventBus::new(),
            dock_state: PlayaApp::default_dock_state(),
            hotkey_handler: {
                let mut handler = HotkeyHandler::new();
                handler.setup_default_bindings();
                handler
            },
            focused_window: events::HotkeyWindow::Global,
            viewport_hovered: false,
            timeline_hovered: false,
            project_hovered: false,
            attributes_state: AttributesState::default(),
        }
    }
}

impl PlayaApp {
    fn default_dock_state() -> DockState<DockTab> {
        let mut dock_state = DockState::new(vec![DockTab::Viewport]);

        let [viewport, _timeline] = dock_state.main_surface_mut().split_below(
            NodeIndex::root(),
            0.65,
            vec![DockTab::Timeline],
        );

        let [_viewport, project] =
            dock_state
                .main_surface_mut()
                .split_right(viewport, 0.75, vec![DockTab::Project]);

        // Add attributes panel below project (vertical split)
        let [_project, _attributes] =
            dock_state
                .main_surface_mut()
                .split_below(project, 0.6, vec![DockTab::Attributes]);

        dock_state
    }

    /// Attach composition event sender to all comps in the current project.
    fn attach_comp_event_sender(&mut self) {
        let sender = self.comp_event_sender.clone();
        for comp in self.player.project.media.values_mut() {
            comp.set_event_sender(sender.clone());
        }
    }

    /// Load sequences from file paths and append to player/project
    ///
    /// Detects sequences from provided paths, appends them to the player project,
    /// and clears any error messages on success.
    ///
    /// # Arguments
    /// * `paths` - Vector of file paths to detect sequences from
    ///
    /// # Returns
    /// * `Ok(())` - Sequences loaded successfully
    /// * `Err(String)` - Detection or loading failed with error message
    fn load_sequences(&mut self, paths: Vec<PathBuf>) -> Result<(), String> {
        match crate::entities::comp::Comp::detect_from_paths(paths) {
            Ok(comps) => {
                if comps.is_empty() {
                    let error_msg = "No valid sequences detected".to_string();
                    warn!("{}", error_msg);
                    self.error_msg = Some(error_msg.clone());
                    return Err(error_msg);
                }

                // Add all detected sequences to unified media pool (File mode comps)
                let comps_count = comps.len();
                let mut first_uuid: Option<String> = None;
                for comp in comps {
                    let uuid = comp.uuid.clone();
                    let name = comp.attrs.get_str("name").unwrap_or("Untitled").to_string();
                    info!("Adding clip (File mode): {} ({})", name, uuid);

                    self.player.project.media.insert(uuid.clone(), comp);
                    self.player.project.comps_order.push(uuid.clone());

                    // Remember first sequence for activation
                    if self.player.active_comp.is_none() && first_uuid.is_none() {
                        first_uuid = Some(uuid);
                    }
                }

                self.attach_comp_event_sender();

                // Activate first sequence and trigger frame loading
                if let Some(uuid) = first_uuid {
                    self.player.active_comp = Some(uuid);
                    self.enqueue_frame_loads_around_playhead(10);
                }

                self.error_msg = None;
                info!("Loaded {} clip(s)", comps_count);
                Ok(())
            }
            Err(e) => {
                let error_msg = format!("Failed to load sequences: {}", e);
                warn!("{}", error_msg);
                self.error_msg = Some(error_msg.clone());
                Err(error_msg)
            }
        }
    }

    /// Enqueue frame loading around playhead for active comp.
    ///
    /// Loads frames in radius around current_frame (e.g., -10..+10).
    /// Workers call frame.set_status(Loaded) which triggers actual load.
    fn enqueue_frame_loads_around_playhead(&self, radius: usize) {
        let current_frame = self.player.current_frame();

        // Get active comp
        let Some(comp_uuid) = &self.player.active_comp else {
            debug!("No active comp for frame loading");
            return;
        };
        let Some(comp) = self.player.project.media.get(comp_uuid) else {
            debug!("Active comp {} not found in media", comp_uuid);
            return;
        };

        debug!(
            "Enqueuing frame loads around frame {} (radius: {}), comp mode: {:?}, children: {}",
            current_frame,
            radius,
            comp.mode,
            comp.children.len()
        );

        // Handle File mode comp (directly loads frames from file sequence)
        if comp.mode == crate::entities::comp::CompMode::File {
            debug!("Active comp is File mode, loading frames directly");

            let play_start = comp.play_start();
            let play_end = comp.play_end();
            let comp_start = comp.start();

            // Calculate frame range to load
            let load_start_i32 = (current_frame - radius as i32).max(comp_start + play_start);
            let load_end_i32 = (current_frame + radius as i32).min(comp.end() - play_end);

            debug!(
                "Loading File mode frames [{}, {}]",
                load_start_i32, load_end_i32
            );

            for frame_idx in load_start_i32..=load_end_i32 {
                // Get frame from File mode comp
                let frame = match comp.get_frame(frame_idx, &self.player.project) {
                    Some(f) => f,
                    None => {
                        debug!("Failed to get frame {}", frame_idx);
                        continue;
                    }
                };

                if frame.file().is_none() {
                    debug!("Frame {} has no backing file, skipping load", frame_idx);
                    continue;
                }

                // Skip if already loaded
                let status = frame.status();
                if status == crate::entities::frame::FrameStatus::Loaded {
                    continue;
                }

                // Queue for loading
                debug!(
                    "Queueing frame {} for load (status: {:?})",
                    frame_idx, status
                );
                if let Err(e) = frame.set_status(crate::entities::frame::FrameStatus::Loading) {
                    debug!("Failed to mark frame {} as Loading: {}", frame_idx, e);
                }
            }

            return;
        }

        // Handle Layer mode comp (recursively loads frames from File mode children)
        debug!(
            "Active comp is Layer mode, processing {} children",
            comp.children.len()
        );

        // Enqueue loads for all children in comp
        for (child_idx, child_uuid) in comp.children.iter().enumerate() {
            let Some(attrs) = comp.children_attrs.get(child_uuid) else {
                debug!("Child {} missing attrs", child_idx);
                continue;
            };

            // Get source UUID from child attrs (child_uuid is now instance UUID)
            let Some(source_uuid) = attrs.get_str("uuid") else {
                debug!("Child {} missing uuid attribute", child_idx);
                continue;
            };

            // Resolve source from Project.media by UUID
            let Some(source) = self.player.project.media.get(source_uuid) else {
                debug!(
                    "Child {} references missing source {}",
                    child_idx, source_uuid
                );
                continue;
            };

            // Only process File mode comps for frame loading (Layer mode comps are composed on-demand)
            if source.mode != crate::entities::comp::CompMode::File {
                debug!(
                    "Child {} is Layer mode comp, skipping frame loading",
                    child_idx
                );
                continue;
            }

            debug!(
                "Processing child {}: comp {} has {} frames",
                child_idx,
                child_uuid,
                source.play_frame_count()
            );

            // Get child range from attrs (supports negative values)
            let child_start = attrs.get_i32("start").unwrap_or(0);
            let child_end = attrs.get_i32("end").unwrap_or(0);

            // Frames to load: [current - radius, current + radius] within child bounds
            let current_i32 = current_frame;
            let window_start = current_i32 - radius as i32;
            let window_end = current_i32 + radius as i32;

            // Find intersection between load window and child range
            let load_start_i32 = window_start.max(child_start);
            let load_end_i32 = window_end.min(child_end);

            // Skip child if no intersection (current_frame too far from child range)
            if load_start_i32 > load_end_i32 {
                debug!(
                    "Child {}: range [{}, {}] outside load window [{}, {}], skipping",
                    child_idx, child_start, child_end, window_start, window_end
                );
                continue;
            }

            debug!(
                "Child {}: range [{}, {}], will load frames [{}, {}]",
                child_idx, child_start, child_end, load_start_i32, load_end_i32
            );

            for global_i32 in load_start_i32..=load_end_i32 {
                // Check if frame is within child range
                if global_i32 < child_start || global_i32 > child_end {
                    continue;
                }

                // Convert global comp frame to local frame index
                let play_start = attrs.get_i32("play_start").unwrap_or(0);
                let frame_idx = (global_i32 - child_start) + play_start;
                if frame_idx < 0 {
                    debug!(
                        "Frame {} not active in child {} (negative play_start)",
                        global_i32, child_idx
                    );
                    continue;
                }

                if frame_idx >= source.play_frame_count() {
                    debug!(
                        "Frame {} (frame_idx {}) out of bounds (comp len: {})",
                        global_i32,
                        frame_idx,
                        source.play_frame_count()
                    );
                    continue;
                }

                // Get frame from File mode comp
                let frame = match source.get_frame(frame_idx, &self.player.project) {
                    Some(f) => f,
                    None => {
                        debug!(
                            "Failed to get frame {} (frame_idx {})",
                            global_i32, frame_idx
                        );
                        continue;
                    }
                };

                if frame.file().is_none() {
                    debug!(
                        "Frame {} (frame_idx {}) has no backing file, skipping load",
                        global_i32, frame_idx
                    );
                    continue;
                }

                // Skip if already loaded
                let status = frame.status();
                if status == crate::entities::frame::FrameStatus::Loaded {
                    continue;
                }

                debug!(
                    "Enqueuing load for frame {} (frame_idx {}) with status {:?}",
                    global_i32, frame_idx, status
                );

                // Enqueue load on worker thread
                let workers = Arc::clone(&self.workers);
                let frame_clone = frame.clone();

                workers.execute(move || {
                    use crate::entities::frame::FrameStatus;

                    debug!(
                        "Worker loading frame with status {:?}",
                        frame_clone.status()
                    );
                    // Transition to Loaded triggers actual load via Frame::load()
                    if let Err(e) = frame_clone.set_status(FrameStatus::Loaded) {
                        error!("Failed to load frame: {}", e);
                    } else {
                        debug!(
                            "Frame loaded successfully, new status: {:?}",
                            frame_clone.status()
                        );
                    }
                });
            }
        }
    }

    /// Handle composition events (frame changes, layer updates)
    fn handle_comp_events(&mut self) {
        // Process all pending events
        while let Ok(event) = self.comp_event_receiver.try_recv() {
            match event {
                events::CompEvent::CurrentFrameChanged {
                    comp_uuid,
                    old_frame,
                    new_frame,
                } => {
                    debug!(
                        "Comp {} frame changed: {} → {}",
                        comp_uuid, old_frame, new_frame
                    );

                    // Trigger frame loading around new position
                    self.enqueue_frame_loads_around_playhead(10);
                }
                events::CompEvent::LayersChanged { comp_uuid } => {
                    debug!("Comp {} layers changed", comp_uuid);
                    // Force viewport texture re-upload since composition changed
                    self.displayed_frame = None;
                    // Future: invalidate timeline cache, rebuild layer UI
                }
                events::CompEvent::TimelineChanged { comp_uuid } => {
                    debug!("Comp {} timeline changed", comp_uuid);
                    // Future: update timeline bounds, recalculate durations
                }
            }
        }
    }

    /// Enable or disable "cinema mode": borderless fullscreen, hidden UI, black background.
    fn set_cinema_mode(&mut self, ctx: &egui::Context, enabled: bool) {
        self.is_fullscreen = enabled;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(enabled));
        // Hide window decorations in cinema mode for a cleaner look
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(!enabled));
        // Request repaint to immediately reflect UI visibility/background changes
        ctx.request_repaint();
    }
    /// Save project to JSON file
    fn save_project(&mut self, path: PathBuf) {
        if let Err(e) = self.player.project.to_json(&path) {
            error!("{}", e);
            self.error_msg = Some(e);
        } else {
            info!("Saved project to {}", path.display());
        }
    }

    /// Load project from JSON file
    fn load_project(&mut self, path: PathBuf) {
        match crate::entities::Project::from_json(&path) {
            Ok(mut project) => {
                info!("Loaded project from {}", path.display());

                // Rebuild runtime with event sender for all comps
                project.rebuild_runtime(Some(self.comp_event_sender.clone()));
                self.player.project = project;
                self.error_msg = None;
            }
            Err(e) => {
                error!("{}", e);
                self.error_msg = Some(e);
            }
        }
    }

    /// Handle application events from the event bus
    fn handle_event(&mut self, event: events::AppEvent) {
        use events::AppEvent;

        match event {
            // ===== Playback Control =====
            AppEvent::Play => {
                self.player.is_playing = true;
            }
            AppEvent::Pause => {
                self.player.is_playing = false;
            }
            AppEvent::TogglePlayPause => {
                self.player.is_playing = !self.player.is_playing;
            }
            AppEvent::Stop => {
                self.player.stop();
            }
            AppEvent::SetFrame(frame) => {
                if let Some(comp_uuid) = &self.player.active_comp {
                    if let Some(comp) = self.player.project.get_comp_mut(comp_uuid) {
                        comp.set_current_frame(frame);
                    }
                }
            }
            AppEvent::StepForward => {
                // TODO: implement step forward
            }
            AppEvent::StepBackward => {
                // TODO: implement step backward
            }
            AppEvent::StepForwardLarge => {
                // TODO: implement step forward large (25 frames)
            }
            AppEvent::StepBackwardLarge => {
                // TODO: implement step backward large (25 frames)
            }
            AppEvent::PreviousClip => {
                // TODO: implement previous clip
            }
            AppEvent::NextClip => {
                // TODO: implement next clip
            }
            AppEvent::JumpToStart => {
                if let Some(comp_uuid) = &self.player.active_comp {
                    if let Some(comp) = self.player.project.get_comp_mut(comp_uuid) {
                        let play_start = comp.play_start();
                        comp.set_current_frame(play_start);
                    }
                }
            }
            AppEvent::JumpToEnd => {
                if let Some(comp_uuid) = &self.player.active_comp {
                    if let Some(comp) = self.player.project.get_comp_mut(comp_uuid) {
                        let play_end = comp.play_end();
                        comp.set_current_frame(play_end);
                    }
                }
            }
            AppEvent::JumpToPrevEdge => {
                if let Some(comp_uuid) = &self.player.active_comp {
                    if let Some(comp) = self.player.project.get_comp_mut(comp_uuid) {
                        if let Some(&(frame, _)) = comp
                            .get_child_edges_near(comp.current_frame)
                            .iter()
                            .find(|(f, _)| *f < comp.current_frame)
                        {
                            comp.set_current_frame(frame);
                        }
                    }
                }
            }
            AppEvent::JumpToNextEdge => {
                if let Some(comp_uuid) = &self.player.active_comp {
                    if let Some(comp) = self.player.project.get_comp_mut(comp_uuid) {
                        if let Some(&(frame, _)) = comp
                            .get_child_edges_near(comp.current_frame)
                            .iter()
                            .find(|(f, _)| *f > comp.current_frame)
                        {
                            comp.set_current_frame(frame);
                        }
                    }
                }
            }

            // ===== Project Management =====
            AppEvent::AddClip(path) => {
                let _ = self.load_sequences(vec![path]);
            }
            AppEvent::AddClips(paths) => {
                let _ = self.load_sequences(paths);
            }
            AppEvent::AddComp { name, fps } => {
                let start = 0;
                let end = 100;
                let mut comp = crate::entities::comp::Comp::new(&name, start, end, fps);
                comp.set_name(name);
                comp.set_start(start);
                comp.set_end(end);
                comp.set_fps(fps);
                self.player.project.add_comp(comp);
            }
            AppEvent::RemoveMedia(_uuid) => {
                // TODO: implement remove media
            }
            AppEvent::SaveProject(path) => {
                self.save_project(path);
            }
            AppEvent::LoadProject(path) => {
                self.load_project(path);
            }

            // ===== Selection =====
            AppEvent::SelectMedia(uuid) => {
                // Select and activate media item (comp/clip)
                self.select_item(uuid);
            }
            AppEvent::SelectLayer(_index) => {
                if let Some(comp_uuid) = &self.player.active_comp {
                    if let Some(comp) = self.player.project.media.get_mut(comp_uuid) {
                        comp.set_selected_layer(Some(_index));
                    }
                }
            }
            AppEvent::DeselectAll => {
                if let Some(comp_uuid) = &self.player.active_comp {
                    if let Some(comp) = self.player.project.media.get_mut(comp_uuid) {
                        comp.set_selected_layer(None);
                    }
                }
            }
            AppEvent::DeselectLayer => {
                if let Some(comp_uuid) = &self.player.active_comp {
                    if let Some(comp) = self.player.project.media.get_mut(comp_uuid) {
                        comp.set_selected_layer(None);
                    }
                }
            }

            // ===== UI State =====
            AppEvent::TogglePlaylist => {
                self.show_playlist = !self.show_playlist;
            }
            AppEvent::ToggleHelp => {
                self.show_help = !self.show_help;
            }
            AppEvent::ToggleAttributeEditor => {
                // TODO: implement when attribute editor exists
            }
            AppEvent::ToggleSettings => {
                self.show_settings = !self.show_settings;
            }
            AppEvent::ToggleFullscreen => {
                // TODO: implement fullscreen toggle
            }
            AppEvent::ToggleLoop => {
                self.settings.loop_enabled = !self.settings.loop_enabled;
            }
            AppEvent::ToggleFrameNumbers => {
                self.settings.show_frame_numbers = !self.settings.show_frame_numbers;
            }
            AppEvent::TimelineZoomChanged(zoom) => {
                self.timeline_state.zoom = zoom.clamp(0.1, 20.0);
            }
            AppEvent::TimelinePanChanged(pan) => {
                self.timeline_state.pan_offset = pan;
            }
            AppEvent::TimelineSnapChanged(enabled) => {
                self.timeline_state.snap_enabled = enabled;
            }
            AppEvent::TimelineLockWorkAreaChanged(locked) => {
                self.timeline_state.lock_work_area = locked;
            }
            AppEvent::TimelineFitAll(canvas_width) => {
                // Fit comp play_range to timeline view
                if let Some(comp_uuid) = &self.player.active_comp {
                    if let Some(comp) = self.player.project.media.get(comp_uuid) {
                        let (min_frame, max_frame) = comp.play_range(true);
                        let duration = (max_frame - min_frame + 1).max(1); // +1 for inclusive range

                        // pixels_per_frame = canvas_width / duration
                        // zoom = pixels_per_frame / default_pixels_per_frame (2.0)
                        let pixels_per_frame = canvas_width / duration as f32;
                        let default_pixels_per_frame = 2.0;
                        let zoom = (pixels_per_frame / default_pixels_per_frame).clamp(0.1, 20.0); // Allow higher zoom

                        self.timeline_state.zoom = zoom;
                        self.timeline_state.pan_offset = min_frame as f32;
                    }
                }
            }
            AppEvent::TimelineFit => {
                // Fit timeline using last known canvas width
                let canvas_width = self.timeline_state.last_canvas_width;
                self.handle_event(AppEvent::TimelineFitAll(canvas_width));
            }
            AppEvent::TimelineResetZoom => {
                // Reset timeline zoom to 1.0 (default)
                self.timeline_state.zoom = 1.0;
            }
            AppEvent::ZoomViewport(factor) => {
                self.viewport_state.zoom *= factor;
            }
            AppEvent::ResetViewport => {
                self.viewport_state.reset();
            }
            AppEvent::FitViewport => {
                self.viewport_state.set_mode_fit();
            }
            AppEvent::Viewport100 => {
                self.viewport_state.set_mode_100();
            }

            // ===== Play Range Control =====
            AppEvent::SetPlayRangeStart => {
                if let Some(comp_uuid) = &self.player.active_comp {
                    if let Some(comp) = self.player.project.get_comp_mut(comp_uuid) {
                        let current = comp.current_frame as i32;
                        comp.set_play_start(current);
                    }
                }
            }
            AppEvent::SetPlayRangeEnd => {
                if let Some(comp_uuid) = &self.player.active_comp {
                    if let Some(comp) = self.player.project.get_comp_mut(comp_uuid) {
                        let current = comp.current_frame as i32;
                        comp.set_play_end(current);
                    }
                }
            }
            AppEvent::ResetPlayRange => {
                if let Some(comp_uuid) = &self.player.active_comp {
                    if let Some(comp) = self.player.project.get_comp_mut(comp_uuid) {
                        comp.set_play_start(0);
                        comp.set_play_end(0); // 0 means use full range
                    }
                }
            }
            AppEvent::SetCompPlayStart { comp_uuid, frame } => {
                if let Some(comp) = self.player.project.media.get_mut(&comp_uuid) {
                    let play_start = (frame - comp.start()).max(0);
                    comp.set_comp_play_start(play_start);
                }
            }
            AppEvent::SetCompPlayEnd { comp_uuid, frame } => {
                if let Some(comp) = self.player.project.media.get_mut(&comp_uuid) {
                    let play_end = (comp.end() - frame).max(0);
                    comp.set_comp_play_end(play_end);
                }
            }
            AppEvent::ResetCompPlayArea { comp_uuid } => {
                if let Some(comp) = self.player.project.media.get_mut(&comp_uuid) {
                    comp.set_comp_play_start(0);
                    comp.set_comp_play_end(0);
                }
            }

            // ===== FPS Control =====
            AppEvent::IncreaseFPS => {
                self.settings.fps_base = (self.settings.fps_base + 1.0).min(120.0);
                if let Some(comp_uuid) = &self.player.active_comp {
                    if let Some(comp) = self.player.project.media.get_mut(comp_uuid) {
                        let new_fps = (comp.fps() + 1.0).min(120.0);
                        comp.set_fps(new_fps);
                    }
                }
            }
            AppEvent::DecreaseFPS => {
                self.settings.fps_base = (self.settings.fps_base - 1.0).max(1.0);
                if let Some(comp_uuid) = &self.player.active_comp {
                    if let Some(comp) = self.player.project.media.get_mut(comp_uuid) {
                        let new_fps = (comp.fps() - 1.0).max(1.0);
                        comp.set_fps(new_fps);
                    }
                }
            }

            // ===== Layer Operations =====
            AppEvent::AddLayer {
                comp_uuid,
                source_uuid,
                start_frame,
                target_row,
            } => {
                let mut comp_opt = {
                    let media = &mut self.player.project.media;
                    media.remove(&comp_uuid)
                };

                if let Some(mut comp) = comp_opt.take() {
                    let add_result = if let Some(target_row) = target_row {
                        let (duration, source_dim) = self
                            .player
                            .project
                            .media
                            .get(&source_uuid)
                            .map(|s| (s.frame_count(), s.dim()))
                            .unwrap_or((1, (64, 64)));
                        comp.add_child_with_duration(
                            source_uuid.clone(),
                            start_frame,
                            duration,
                            Some(target_row),
                            source_dim,
                        )
                    } else {
                        comp.add_child(source_uuid.clone(), start_frame, &self.player.project)
                    };

                    if let Err(e) = add_result {
                        log::error!("Failed to add layer: {}", e);
                    } else if let Some(child_comp) = self.player.project.media.get_mut(&source_uuid)
                    {
                        if child_comp.get_parent() != Some(&comp_uuid) {
                            child_comp.set_parent(Some(comp_uuid.clone()));
                        }
                    }

                    // Put comp back
                    self.player.project.media.insert(comp_uuid.clone(), comp);
                }
            }
            AppEvent::RemoveLayer {
                comp_uuid,
                layer_idx,
            } => {
                let mut comp_opt = {
                    let media = &mut self.player.project.media;
                    media.remove(&comp_uuid)
                };

                if let Some(mut comp) = comp_opt.take() {
                    // Get child UUID by index
                    if let Some(child_uuid) = comp.get_children().get(layer_idx).cloned() {
                        if let Some(attrs) = comp.children_attrs.get(&child_uuid) {
                            if let Some(source_uuid) = attrs.get_str("uuid") {
                                if let Some(child_comp) =
                                    self.player.project.media.get_mut(source_uuid)
                                {
                                    if child_comp.get_parent() == Some(&comp_uuid) {
                                        child_comp.set_parent(None);
                                    }
                                }
                            }
                        }

                        if comp.has_child(&child_uuid) {
                            comp.remove_child(&child_uuid);
                        } else {
                            log::error!("Child {} not found in comp {}", child_uuid, comp_uuid);
                        }
                    } else {
                        log::error!("Layer index {} out of bounds", layer_idx);
                    }

                    // Put comp back
                    self.player.project.media.insert(comp_uuid.clone(), comp);
                }
            }
            AppEvent::MoveLayer {
                comp_uuid,
                layer_idx,
                new_start,
            } => {
                if let Some(comp) = self.player.project.media.get_mut(&comp_uuid) {
                    if let Err(e) = comp.move_child(layer_idx, new_start as i32) {
                        log::error!("Failed to move layer: {}", e);
                    }
                }
            }
            AppEvent::ReorderLayer {
                comp_uuid,
                from_idx,
                to_idx,
            } => {
                if let Some(comp) = self.player.project.media.get_mut(&comp_uuid) {
                    let children = comp.get_children();
                    if from_idx != to_idx && from_idx < children.len() && to_idx < children.len() {
                        let mut reordered = comp.children.clone();
                        let child_uuid = reordered.remove(from_idx);
                        reordered.insert(to_idx, child_uuid);
                        comp.children = reordered;
                        comp.clear_cache();
                    }
                }
            }
            AppEvent::MoveAndReorderLayer {
                comp_uuid,
                layer_idx,
                new_start,
                new_idx,
            } => {
                if let Some(comp) = self.player.project.media.get_mut(&comp_uuid) {
                    let children_len = comp.get_children().len();
                    if layer_idx != new_idx && layer_idx < children_len && new_idx < children_len {
                        let mut reordered = comp.children.clone();
                        let child_uuid = reordered.remove(layer_idx);
                        reordered.insert(new_idx, child_uuid);
                        comp.children = reordered;
                    }

                    let final_idx = new_idx.min(comp.get_children().len().saturating_sub(1));
                    let new_start_i32 = new_start as i32;
                    let _ = comp.move_child(final_idx, new_start_i32);
                }
            }
            AppEvent::SetLayerPlayStart {
                comp_uuid,
                layer_idx,
                new_play_start,
            } => {
                if let Some(comp) = self.player.project.media.get_mut(&comp_uuid) {
                    let _ = comp.set_child_play_start(layer_idx, new_play_start);
                }
            }
            AppEvent::SetLayerPlayEnd {
                comp_uuid,
                layer_idx,
                new_play_end,
            } => {
                if let Some(comp) = self.player.project.media.get_mut(&comp_uuid) {
                    let _ = comp.set_child_play_end(layer_idx, new_play_end);
                }
            }
            AppEvent::RemoveSelectedLayer => {
                // TODO: implement remove selected layer (need selection tracking)
            }

            // ===== Drag-and-Drop (placeholders for now) =====
            AppEvent::DragStart { .. } => {
                // TODO: implement drag start
            }
            AppEvent::DragMove { .. } => {
                // TODO: implement drag move
            }
            AppEvent::DragDrop { .. } => {
                // TODO: implement drag drop
            }
            AppEvent::DragCancel => {
                // TODO: implement drag cancel
            }
        }
    }

    /// Determine which window has focus based on hover state and context
    fn determine_focused_window(&self, ctx: &egui::Context) -> events::HotkeyWindow {
        use events::HotkeyWindow;

        // Priority 1: Modal dialogs (settings, encode) - always capture input
        if self.show_settings || self.show_encode_dialog {
            return HotkeyWindow::Global;
        }

        // Priority 2: Keyboard focus (text fields) - don't process hotkeys
        if ctx.wants_keyboard_input() {
            return HotkeyWindow::Global; // Return Global but will be filtered later
        }

        // Priority 3: Hover detection - which widget is under the cursor
        if self.timeline_hovered {
            return HotkeyWindow::Timeline;
        }
        if self.viewport_hovered {
            return HotkeyWindow::Viewport;
        }
        if self.project_hovered {
            return HotkeyWindow::Project;
        }

        // Priority 4: Fallback to Global
        HotkeyWindow::Global
    }

    fn handle_keyboard_input(&mut self, ctx: &egui::Context) {
        let input = ctx.input(|i| i.clone());

        // Determine focused window and update hotkey handler
        let focused_window = self.determine_focused_window(ctx);
        self.focused_window = focused_window.clone();
        self.hotkey_handler
            .set_focused_window(focused_window.clone());

        // Try hotkey handler first (for context-aware hotkeys)
        if let Some(event) = self.hotkey_handler.handle_input(&input) {
            log::debug!(
                "Hotkey event: {:?}, focused_window: {:?}",
                event,
                focused_window
            );
            self.event_bus.send(event);
            return; // Hotkey handled, don't process manual checks
        }

        // Debug: log when F or A is pressed but no event
        if input.key_pressed(egui::Key::F) || input.key_pressed(egui::Key::A) {
            log::debug!(
                "F/A pressed but no event. focused_window: {:?}, viewport_hovered: {}, timeline_hovered: {}",
                focused_window,
                self.viewport_hovered,
                self.timeline_hovered
            );
        }

        // F1: Toggle help
        if input.key_pressed(egui::Key::F1) {
            self.event_bus.send(events::AppEvent::ToggleHelp);
        }

        // F2: Toggle playlist
        if input.key_pressed(egui::Key::F2) {
            self.event_bus.send(events::AppEvent::TogglePlaylist);
        }

        // F3: Toggle settings (not yet in EventBus, keep direct)
        if input.key_pressed(egui::Key::F3) {
            self.show_settings = !self.show_settings;
        }

        // F4: Toggle encode dialog (not yet in EventBus, keep direct)
        if input.key_pressed(egui::Key::F4) {
            self.show_encode_dialog = !self.show_encode_dialog;
            // Load dialog state from settings when opening
            if self.show_encode_dialog && self.encode_dialog.is_none() {
                debug!("[F4] Opening encode dialog, loading settings from AppSettings");
                self.encode_dialog = Some(EncodeDialog::load_from_settings(
                    &self.settings.encode_dialog,
                ));
            }
        }

        // ESC/Q: Priority-based handler. ESC: fullscreen -> encode dialog -> settings -> quit. Q: always quit.
        if input.key_pressed(egui::Key::Escape) || input.key_pressed(egui::Key::Q) {
            // Priority 1: Fullscreen/Cinema mode (highest priority - most immersive state)
            if input.key_pressed(egui::Key::Escape) && self.is_fullscreen {
                self.set_cinema_mode(ctx, false);
            }
            // Priority 2: Encode dialog (modal dialog should be dismissed before app closes)
            else if input.key_pressed(egui::Key::Escape) && self.show_encode_dialog {
                // Close encode dialog (stop encoding if in progress)
                if let Some(ref mut dialog) = self.encode_dialog
                    && dialog.is_encoding()
                {
                    dialog.stop_encoding();
                }
                self.show_encode_dialog = false;
            }
            // Priority 3: Settings dialog (preferences window)
            else if input.key_pressed(egui::Key::Escape) && self.show_settings {
                self.show_settings = false;
            }
            // Priority 4: Quit application (default action when nothing else to dismiss)
            else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }

        // Play/Pause (Space, ArrowUp)
        if input.key_pressed(egui::Key::Space) || input.key_pressed(egui::Key::ArrowUp) {
            // Toggle between play and pause
            if self.player.is_playing {
                self.event_bus.send(events::AppEvent::Pause);
            } else {
                self.event_bus.send(events::AppEvent::Play);
            }
        }

        // Stop (K, .)
        if input.key_pressed(egui::Key::K) || input.key_pressed(egui::Key::Period) {
            self.event_bus.send(events::AppEvent::Stop);
        }

        // Only process playback hotkeys when no widget has keyboard focus
        // (prevents arrow keys from triggering playback while editing text fields)
        if !ctx.wants_keyboard_input() {
            // Jump to start (1, Home)
            if input.key_pressed(egui::Key::Num1) || input.key_pressed(egui::Key::Home) {
                self.event_bus.send(events::AppEvent::JumpToStart);
            }

            // Jump to end (2, End)
            if input.key_pressed(egui::Key::Num2) || input.key_pressed(egui::Key::End) {
                self.event_bus.send(events::AppEvent::JumpToEnd);
            }

            // Frame stepping
            // PageDown - step 1 frame forward (or 25 with Shift)
            if input.key_pressed(egui::Key::PageDown) {
                let step = if input.modifiers.shift {
                    crate::player::FRAME_JUMP_STEP
                } else {
                    1
                };
                self.player.step(step);
            }

            // PageUp - step 1 frame backward (or 25 with Shift)
            if input.key_pressed(egui::Key::PageUp) {
                let step = if input.modifiers.shift {
                    -crate::player::FRAME_JUMP_STEP
                } else {
                    -1
                };
                self.player.step(step);
            }

            // Ctrl+PageDown - jump to end
            if input.modifiers.ctrl && input.key_pressed(egui::Key::PageDown) {
                self.player.to_end();
            }

            // Ctrl+PageUp - jump to start
            if input.modifiers.ctrl && input.key_pressed(egui::Key::PageUp) {
                self.player.to_start();
            }

            // Base FPS controls
            // Decrease base FPS (-)
            if input.key_pressed(egui::Key::Minus) {
                self.player.decrease_fps_base();
            }

            // Increase base FPS (=, +)
            if input.key_pressed(egui::Key::Equals) || input.key_pressed(egui::Key::Plus) {
                self.player.increase_fps_base();
            }

            // Arrow navigation
            // Shift+ArrowLeft - step 25 frames backward
            if input.modifiers.shift && input.key_pressed(egui::Key::ArrowLeft) {
                self.player.step(-crate::player::FRAME_JUMP_STEP);
            }
            // Shift+ArrowRight - step 25 frames forward
            else if input.modifiers.shift && input.key_pressed(egui::Key::ArrowRight) {
                self.player.step(crate::player::FRAME_JUMP_STEP);
            }
            // ArrowLeft - step 1 frame backward (no modifiers)
            else if !input.modifiers.any() && input.key_pressed(egui::Key::ArrowLeft) {
                self.player.step(-1);
            }
            // ArrowRight - step 1 frame forward (no modifiers)
            else if !input.modifiers.any() && input.key_pressed(egui::Key::ArrowRight) {
                self.player.step(1);
            }

            // ArrowDown - stop playback
            if input.key_pressed(egui::Key::ArrowDown) {
                self.player.stop();
            }

            // J, , - jog backward
            if input.key_pressed(egui::Key::J) || input.key_pressed(egui::Key::Comma) {
                self.player.jog_backward();
            }

            // L, / - jog forward
            if input.key_pressed(egui::Key::L) || input.key_pressed(egui::Key::Slash) {
                self.player.jog_forward();
            }

            // Sequence navigation
            // Jump to previous sequence start ([)
            if input.key_pressed(egui::Key::OpenBracket) {
                self.player.jump_prev_sequence();
            }

            // Jump to next sequence start (])
            if input.key_pressed(egui::Key::CloseBracket) {
                self.player.jump_next_sequence();
            }

            // Toggle Loop with ' and `
            if input.key_pressed(egui::Key::Quote) || input.key_pressed(egui::Key::Backtick) {
                self.player.loop_enabled = !self.player.loop_enabled;
            }

            // Toggle frame numbers on timeslider (Backspace)
            if input.key_pressed(egui::Key::Backspace) {
                self.settings.show_frame_numbers = !self.settings.show_frame_numbers;
            }

            // Set comp work area start (B = Begin) relative to comp.start
            if !input.modifiers.ctrl && input.key_pressed(egui::Key::B) {
                let current = self.player.current_frame();
                if let Some(comp_uuid) = &self.player.active_comp.clone() {
                    if let Some(comp) = self.player.project.media.get_mut(comp_uuid) {
                        let play_start = (current as i32 - comp.start() as i32).max(0);
                        comp.set_comp_play_start(play_start);
                    }
                }
            }

            // Set comp work area end (N = eNd) relative to comp.end
            if !input.modifiers.ctrl && input.key_pressed(egui::Key::N) {
                let current = self.player.current_frame();
                if let Some(comp_uuid) = &self.player.active_comp.clone() {
                    if let Some(comp) = self.player.project.media.get_mut(comp_uuid) {
                        let play_end = (comp.end() as i32 - current as i32).max(0);
                        comp.set_comp_play_end(play_end);
                    }
                }
            }

            // Reset comp work area to full (Ctrl+B)
            if input.modifiers.ctrl && input.key_pressed(egui::Key::B) {
                if let Some(comp_uuid) = &self.player.active_comp.clone() {
                    if let Some(comp) = self.player.project.media.get_mut(comp_uuid) {
                        comp.set_comp_play_start(0);
                        comp.set_comp_play_end(0);
                    }
                }
            }

            // Skip to start/end (Ctrl modifiers)
            if input.modifiers.ctrl && input.key_pressed(egui::Key::ArrowLeft) {
                self.player.to_start();
            }
            if input.modifiers.ctrl && input.key_pressed(egui::Key::ArrowRight) {
                self.player.to_end();
            }

            // Ctrl+R: reset settings and force exit cinema/fullscreen
            if input.modifiers.ctrl && input.key_pressed(egui::Key::R) {
                self.reset_settings(ctx);
                if self.is_fullscreen {
                    self.set_cinema_mode(ctx, false);
                }
            }

            // Z: toggle cinema/fullscreen
            if input.key_pressed(egui::Key::Z) {
                let enable = !self.is_fullscreen;
                self.set_cinema_mode(ctx, enable);
            }

            // Viewport controls F/A/H moved to hotkey system (context-aware)
        } // End of !ctx.wants_keyboard_input()
    }

    fn reset_settings(&mut self, ctx: &egui::Context) {
        info!("Resetting settings to default");
        self.settings = AppSettings::default();
        self.player.reset_settings();
        self.viewport_state = ViewportState::new();
        self.shader_manager.reset_settings();

        // Reset window size
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(1280.0, 720.0)));

        // Re-apply image-dependent viewport settings if an image is loaded
        if let Some(frame) = &self.frame {
            let (width, height) = frame.resolution();
            self.viewport_state
                .set_image_size(egui::vec2(width as f32, height as f32));
            self.viewport_state.set_mode_fit();
        }
    }

    /// Select and activate media item (comp/clip) by UUID
    fn select_item(&mut self, uuid: String) {
        self.selected_media_uuid = Some(uuid.clone());
        self.player.set_active_comp(uuid.clone());
        // Trigger frame loading around new current_frame
        self.enqueue_frame_loads_around_playhead(10);
    }

    fn render_project_tab(&mut self, ui: &mut egui::Ui) {
        if !self.show_playlist {
            ui.centered_and_justified(|ui| {
                ui.label("Project panel hidden (enable playlist to show)");
            });
            return;
        }

        let project_actions =
            widgets::project::render(ui, &mut self.player, self.selected_media_uuid.as_ref());

        // Store hover state for input routing
        self.project_hovered = project_actions.hovered;

        // Handle selection from click via EventBus
        if let Some(uuid) = project_actions.selected_uuid {
            self.event_bus.send(events::AppEvent::SelectMedia(uuid));
        }

        // Load media files
        if let Some(path) = project_actions.load_sequence {
            let _ = self.load_sequences(vec![path]);
        }

        // Save/Load project
        if let Some(path) = project_actions.save_project {
            self.save_project(path);
        }
        if let Some(path) = project_actions.load_project {
            self.load_project(path);
        }

        // Create new composition
        if project_actions.new_comp {
            use crate::entities::Comp;
            let fps = 30.0;
            let end = (fps * 5.0) as i32; // 5 seconds
            let mut comp = Comp::new("New Comp", 0, end, fps);
            let uuid = comp.uuid.clone();

            // Set event sender for the new comp
            comp.set_event_sender(self.comp_event_sender.clone());

            self.player.project.media.insert(uuid.clone(), comp);
            self.player.project.comps_order.push(uuid.clone());

            // Activate the new comp
            self.player.set_active_comp(uuid.clone());

            info!("Created new comp: {}", uuid);
        }

        // Remove composition
        if let Some(comp_uuid) = project_actions.remove_comp {
            self.player.project.media.remove(&comp_uuid);
            self.player
                .project
                .comps_order
                .retain(|uuid| uuid != &comp_uuid);

            // If removed comp was active, switch to first available or None
            if self.player.active_comp.as_ref() == Some(&comp_uuid) {
                let first_comp = self.player.project.comps_order.first().cloned();
                if let Some(new_active) = first_comp {
                    self.player.set_active_comp(new_active);
                } else {
                    self.player.active_comp = None;
                }
            }

            info!("Removed comp {}", comp_uuid);
        }

        // Clear all compositions
        if project_actions.clear_all_comps {
            // Remove all media (clips and comps are unified now)
            self.player.project.media.clear();
            self.player.project.comps_order.clear();
            self.player.active_comp = None;
            info!("All media cleared");
        }
    }

    fn render_timeline_tab(&mut self, ui: &mut egui::Ui) {
        // Sync timeline toggles from settings
        self.timeline_state.snap_enabled = self.settings.timeline_snap_enabled;
        self.timeline_state.lock_work_area = self.settings.timeline_lock_work_area;

        // Render timeline panel with transport controls
        let (shader_changed, timeline_actions) = ui::render_timeline_panel(
            ui,
            &mut self.player,
            &mut self.shader_manager,
            &mut self.timeline_state,
            &self.event_bus,
        );

        // Store hover state for input routing
        self.timeline_hovered = timeline_actions.hovered;

        if shader_changed {
            let mut renderer = self.viewport_renderer.lock().unwrap();
            renderer.update_shader(&self.shader_manager);
            log::info!("Shader changed to: {}", self.shader_manager.current_shader);
        }
    }

    fn render_viewport_tab(&mut self, ui: &mut egui::Ui) {
        // Determine if the texture needs to be re-uploaded by checking if the frame has changed
        let texture_needs_upload = self.displayed_frame != Some(self.player.current_frame());

        // If the frame has changed, update our cached frame
        if texture_needs_upload {
            self.frame = self.player.get_current_frame();
            self.displayed_frame = Some(self.player.current_frame());
        }

        let (viewport_actions, render_time) = widgets::viewport::render(
            ui,
            self.frame.as_ref(),
            self.error_msg.as_ref(),
            &mut self.player,
            &mut self.viewport_state,
            &self.viewport_renderer,
            &mut self.shader_manager,
            self.show_help,
            self.is_fullscreen,
            texture_needs_upload,
        );
        self.last_render_time_ms = render_time;

        // Store hover state for input routing
        self.viewport_hovered = viewport_actions.hovered;

        if let Some(path) = viewport_actions.load_sequence {
            let _ = self.load_sequences(vec![path]);
        }

        // Persist timeline options back to settings
        self.settings.timeline_snap_enabled = self.timeline_state.snap_enabled;
        self.settings.timeline_lock_work_area = self.timeline_state.lock_work_area;
    }

    fn render_attributes_tab(&mut self, ui: &mut egui::Ui) {
        if let Some(active) = self.player.active_comp.clone() {
            if let Some(comp) = self.player.project.media.get_mut(&active) {
                // Show attributes of selected layer if any, otherwise show comp attributes
                if let Some(layer_idx) = comp.selected_layer {
                    // Get layer instance UUID
                    if let Some(instance_uuid) = comp.children.get(layer_idx) {
                        if let Some(attrs) = comp.children_attrs.get_mut(instance_uuid) {
                            let display_name = attrs
                                .get_str("name")
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| format!("Layer {}", layer_idx));
                            crate::widgets::ae::render(
                                ui,
                                attrs,
                                &mut self.attributes_state,
                                &display_name,
                            );
                        } else {
                            ui.label("(layer has no attributes)");
                        }
                    } else {
                        ui.label("(invalid layer index)");
                    }
                } else {
                    // No layer selected - show comp attributes
                    let comp_name = comp.name().to_string();
                    crate::widgets::ae::render(
                        ui,
                        &mut comp.attrs,
                        &mut self.attributes_state,
                        &comp_name,
                    );
                }
            } else {
                ui.label("No active comp");
            }
        } else {
            ui.label("No active comp");
        }
    }
}

struct DockTabs<'a> {
    app: &'a mut PlayaApp,
}

impl<'a> TabViewer for DockTabs<'a> {
    type Tab = DockTab;

    fn title(&mut self, tab: &mut DockTab) -> egui::WidgetText {
        match tab {
            DockTab::Viewport => "Viewport".into(),
            DockTab::Timeline => "Timeline".into(),
            DockTab::Project => "Project".into(),
            DockTab::Attributes => "Attributes".into(),
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut DockTab) {
        match tab {
            DockTab::Viewport => self.app.render_viewport_tab(ui),
            DockTab::Timeline => self.app.render_timeline_tab(ui),
            DockTab::Project => self.app.render_project_tab(ui),
            DockTab::Attributes => self.app.render_attributes_tab(ui),
        }
    }
}

impl eframe::App for PlayaApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Process all events from the event bus
        while let Some(event) = self.event_bus.try_recv() {
            self.handle_event(event);
        }

        // Apply theme based on settings
        if self.settings.dark_mode {
            ctx.set_visuals(egui::Visuals::dark());
        } else {
            ctx.set_visuals(egui::Visuals::light());
        }

        // Apply font size from settings
        let mut style = (*ctx.style()).clone();
        for (_, font_id) in style.text_styles.iter_mut() {
            font_id.size = self.settings.font_size;
        }
        ctx.set_style(style);

        // Enable multipass for better taffy layout recalculation responsiveness
        ctx.options_mut(|opts| {
            opts.max_passes = std::num::NonZeroUsize::new(2).unwrap();
        });

        // cache_mem_percent is deprecated (old frame cache); kept only for config compatibility
        self.player.update();

        // Handle composition events (CurrentFrameChanged → triggers frame loading)
        self.handle_comp_events();

        // Handle drag-and-drop files/folders - queue for async loading
        ctx.input(|i| {
            let mut dropped: Vec<std::path::PathBuf> = Vec::new();
            for file in &i.raw.dropped_files {
                if let Some(path) = &file.path {
                    dropped.push(path.clone());
                }
            }
            if !dropped.is_empty() {
                info!("Files dropped: {:?}", dropped);
                let _ = self.load_sequences(dropped);
            }
        });

        if self.player.is_playing {
            ctx.request_repaint();
        }

        // Update status messages BEFORE laying out panels
        self.status_bar.update(ctx);

        // Status bar (bottom panel)
        if !self.is_fullscreen {
            self.status_bar.render(
                ctx,
                self.frame.as_ref(),
                &mut self.player,
                &self.viewport_state,
                self.last_render_time_ms,
            );
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.is_fullscreen {
                self.render_viewport_tab(ui);
            } else {
                let dock_style = egui_dock::Style::from_egui(ctx.style().as_ref());
                let mut dock_state =
                    std::mem::replace(&mut self.dock_state, PlayaApp::default_dock_state());
                {
                    let mut tabs = DockTabs { app: self };
                    DockArea::new(&mut dock_state)
                        .style(dock_style)
                        .show_inside(ui, &mut tabs);
                }
                self.dock_state = dock_state;
            }
        });

        // Process keyboard input after hover states were updated by panel rendering
        self.handle_keyboard_input(ctx);

        // Settings window (can be shown even in cinema mode)
        if self.show_settings {
            render_settings_window(ctx, &mut self.show_settings, &mut self.settings);
        }

        // Encode dialog (can be shown even in cinema mode)
        if self.show_encode_dialog
            && let Some(ref mut dialog) = self.encode_dialog
        {
            let active_comp = self
                .player
                .active_comp
                .as_ref()
                .and_then(|uuid| self.player.project.media.get(uuid));
            let should_stay_open = dialog.render(ctx, &self.player.project, active_comp);

            // Save dialog state (on every render - cheap clone)
            self.settings.encode_dialog = dialog.save_to_settings();

            if !should_stay_open {
                debug!("Encode dialog closed, settings saved to AppSettings");
                self.show_encode_dialog = false;
            }
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        // Gather all settings from components
        self.settings.fps_base = self.player.fps_base;
        self.settings.loop_enabled = self.player.loop_enabled;
        self.settings.current_shader = self.shader_manager.current_shader.clone();
        self.settings.show_help = self.show_help;
        self.settings.show_playlist = self.show_playlist;
        // Snapshot current project from runtime player into persisted field
        self.project = self.player.project.clone();

        // Save cache state separately (sequences + current frame)
        // Serialize and save app settings
        if let Ok(json) = serde_json::to_string(self) {
            storage.set_string(eframe::APP_KEY, json);
            debug!(
                "App state saved: FPS={}, Loop={}, Shader={}",
                self.settings.fps_base, self.settings.loop_enabled, self.settings.current_shader
            );
        }
    }

    fn on_exit(&mut self, gl: Option<&glow::Context>) {
        // Cleanup OpenGL resources
        if let Some(gl) = gl {
            let mut renderer = self.viewport_renderer.lock().unwrap();
            renderer.destroy(gl);
            debug!("ViewportRenderer resources cleaned up");
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize FFmpeg
    playa_ffmpeg::init()?;

    // Parse command-line arguments first (needed for log setup)
    let args = Args::parse();

    // Check if running without arguments (GUI mode) and print help
    let has_any_args = args.file_path.is_some()
        || !args.files.is_empty()
        || args.playlist.is_some()
        || args.fullscreen
        || args.start_frame.is_some()
        || args.autoplay
        || args.loop_playback != 1
        || args.range_start.is_some()
        || args.range_end.is_some()
        || args.range.is_some()
        || args.log_file.is_some()
        || args.verbosity > 0
        || args.config_dir.is_some();

    if !has_any_args {
        // Print help in GUI mode (no CLI arguments provided)
        use clap::CommandFactory;
        let mut cmd = Args::command();
        let _ = cmd.print_help();
        println!("\n");
    }

    // Create path configuration from CLI args and environment
    let path_config = config::PathConfig::from_env_and_cli(args.config_dir.clone());

    // Ensure directories exist
    if let Err(e) = config::ensure_dirs(&path_config) {
        eprintln!("Warning: Failed to create application directories: {}", e);
    }

    // Determine log level based on verbosity flags
    // 0 (default) = warn, 1 (-v) = info, 2 (-vv) = debug, 3+ (-vvv) = trace
    let log_level = match args.verbosity {
        0 => log::LevelFilter::Warn,
        1 => log::LevelFilter::Info,
        2 => log::LevelFilter::Debug,
        _ => log::LevelFilter::Trace,
    };

    // Initialize logger based on --log flag
    if let Some(log_path_opt) = &args.log_file {
        // File logging with specified verbosity level
        let log_path = log_path_opt
            .as_ref()
            .cloned()
            .unwrap_or_else(|| config::data_file("playa.log", &path_config));

        let file = std::fs::File::create(&log_path).expect("Failed to create log file");

        env_logger::Builder::new()
            .filter_level(log_level)
            .filter_module("egui", log::LevelFilter::Info) // Suppress egui DEBUG spam
            .filter_module("egui_taffy", log::LevelFilter::Warn) // Suppress taffy spam
            .format_timestamp_millis()
            .target(env_logger::Target::Pipe(Box::new(file)))
            .init();

        info!(
            "Logging to file: {} (level: {:?})",
            log_path.display(),
            log_level
        );
    } else {
        // Console logging with specified verbosity level (respects RUST_LOG if set)
        let default_level = match args.verbosity {
            0 => "warn",
            1 => "info",
            2 => "debug",
            _ => "trace",
        };

        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(default_level))
            .filter_module("egui", log::LevelFilter::Info) // Suppress egui DEBUG spam
            .filter_module("egui_taffy", log::LevelFilter::Warn) // Suppress taffy spam
            .format_timestamp_millis()
            .init();
    }

    info!("Playa Image Sequence Player starting...");
    debug!("Command-line args: {:?}", args);

    // Log application paths
    info!(
        "Config path: {}",
        config::config_file("playa.json", &path_config).display()
    );
    info!(
        "Data path: {}",
        config::data_file("playa_data.json", &path_config)
            .parent()
            .unwrap()
            .display()
    );

    if let Some(ref path) = args.file_path {
        info!("Input file: {}", path.display());
    } else {
        info!("No input file provided, starting with empty state (drag-and-drop supported)");
    }

    // Determine EXR backend at compile time
    const BACKEND: &str = if cfg!(feature = "openexr") {
        "openexr-rs"
    } else {
        "exrs"
    };

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!(
                "Playa v{} • {} • F1 for help",
                env!("CARGO_PKG_VERSION"),
                BACKEND
            ))
            .with_resizable(true)
            .with_drag_and_drop(true),
        persist_window: true,
        #[cfg(not(target_arch = "wasm32"))]
        persistence_path: Some(config::config_file("playa.json", &path_config)),
        ..Default::default()
    };

    info!("Starting Playa with window persistence and drag-and-drop enabled");

    // Clone path_config for the closure
    let path_config_for_app = path_config.clone();

    // Run the app
    eframe::run_native(
        "Playa",
        native_options,
        Box::new(move |cc| {
            // Load persisted app state if available, otherwise create default
            let mut app: PlayaApp = cc
                .storage
                .and_then(|storage| storage.get_string(eframe::APP_KEY))
                .and_then(|json| serde_json::from_str(&json).ok())
                .unwrap_or_else(|| {
                    info!("No persisted state found, creating default app");
                    PlayaApp::default()
                });

            // Recreate Player with CLI- or Settings-configured cache memory/worker settings
            // and rewire status bar + path sender
            let mem_fraction = args
                .mem_percent
                .map(|p| (p / 100.0).clamp(0.05, 0.95))
                .unwrap_or(0.75);

            // workers_override in settings controls App-level workers; we keep it for future use
            let _workers = args.workers.or(if app.settings.workers_override > 0 {
                Some(app.settings.workers_override as usize)
            } else {
                None
            });

            // Recreate Player runtime from persisted project
            let mut player = Player::new();
            player.project = app.project.clone();

            // Rebuild Arc references and set event sender for all comps
            player
                .project
                .rebuild_runtime(Some(app.comp_event_sender.clone()));

            // Ensure default comp exists and set as active
            let default_comp_uuid = player.project.ensure_default_comp();
            if player.active_comp.is_none() {
                player.active_comp = Some(default_comp_uuid);
            }

            app.player = player;
            app.status_bar = StatusBar::new();
            app.applied_mem_fraction = mem_fraction;
            app.applied_workers = _workers;
            app.path_config = path_config_for_app;

            // Attempt to load shaders from the shaders directory
            if app
                .shader_manager
                .load_shader_directory(&std::path::PathBuf::from("shaders"))
                .is_err()
            {
                log::info!("Shaders folder does not exist, skipping external shader loading");
            }

            // Apply persisted settings to components
            app.player.fps_base = app.settings.fps_base;
            app.player.fps_play = app.settings.fps_base; // Initialize fps_play from base
            app.player.loop_enabled = app.settings.loop_enabled;
            app.shader_manager.current_shader = app.settings.current_shader.clone();
            app.show_help = app.settings.show_help;
            info!(
                "Applied settings: FPS={}, Loop={}, Shader={}, Help={}",
                app.settings.fps_base,
                app.settings.loop_enabled,
                app.settings.current_shader,
                app.show_help
            );

            // Restore selected media item (activate if exists)
            if let Some(selected_uuid) = app.selected_media_uuid.clone() {
                if app.player.project.media.contains_key(&selected_uuid) {
                    app.select_item(selected_uuid.clone());
                    info!("Restored selected media: {}", selected_uuid);
                }
            }

            // CLI arguments have priority
            let has_cli_input =
                args.file_path.is_some() || !args.files.is_empty() || args.playlist.is_some();

            if has_cli_input {
                info!("CLI arguments provided, loading sequences");

                // Collect all file paths in order: positional arg, -f flags, -p playlist
                let mut all_files = Vec::new();

                if let Some(ref path) = args.file_path {
                    all_files.push(path.clone());
                }

                all_files.extend(args.files.iter().cloned());

                // Load files
                if !all_files.is_empty() {
                    let _ = app.load_sequences(all_files);
                }

                // Load playlist as Project
                if let Some(ref playlist_path) = args.playlist {
                    info!("Loading playlist: {}", playlist_path.display());
                    match crate::entities::Project::from_json(playlist_path) {
                        Ok(mut project) => {
                            // Rebuild runtime with event sender for all comps
                            project.rebuild_runtime(Some(app.comp_event_sender.clone()));
                            app.player.project = project;
                            info!("Playlist loaded via Project");
                        }
                        Err(e) => {
                            warn!("Failed to load playlist {}: {}", playlist_path.display(), e);
                        }
                    }
                }

                // Apply CLI options
                if let Some(frame) = args.start_frame {
                    app.player.set_frame(frame);
                }

                if args.autoplay {
                    app.player.is_playing = true;
                }

                app.player.loop_enabled = args.loop_playback != 0;

                // Set play range
                let (range_start, range_end) = if let Some(ref range) = args.range {
                    (Some(range[0]), Some(range[1]))
                } else {
                    (args.range_start, args.range_end)
                };

                if let (Some(start), Some(end)) = (range_start, range_end) {
                    app.player.set_play_range(start, end);
                }

                // Set fullscreen
                if args.fullscreen {
                    app.set_cinema_mode(&cc.egui_ctx, true);
                }
            }

            Ok(Box::new(app))
        }),
    )?;

    info!("Application exiting");
    Ok(())
}
