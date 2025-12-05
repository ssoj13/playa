//! Standalone Project window for development and testing.
//!
//! Displays only the Project panel UI without timeline, viewport, or other components.
//! Useful for fine-tuning the Project UI and testing media management.

use std::sync::Arc;

use eframe::egui;
use playa::cache_man::CacheManager;
use playa::entities::Project;
use playa::event_bus::{downcast_event, EventBus};
use playa::global_cache::CacheStrategy;
use playa::main_events::{handle_app_event, EventResult};
use playa::player::Player;
use playa::project_events::*;
use playa::widgets::project::project_ui;

fn main() -> eframe::Result<()> {
    // Initialize logger
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([400.0, 600.0])
            .with_title("Playa - Project Panel"),
        ..Default::default()
    };

    eframe::run_native(
        "playa-project",
        options,
        Box::new(|_cc| Ok(Box::new(ProjectApp::new()))),
    )
}

/// Minimal application state for standalone Project panel
struct ProjectApp {
    project: Project,
    player: Player,
    event_bus: EventBus,
    cache_manager: Arc<CacheManager>,
    error_msg: Option<String>,
}

impl ProjectApp {
    fn new() -> Self {
        // Create cache manager
        let cache_manager = Arc::new(CacheManager::new(0.75, 2.0));

        // Create project with cache
        let project = Project::new_with_strategy(Arc::clone(&cache_manager), CacheStrategy::All);

        // Create minimal player
        let player = Player::new();

        // Create event bus
        let event_bus = EventBus::new();

        Self {
            project,
            player,
            event_bus,
            cache_manager,
            error_msg: None,
        }
    }

    /// Handle deferred actions from event processing
    fn handle_deferred(&mut self, result: EventResult) {
        // Load sequences
        if let Some(paths) = result.load_sequences {
            self.load_sequences(&paths);
        }

        // Save project
        if let Some(path) = result.save_project {
            if let Err(e) = self.project.to_json(&path) {
                self.error_msg = Some(format!("Save failed: {}", e));
            } else {
                self.project.set_last_save_path(Some(path));
                log::info!("Project saved");
            }
        }

        // Load project
        if let Some(path) = result.load_project {
            match Project::from_json(&path) {
                Ok(mut loaded) => {
                    loaded.rebuild_with_manager(Arc::clone(&self.cache_manager), None);
                    self.project = loaded;
                    self.project.set_last_save_path(Some(path));
                    // Set first comp as active
                    let first = self.project.comps_order().first().cloned();
                    self.player.set_active_comp(first, &mut self.project);
                    log::info!("Project loaded");
                }
                Err(e) => {
                    self.error_msg = Some(format!("Load failed: {}", e));
                }
            }
        }

        // New comp
        if let Some((name, fps)) = result.new_comp {
            let emitter = playa::event_bus::CompEventEmitter::from_emitter(self.event_bus.emitter());
            let uuid = self.project.create_comp(&name, fps, emitter);
            self.player.set_active_comp(Some(uuid), &mut self.project);
            log::info!("Created comp: {}", name);
        }

        // Quick save
        if result.quick_save {
            if let Some(path) = self.project.last_save_path() {
                if let Err(e) = self.project.to_json(&path) {
                    self.error_msg = Some(format!("Quick save failed: {}", e));
                } else {
                    log::info!("Quick saved to {:?}", path);
                }
            } else {
                self.error_msg = Some("No save path set. Use Save first.".to_string());
            }
        }

        // Show open dialog
        if result.show_open_dialog {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Playa Project", &["json"])
                .set_title("Open Project")
                .pick_file()
            {
                self.event_bus.emit(LoadProjectEvent(path));
            }
        }
    }

    /// Load image sequences from paths
    fn load_sequences(&mut self, paths: &[std::path::PathBuf]) {
        use playa::entities::Comp;

        for path in paths {
            match Comp::detect_from_paths(vec![path.clone()]) {
                Ok(comps) => {
                    for mut comp in comps {
                        // Set up comp with event emitter
                        let emitter = playa::event_bus::CompEventEmitter::from_emitter(
                            self.event_bus.emitter(),
                        );
                        comp.set_event_emitter(emitter);
                        let uuid = comp.get_uuid();
                        self.project.add_comp(comp);

                        // Set as active if first
                        if self.player.active_comp().is_none() {
                            self.player.set_active_comp(Some(uuid), &mut self.project);
                        }
                    }
                    log::info!("Loaded: {:?}", path);
                }
                Err(e) => {
                    log::error!("Failed to load {:?}: {}", path, e);
                    self.error_msg = Some(format!("Load failed: {}", e));
                }
            }
        }
    }
}

impl eframe::App for ProjectApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Top panel with error display
        if self.error_msg.is_some() {
            egui::TopBottomPanel::top("error_panel").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(egui::Color32::RED, self.error_msg.as_ref().unwrap());
                    if ui.button("X").clicked() {
                        self.error_msg = None;
                    }
                });
            });
        }

        // Bottom status panel
        egui::TopBottomPanel::bottom("status_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let media_count = self.project.media.read().unwrap().len();
                let selection_count = self.project.selection().len();
                let active = self.player.active_comp();

                ui.label(format!("Media: {}", media_count));
                ui.separator();
                ui.label(format!("Selected: {}", selection_count));
                ui.separator();
                if let Some(uuid) = active {
                    ui.label(format!("Active: {:.8}", uuid));
                } else {
                    ui.label("Active: None");
                }
            });
        });

        // Main project panel
        egui::CentralPanel::default().show(ctx, |ui| {
            let actions = project_ui::render(ui, &mut self.player, &self.project);

            // Emit all events from UI actions
            for evt in actions.events {
                self.event_bus.emit_boxed(evt);
            }
        });

        // Process events
        let events = self.event_bus.poll();
        for event in events {
            // Create dummy state for unused parameters
            let mut timeline_state = playa::widgets::timeline::TimelineState::default();
            let mut viewport_state = playa::widgets::viewport::ViewportState::new();
            let mut settings = playa::dialogs::prefs::AppSettings::default();
            let mut show_help = false;
            let mut show_playlist = false;
            let mut show_settings = false;
            let mut show_encode_dialog = false;
            let mut show_attributes_editor = false;
            let mut encode_dialog = None;
            let mut is_fullscreen = false;
            let mut fullscreen_dirty = false;
            let mut reset_settings_pending = false;

            if let Some(result) = handle_app_event(
                &event,
                &mut self.player,
                &mut self.project,
                &mut timeline_state,
                &mut viewport_state,
                &mut settings,
                &mut show_help,
                &mut show_playlist,
                &mut show_settings,
                &mut show_encode_dialog,
                &mut show_attributes_editor,
                &mut encode_dialog,
                &mut is_fullscreen,
                &mut fullscreen_dirty,
                &mut reset_settings_pending,
            ) {
                self.handle_deferred(result);
            }
        }

        // Request repaint for continuous updates
        ctx.request_repaint();
    }
}
