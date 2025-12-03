mod cache_man;
mod cli;
mod config;
mod dialogs;
mod entities;
mod event_bus;
mod global_cache;
mod main_events;
mod player;
mod player_events;
mod project_events;
mod ui;
mod utils;
mod widgets;
mod workers;

use cache_man::CacheManager;
use clap::Parser;
use cli::Args;
use dialogs::encode::EncodeDialog;
use dialogs::prefs::{AppSettings, HotkeyHandler, render_settings_window};
use dialogs::prefs::prefs_events::HotkeyWindow;
use eframe::{egui, glow};
use event_bus::{CompEventSender, EventBus, downcast_event};
use egui_dock::{DockArea, DockState, NodeIndex, TabViewer};
use entities::Frame;
use entities::Project;
use log::{debug, error, info, warn};
use player::Player;
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;
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
    selected_media_uuid: Option<Uuid>,
    #[serde(skip)]
    last_render_time_ms: f32,
    /// Last time cache stats were logged (for periodic logging)
    #[serde(skip)]
    last_stats_log_time: f64,
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
    show_attributes_editor: bool,
    #[serde(skip)]
    is_fullscreen: bool,
    #[serde(skip)]
    fullscreen_dirty: bool,
    #[serde(skip)]
    reset_settings_pending: bool,
    #[serde(skip)]
    applied_mem_fraction: f64,
    #[serde(skip)]
    applied_workers: Option<usize>,
    #[serde(skip)]
    path_config: config::PathConfig,
    /// Global cache manager (memory tracking + epoch)
    #[serde(skip)]
    cache_manager: Arc<CacheManager>,
    /// Global worker pool for background tasks (frame loading, encoding)
    #[serde(skip)]
    workers: Arc<Workers>,
    /// Event sender for compositions (shared across all comps)
    #[serde(skip)]
    comp_event_sender: CompEventSender,
    /// Global event bus for application-wide events
    #[serde(skip)]
    event_bus: EventBus,
    #[serde(default = "PlayaApp::default_dock_state")]
    dock_state: DockState<DockTab>,
    /// Hotkey handler for context-aware keyboard shortcuts
    #[serde(skip)]
    hotkey_handler: HotkeyHandler,
    /// Currently focused window for input routing
    #[serde(skip)]
    focused_window: HotkeyWindow,
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
        // Create global cache manager (memory tracking + epoch)
        let cache_manager = Arc::new(CacheManager::new(0.75, 2.0));

        // Create player (no longer owns project)
        let player = Player::new();
        let status_bar = StatusBar::new();

        // Create worker pool (75% of CPU cores for workers, 25% for UI thread)
        let num_workers = (num_cpus::get() * 3 / 4).max(1);
        let workers = Arc::new(Workers::new(num_workers, cache_manager.epoch_ref()));

        // Create global event bus and comp event sender
        let event_bus = EventBus::new();
        let comp_event_sender = CompEventSender::from_sender(event_bus.sender());

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
            last_stats_log_time: 0.0,
            settings: AppSettings::default(),
            project: {
                let settings = AppSettings::default();
                Project::new_with_strategy(Arc::clone(&cache_manager), settings.cache_strategy)
            },
            show_help: true,
            show_playlist: true,
            show_settings: false,
            show_encode_dialog: false,
            show_attributes_editor: true,
            encode_dialog: None,
            is_fullscreen: false,
            fullscreen_dirty: false,
            reset_settings_pending: false,
            applied_mem_fraction: 0.75,
            applied_workers: None,
            path_config: config::PathConfig::from_env_and_cli(None),
            cache_manager,
            workers,
            comp_event_sender,
            event_bus,
            dock_state: PlayaApp::default_dock_state(),
            hotkey_handler: {
                let mut handler = HotkeyHandler::new();
                handler.setup_default_bindings();
                handler
            },
            focused_window: HotkeyWindow::Global,
            viewport_hovered: false,
            timeline_hovered: false,
            project_hovered: false,
            attributes_state: AttributesState::default(),
        }
    }
}

impl PlayaApp {
    fn default_dock_state() -> DockState<DockTab> {
        // By default show both Project and Attributes with default split position
        Self::build_dock_state(true, true, 0.6)
    }

    /// Attach composition event sender to all comps in the current project.
    fn attach_comp_event_sender(&mut self) {
        let sender = self.comp_event_sender.clone();
        for comp in self.project.media.write().unwrap().values_mut() {
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
                let mut first_uuid: Option<Uuid> = None;
                for comp in comps {
                    let uuid = comp.uuid;
                    let name = comp.attrs.get_str("name").unwrap_or("Untitled").to_string();
                    info!("Adding clip (File mode): {} ({})", name, uuid);

                    // add_comp() injects global_cache + cache_manager AND adds to comps_order
                    self.project.add_comp(comp);

                    // Remember first sequence for activation
                    if self.player.active_comp().is_none() && first_uuid.is_none() {
                        first_uuid = Some(uuid);
                    }
                }

                self.attach_comp_event_sender();

                // Activate first sequence and trigger frame loading
                if let Some(uuid) = first_uuid {
                    self.player.set_active_comp_uuid(Some(uuid));
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
    /// Unified interface: works for both File mode and Layer mode.
    /// File mode: loads frames from disk using spiral/forward strategies
    /// Layer mode: composes frames from children (on-demand for now)
    fn enqueue_frame_loads_around_playhead(&self, _radius: usize) {
        // Get active comp
        let Some(comp_uuid) = self.player.active_comp() else {
            debug!("No active comp for frame loading");
            return;
        };
        let Some(comp) = self.project.get_comp(comp_uuid) else {
            debug!("Active comp {} not found in media", comp_uuid);
            return;
        };

        // Trigger preload (works for both File and Layer modes)
        comp.signal_preload(&self.workers, &self.project, None);
    }

    /// Handle all events from the event bus (comp events, app events, etc.)
    fn handle_events(&mut self) {
        use entities::comp_events::*;

        // Deferred actions to execute after event loop
        let mut deferred_load_project: Option<std::path::PathBuf> = None;
        let mut deferred_save_project: Option<std::path::PathBuf> = None;
        let mut deferred_load_sequences: Option<Vec<std::path::PathBuf>> = None;
        let mut deferred_enqueue_frames: Option<usize> = None;

        // Drain all events from the bus
        for event in self.event_bus.drain() {
            // === Comp events (high priority, internal) ===
            if let Some(e) = downcast_event::<CurrentFrameChangedEvent>(&event) {
                debug!("Comp {} frame changed: {} → {}", e.comp_uuid, e.old_frame, e.new_frame);
                self.enqueue_frame_loads_around_playhead(10);
                continue;
            }
            if let Some(e) = downcast_event::<LayersChangedEvent>(&event) {
                debug!("Comp {} layers changed", e.0);
                self.displayed_frame = None;
                continue;
            }
            if let Some(e) = downcast_event::<TimelineChangedEvent>(&event) {
                debug!("Comp {} timeline changed", e.0);
                continue;
            }
            if let Some(e) = downcast_event::<AttrsChangedEvent>(&event) {
                debug!("Comp {} attrs changed - triggering cascade invalidation", e.0);
                self.project.invalidate_cascade(e.0);
                continue;
            }

            // === App events - delegate to main_events module ===
            if let Some(result) = main_events::handle_app_event(
                &event,
                &mut self.player,
                &mut self.project,
                &mut self.timeline_state,
                &mut self.viewport_state,
                &mut self.settings,
                &mut self.show_help,
                &mut self.show_playlist,
                &mut self.show_settings,
                &mut self.show_encode_dialog,
                &mut self.show_attributes_editor,
                &mut self.encode_dialog,
                &mut self.is_fullscreen,
                &mut self.fullscreen_dirty,
                &mut self.reset_settings_pending,
            ) {
                // Process deferred actions from EventResult
                if let Some(path) = result.load_project {
                    deferred_load_project = Some(path);
                }
                if let Some(path) = result.save_project {
                    deferred_save_project = Some(path);
                }
                if let Some(paths) = result.load_sequences {
                    deferred_load_sequences = Some(paths);
                }
                if let Some(n) = result.enqueue_frames {
                    deferred_enqueue_frames = Some(n);
                }
            }
        }

        // Execute deferred actions outside the event loop (to avoid borrow conflicts)
        if let Some(path) = deferred_load_project {
            self.load_project(path);
        }
        if let Some(path) = deferred_save_project {
            self.save_project(path);
        }
        if let Some(paths) = deferred_load_sequences {
            let _ = self.load_sequences(paths);
        }
        if let Some(n) = deferred_enqueue_frames {
            self.enqueue_frame_loads_around_playhead(n);
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
        if let Err(e) = self.project.to_json(&path) {
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

                // Rebuild runtime + set cache manager (unified)
                project.rebuild_with_manager(
                    Arc::clone(&self.cache_manager),
                    Some(self.comp_event_sender.clone()),
                );

                self.project = project;
                // Restore active comp from project (also sync selection)
                if let Some(active) = self.project.active() {
                    self.player.set_active_comp(active, &mut self.project);
                } else {
                    // Ensure default if none
                    let uuid = self.project.ensure_default_comp();
                    self.player.set_active_comp(uuid, &mut self.project);
                }
                self.selected_media_uuid = self.project.selection().last().cloned();
                self.error_msg = None;
            }
            Err(e) => {
                error!("{}", e);
                self.error_msg = Some(e);
            }
        }
    }

    fn determine_focused_window(&self, ctx: &egui::Context) -> dialogs::prefs::prefs_events::HotkeyWindow {
        use dialogs::prefs::prefs_events::HotkeyWindow;

        // Priority 1: Modal dialogs (settings, encode) - always capture input
        if self.show_settings || self.show_encode_dialog {
            return HotkeyWindow::Global;
        }

        // Priority 2: Keyboard focus (text fields) - don't process hotkeys
        if ctx.wants_keyboard_input() {
            return HotkeyWindow::Global; // Return Global but will be filtered later
        }

        // Priority 3: Explicit viewport hover
        if self.viewport_hovered {
            return HotkeyWindow::Viewport;
        }

        // Priority 4: Default to timeline when a comp is active (no mouse gating)
        if self.player.active_comp().is_some() {
            return HotkeyWindow::Timeline;
        }

        // Priority 5: Project hover (if no active comp)
        if self.project_hovered {
            return HotkeyWindow::Project;
        }

        // Fallback to Global
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
            use crate::entities::comp_events::{AlignLayersStartEvent, AlignLayersEndEvent, TrimLayersStartEvent, TrimLayersEndEvent};

            // Fill comp_uuid for timeline-specific events
            if let Some(active_comp_uuid) = self.player.active_comp() {
                // Check if event needs comp_uuid filled in
                if downcast_event::<AlignLayersStartEvent>(&event).is_some() {
                    self.event_bus.send(AlignLayersStartEvent(active_comp_uuid));
                    return;
                }
                if downcast_event::<AlignLayersEndEvent>(&event).is_some() {
                    self.event_bus.send(AlignLayersEndEvent(active_comp_uuid));
                    return;
                }
                if downcast_event::<TrimLayersStartEvent>(&event).is_some() {
                    self.event_bus.send(TrimLayersStartEvent(active_comp_uuid));
                    return;
                }
                if downcast_event::<TrimLayersEndEvent>(&event).is_some() {
                    self.event_bus.send(TrimLayersEndEvent(active_comp_uuid));
                    return;
                }
            }

            self.event_bus.send_boxed(event);
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

        // All other hotkeys (playback, viewport, etc.) are routed via EventBus (HotkeyHandler)
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

    /// Remove media by UUID and clean up references in other comps.
    fn remove_media_and_cleanup(&mut self, uuid: Uuid) {
        if !self.project.media.read().unwrap().contains_key(&uuid) {
            log::warn!("remove_media_and_cleanup: uuid {} not found", uuid);
            return;
        }

        // Remove references from other comps (layers using this media as source)
        let mut removed_refs = 0usize;
        {
            let mut media = self.project.media.write().unwrap();
            for (comp_uuid, comp) in media.iter_mut() {
                // Find all child instances that use this source_uuid
                let children_to_remove = comp.find_children_by_source(uuid);
                for child_uuid in children_to_remove {
                    comp.remove_child(child_uuid);
                    removed_refs += 1;
                    log::info!(
                        "Removed child instance {} (source {}) from comp {} while deleting media",
                        child_uuid,
                        uuid,
                        comp_uuid
                    );
                }
            }
        } // Release write lock
        if removed_refs == 0 {
            log::debug!(
                "remove_media_and_cleanup: no layer references to {} found",
                uuid
            );
        }

        // Drop the media itself
        self.project.remove_media(uuid);

        // Clear selection/active pointers to this media
        if self.player.active_comp() == Some(uuid) {
            self.player.set_active_comp_uuid(None);
        }
        if self.selected_media_uuid == Some(uuid) {
            self.selected_media_uuid = None;
        }
        self.project.retain_selection(|u| *u != uuid);
        // Anchor may no longer be valid; recompute later
        self.project.selection_anchor = None;
    }

    /// After deletions, ensure active/selection are valid and not empty.
    fn post_remove_fixups(&mut self) {
        // Ensure active comp exists
        if self.player.active_comp().is_none() {
            if self.project.media.read().unwrap().is_empty() {
                let uuid = self.project.ensure_default_comp();
                self.player.set_active_comp(uuid, &mut self.project);
            } else {
                // Pick last in order or any available
                let order = self.project.comps_order();
                let fallback = order
                    .last()
                    .cloned()
                    .or_else(|| self.project.media.read().unwrap().keys().next().cloned());
                if let Some(uuid) = fallback {
                    self.player.set_active_comp(uuid, &mut self.project);
                }
            }
        }

        // Ensure selection exists and points to valid items
        {
            let valid_uuids: Vec<Uuid> = {
                let media = self.project.media.read().unwrap();
                self.project.selection().into_iter().filter(|u| media.contains_key(u)).collect()
            };
            self.project.set_selection(valid_uuids);
        }
        if self.project.selection().is_empty() {
            if let Some(active) = self.player.active_comp() {
                self.project.push_selection(active);
                self.project.selection_anchor = self
                    .project
                    .comps_order()
                    .iter()
                    .position(|u| *u == active);
                self.selected_media_uuid = Some(active);
            }
        } else {
            let sel = self.project.selection();
            let order = self.project.comps_order();
            self.selected_media_uuid = sel.last().copied();
            self.project.selection_anchor = sel.last().and_then(
                |u| order.iter().position(|c| c == u),
            );
        }
    }

    /// Select and activate media item (comp/clip) by UUID
    fn select_item(&mut self, uuid: Uuid) {
        self.selected_media_uuid = Some(uuid);
        self.event_bus
            .send(project_events::ProjectActiveChangedEvent(uuid));
    }

    /// Update compositor backend based on settings
    fn update_compositor_backend(&mut self, gl: &std::sync::Arc<glow::Context>) {
        use entities::compositor::{CompositorType, CpuCompositor};
        use entities::gpu_compositor::GpuCompositor;

        let desired_backend = match self.settings.compositor_backend {
            dialogs::prefs::CompositorBackend::Cpu => CompositorType::Cpu(CpuCompositor),
            dialogs::prefs::CompositorBackend::Gpu => {
                CompositorType::Gpu(GpuCompositor::new(gl.clone()))
            }
        };

        // Check if compositor type changed
        let current_is_cpu = matches!(
            *self.project.compositor.borrow(),
            CompositorType::Cpu(_)
        );
        let desired_is_cpu = matches!(desired_backend, CompositorType::Cpu(_));

        if current_is_cpu != desired_is_cpu {
            info!(
                "Switching compositor to: {:?}",
                self.settings.compositor_backend
            );
            self.project.set_compositor(desired_backend);
        }
    }

    fn render_project_tab(&mut self, ui: &mut egui::Ui) {
        let project_actions = widgets::project::render(ui, &mut self.player, &self.project);

        // Store hover state for input routing
        self.project_hovered = project_actions.hovered;

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

        // Dispatch queued events from Project UI
        for evt in project_actions.events {
            self.event_bus.send(evt);
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

            // add_comp() injects global_cache + cache_manager AND adds to comps_order
            self.project.add_comp(comp);

            // Activate the new comp
            self.player.set_active_comp(uuid.clone(), &mut self.project);

            info!("Created new comp: {}", uuid);
        }

        // Remove composition
        if let Some(comp_uuid) = project_actions.remove_comp {
            self.project.media.write().unwrap().remove(&comp_uuid);
            self.project.retain_comps_order(|uuid| uuid != &comp_uuid);

            // If removed comp was active, switch to first available or None
            if self.player.active_comp().as_ref() == Some(&comp_uuid) {
                let first_comp = self.project.comps_order().first().cloned();
                if let Some(new_active) = first_comp {
                    self.player.set_active_comp(new_active, &mut self.project);
                } else {
                    self.player.set_active_comp_uuid(None);
                }
            }

            info!("Removed comp {}", comp_uuid);
        }

        // Clear all compositions
        if project_actions.clear_all_comps {
            // Remove all media (clips and comps are unified now)
            self.project.media.write().unwrap().clear();
            self.project.set_comps_order(Vec::new());
            self.player.set_active_comp_uuid(None);
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
            &self.project,
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
        let texture_needs_upload = self.displayed_frame != Some(self.player.current_frame(&self.project));

        // If the frame has changed, update our cached frame
        if texture_needs_upload {
            self.frame = self.player.get_current_frame(&self.project);
            self.displayed_frame = Some(self.player.current_frame(&self.project));
        }

        let (viewport_actions, render_time) = widgets::viewport::render(
            ui,
            self.frame.as_ref(),
            self.error_msg.as_ref(),
            &mut self.player,
            &mut self.project,
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

    fn sync_dock_tabs_visibility(&mut self) {
        // Check which optional tabs should be visible
        let show_project = self.show_playlist;
        let show_attributes = self.show_attributes_editor;

        // Get current visibility state
        let current_tabs: Vec<DockTab> = self.dock_state
            .iter_all_tabs()
            .map(|(_, tab)| tab.clone())
            .collect();

        let current_has_project = current_tabs.contains(&DockTab::Project);
        let current_has_attributes = current_tabs.contains(&DockTab::Attributes);

        // If visibility state changed, rebuild dock structure with saved position
        if show_project != current_has_project || show_attributes != current_has_attributes {
            self.dock_state = Self::build_dock_state(
                show_project,
                show_attributes,
                self.attributes_state.project_attributes_split,
            );
        }
    }

    /// Save current split position (call after DockArea rendering)
    fn save_dock_split_positions(&mut self) {
        if let Some(pos) = self.extract_project_attributes_split() {
            self.attributes_state.project_attributes_split = pos;
        }
    }

    /// Extract the current split position between Project and Attributes panels
    fn extract_project_attributes_split(&self) -> Option<f32> {
        // In our dock layout:
        // - First vertical split = Viewport/Timeline (0.65)
        // - Second vertical split = Project/Attributes (user's position)
        // So we need to find the SECOND vertical split, not the first
        use egui_dock::Node;

        let surface = self.dock_state.main_surface();
        let mut vertical_count = 0;

        for node in surface.iter() {
            if let Node::Vertical(split_node) = node {
                vertical_count += 1;
                // Return the second vertical split we find
                if vertical_count == 2 {
                    return Some(split_node.fraction);
                }
            }
        }
        None
    }

    fn build_dock_state(show_project: bool, show_attributes: bool, split_pos: f32) -> DockState<DockTab> {
        let mut dock_state = DockState::new(vec![DockTab::Viewport]);

        // Always split viewport and timeline vertically
        let [viewport, _timeline] = dock_state.main_surface_mut().split_below(
            NodeIndex::root(),
            0.65,
            vec![DockTab::Timeline],
        );

        if show_project || show_attributes {
            if show_project && show_attributes {
                // Both: create right panel with Project, then split it to add Attributes below
                let [_viewport, right_panel] = dock_state
                    .main_surface_mut()
                    .split_right(viewport, 0.75, vec![DockTab::Project]);

                // Split right panel vertically: Project stays on top, Attributes below
                // Use saved split position
                let _ = dock_state.main_surface_mut().split_below(
                    right_panel,
                    split_pos,
                    vec![DockTab::Attributes],
                );
            } else if show_project {
                // Only Project
                let _ = dock_state
                    .main_surface_mut()
                    .split_right(viewport, 0.75, vec![DockTab::Project]);
            } else {
                // Only Attributes
                let _ = dock_state
                    .main_surface_mut()
                    .split_right(viewport, 0.75, vec![DockTab::Attributes]);
            }
        }

        dock_state
    }

    fn render_attributes_tab(&mut self, ui: &mut egui::Ui) {
        if let Some(active) = self.player.active_comp() {
            self.project.modify_comp(active, |comp| {
                // Collect selected layers (now UUIDs instead of indices)
                let selection: Vec<Uuid> = comp.layer_selection.clone();

                if selection.len() > 1 {
                    // Multi-select: compute intersection of attribute keys
                    use std::collections::{BTreeSet, HashSet};
                    let mut common_keys: BTreeSet<String> = BTreeSet::new();
                    let mut first = true;
                    for instance_uuid in selection.iter() {
                        if let Some(attrs) = comp.children_attrs_get(instance_uuid) {
                            let keys: BTreeSet<String> =
                                attrs.iter().map(|(k, _)| k.clone()).collect();
                            if first {
                                common_keys = keys;
                                first = false;
                            } else {
                                common_keys =
                                    common_keys.intersection(&keys).cloned().collect();
                            }
                        }
                    }

                    if common_keys.is_empty() {
                        ui.label("(no common attributes)");
                        return;
                    }

                    // Build merged view: take values from first selected; mark mixed when others differ
                    let mut merged: crate::entities::Attrs = crate::entities::Attrs::new();
                    let mut mixed_keys: HashSet<String> = HashSet::new();

                    if let Some(first_uuid) = selection.first() {
                        if let Some(attrs) = comp.children_attrs_get(first_uuid) {
                            for key in common_keys.iter() {
                                if let Some(v) = attrs.get(key) {
                                    merged.set(key.clone(), v.clone());
                                }
                            }
                        }
                    }

                    for key in common_keys.iter() {
                        if let Some(base) = merged.get(key) {
                            for instance_uuid in selection.iter() {
                                if let Some(attrs) = comp.children_attrs_get(instance_uuid) {
                                    if let Some(other) = attrs.get(key) {
                                        if other != base {
                                            mixed_keys.insert(key.clone());
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Render merged attrs; collect changes and apply to all selected layers
                    let mut changed: Vec<(String, crate::entities::AttrValue)> = Vec::new();
                    crate::widgets::ae::render_with_mixed(
                        ui,
                        &mut merged,
                        &mut self.attributes_state,
                        "Multiple layers",
                        &mixed_keys,
                        &mut changed,
                    );

                    if !changed.is_empty() {
                        for (key, val) in changed {
                            for instance_uuid in selection.iter() {
                                if let Some(attrs) = comp.children_attrs_get_mut(instance_uuid) {
                                    attrs.set(key.clone(), val.clone());
                                }
                            }
                        }
                    }
                } else if let Some(layer_uuid) = selection.first() {
                    // Single layer selected
                    let layer_idx = comp.uuid_to_idx(*layer_uuid).unwrap_or(0);
                    if let Some(attrs) = comp.children_attrs_get_mut(layer_uuid) {
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
                    // No layer selected - show comp attributes
                    let comp_name = comp.name().to_string();
                    crate::widgets::ae::render(
                        ui,
                        &mut comp.attrs,
                        &mut self.attributes_state,
                        &comp_name,
                    );
                }
            });
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
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // Get GL context and update compositor backend
        if let Some(gl) = frame.gl() {
            self.update_compositor_backend(gl);
        }

        // Process all events from the event bus
        self.handle_events();

        // Periodic cache statistics logging (every 10 seconds)
        let current_time = ctx.input(|i| i.time);
        if current_time - self.last_stats_log_time > 10.0 {
            if let Some(ref global_cache) = self.project.global_cache {
                let stats = global_cache.stats();
                let cache_size = global_cache.len();
                log::info!(
                    "Cache stats: {} entries | hits: {} | misses: {} | hit rate: {:.1}%",
                    cache_size,
                    stats.hits(),
                    stats.misses(),
                    stats.hit_rate() * 100.0
                );
            }
            self.last_stats_log_time = current_time;
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

        // Apply pending fullscreen changes requested via events
        if self.fullscreen_dirty {
            self.set_cinema_mode(ctx, self.is_fullscreen);
            self.fullscreen_dirty = false;
        }

        // Apply pending settings reset requested via events
        if self.reset_settings_pending {
            self.reset_settings(ctx);
            if self.is_fullscreen {
                self.set_cinema_mode(ctx, false);
            }
            self.reset_settings_pending = false;
        }

        // Enable multipass for better taffy layout recalculation responsiveness
        ctx.options_mut(|opts| {
            opts.max_passes = std::num::NonZeroUsize::new(2).unwrap();
        });

        // Apply memory settings from UI if changed
        let mem_fraction = (self.settings.cache_memory_percent as f64 / 100.0).clamp(0.25, 0.95);
        let reserve_gb = self.settings.reserve_system_memory_gb as f64;

        if (mem_fraction - self.applied_mem_fraction).abs() > f64::EPSILON {
            // Update cache manager with new limits
            Arc::get_mut(&mut self.cache_manager)
                .map(|cm| cm.set_memory_limit(mem_fraction, reserve_gb));
            self.applied_mem_fraction = mem_fraction;
            info!("Memory limit updated: {}% (reserve {} GB)", mem_fraction * 100.0, reserve_gb);
        }

        self.player.update(&mut self.project);

        // Handle composition events (CurrentFrameChanged → triggers frame loading)
        self.handle_events();

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

        if self.player.is_playing() {
            ctx.request_repaint();
        }

        // Update status messages BEFORE laying out panels
        self.status_bar.update(ctx);

        // Status bar (bottom panel)
        if !self.is_fullscreen {
            let cache_mgr = self.project.cache_manager().map(Arc::clone);
            self.status_bar.render(
                ctx,
                self.frame.as_ref(),
                &mut self.player,
                &self.project,
                &self.viewport_state,
                self.last_render_time_ms,
                cache_mgr.as_ref(),
            );
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.is_fullscreen {
                self.render_viewport_tab(ui);
            } else {
                // Remove hidden tabs before rendering
                self.sync_dock_tabs_visibility();

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

                // Save split positions after DockArea rendering (only if changed by user)
                self.save_dock_split_positions();
            }
        });

        // Process keyboard input after hover states were updated by panel rendering
        self.handle_keyboard_input(ctx);

        // Settings window (can be shown even in cinema mode)
        if self.show_settings {
            let old_strategy = self.settings.cache_strategy;
            render_settings_window(ctx, &mut self.show_settings, &mut self.settings);

            // Apply cache strategy changes immediately
            if self.settings.cache_strategy != old_strategy {
                log::info!("Cache strategy changed to: {:?}", self.settings.cache_strategy);
                if let Some(ref global_cache) = self.project.global_cache {
                    global_cache.set_strategy(self.settings.cache_strategy);
                }
                if let Some(ref global_cache) = self.project.global_cache {
                    global_cache.set_strategy(self.settings.cache_strategy);
                }
            }
        }

        // Encode dialog (can be shown even in cinema mode)
        if self.show_encode_dialog
            && let Some(ref mut dialog) = self.encode_dialog
        {
            let media = self.project.media.read().unwrap();
            let active_comp = self
                .player
                .active_comp()
                .and_then(|uuid| media.get(&uuid));
            let should_stay_open = dialog.render(ctx, &self.project, active_comp);

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
        self.settings.fps_base = self.player.fps_base();
        self.settings.loop_enabled = self.player.loop_enabled();
        self.settings.current_shader = self.shader_manager.current_shader.clone();
        self.settings.show_help = self.show_help;
        self.settings.show_playlist = self.show_playlist;
        self.settings.show_attributes_editor = self.show_attributes_editor;
        // Snapshot current project from runtime player into persisted field
        self.project = self.project.clone();

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

            // Recreate Player runtime (no longer owns project)
            let mut player = Player::new();

            // Rebuild runtime + set cache manager (unified, lost during clone)
            app.project.rebuild_with_manager(
                Arc::clone(&app.cache_manager),
                Some(app.comp_event_sender.clone()),
            );

            // Restore active from project or ensure default
            let active_uuid = app.project.active().or_else(|| {
                let uuid = app.project.ensure_default_comp();
                Some(uuid)
            });
            if let Some(active) = active_uuid {
                player.set_active_comp_uuid(Some(active.clone()));
                app.project.set_active(Some(active));
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
            app.player.set_fps_base(app.settings.fps_base);
            app.player.set_fps_play(app.settings.fps_base); // Initialize fps_play from base
            app.player.set_loop_enabled(app.settings.loop_enabled);
            app.shader_manager.current_shader = app.settings.current_shader.clone();
            app.show_help = app.settings.show_help;
            app.show_playlist = app.settings.show_playlist;
            app.show_attributes_editor = app.settings.show_attributes_editor;
            info!(
                "Applied settings: FPS={}, Loop={}, Shader={}, Help={}",
                app.settings.fps_base,
                app.settings.loop_enabled,
                app.settings.current_shader,
                app.show_help
            );

            // Restore selection/active handled via project fields already

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
                            // Rebuild runtime + set cache manager (unified)
                            project.rebuild_with_manager(
                                Arc::clone(&app.cache_manager),
                                Some(app.comp_event_sender.clone()),
                            );

                            app.project = project;
                            info!("Playlist loaded via Project");
                        }
                        Err(e) => {
                            warn!("Failed to load playlist {}: {}", playlist_path.display(), e);
                        }
                    }
                }

                // Apply CLI options
                if let Some(frame) = args.start_frame {
                    app.player.set_frame(frame, &mut app.project);
                }

                if args.autoplay {
                    app.player.set_is_playing(true);
                }

                app.player.set_loop_enabled(args.loop_playback != 0);

                // Set play range
                let (range_start, range_end) = if let Some(ref range) = args.range {
                    (Some(range[0]), Some(range[1]))
                } else {
                    (args.range_start, args.range_end)
                };

                if let (Some(start), Some(end)) = (range_start, range_end) {
                    app.player.set_play_range(start, end, &mut app.project);
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