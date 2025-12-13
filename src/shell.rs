//! Shared shell module for standalone binary targets.
//!
//! Provides common initialization and event handling boilerplate.

use std::path::PathBuf;
use std::sync::Arc;

use crate::core::cache_man::CacheManager;
use crate::dialogs::prefs::AppSettings;
use crate::entities::Project;
use crate::core::event_bus::{CompEventEmitter, EventBus};
use crate::core::global_cache::CacheStrategy;
use crate::main_events::{handle_app_event, EventResult};
use crate::core::player::Player;
use crate::widgets::timeline::TimelineState;
use crate::widgets::viewport::ViewportState;
use crate::widgets::node_editor::NodeEditorState;

/// Common shell state shared by all standalone binaries
pub struct Shell {
    pub project: Project,
    pub player: Player,
    pub event_bus: EventBus,
    pub cache_manager: Arc<CacheManager>,
    pub comp_emitter: CompEventEmitter,
    pub error_msg: Option<String>,
}

impl Shell {
    /// Create new shell with empty project
    pub fn new() -> Self {
        let cache_manager = Arc::new(CacheManager::new(0.75, 2.0));
        let event_bus = EventBus::new();
        let comp_emitter = CompEventEmitter::from_emitter(event_bus.emitter());
        let project = Project::new_with_strategy(Arc::clone(&cache_manager), CacheStrategy::All);
        let player = Player::new();

        Self {
            project,
            player,
            event_bus,
            cache_manager,
            comp_emitter,
            error_msg: None,
        }
    }

    /// Create shell with test sequence loaded
    pub fn with_test_sequence(path: &str) -> Self {
        let mut shell = Self::new();
        shell.load_sequence(path);
        shell
    }

    /// Load a sequence from path
    pub fn load_sequence(&mut self, path: &str) {
        use crate::entities::FileNode;
        use crate::entities::node::Node;
        
        let path_buf = PathBuf::from(path);
        if !path_buf.exists() {
            self.error_msg = Some(format!("Test sequence not found: {}", path));
            log::warn!("Test sequence not found: {}", path);
            return;
        }
        
        match FileNode::detect_from_paths(vec![path_buf]) {
            Ok(nodes) => {
                if nodes.is_empty() {
                    self.error_msg = Some("No valid sequences detected".to_string());
                    log::warn!("No valid sequences detected from: {}", path);
                    return;
                }
                
                let mut first_uuid = None;
                for node in nodes {
                    let uuid = node.uuid();
                    log::info!("Adding FileNode: {} ({})", node.name(), uuid);
                    self.project.add_node(node.into());
                    if first_uuid.is_none() {
                        first_uuid = Some(uuid);
                    }
                }
                
                // Activate first loaded sequence
                if let Some(uuid) = first_uuid {
                    self.player.set_active_comp(Some(uuid), &mut self.project);
                }
                
                self.error_msg = None;
            }
            Err(e) => {
                self.error_msg = Some(format!("Failed to load sequence: {}", e));
                log::warn!("Failed to load sequence {}: {}", path, e);
            }
        }
    }

    /// Process events from the event bus, returns merged deferred actions
    pub fn process_events(&mut self) -> Option<EventResult> {
        let mut merged = EventResult::default();
        let mut any_handled = false;

        // Dummy state for unused parameters
        let mut timeline_state = TimelineState::default();
        let mut node_editor_state = NodeEditorState::new();
        let mut viewport_state = ViewportState::new();
        let mut settings = AppSettings::default();
        let mut show_help = false;
        let mut show_playlist = false;
        let mut show_settings = false;
        let mut show_encode_dialog = false;
        let mut show_attributes_editor = false;
        let mut encode_dialog = None;
        let mut is_fullscreen = false;
        let mut fullscreen_dirty = false;
        let mut reset_settings_pending = false;

        for event in self.event_bus.poll() {
            if let Some(r) = handle_app_event(
                &event,
                &mut self.player,
                &mut self.project,
                &mut timeline_state,
                &mut node_editor_state,
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
                merged.merge(r);
                any_handled = true;
            }
        }

        if any_handled { Some(merged) } else { None }
    }

    /// Process events with custom state (for timeline/viewport binaries)
    pub fn process_events_with_state(
        &mut self,
        timeline_state: &mut TimelineState,
        viewport_state: &mut ViewportState,
        settings: &mut AppSettings,
    ) -> Option<EventResult> {
        let mut merged = EventResult::default();
        let mut any_handled = false;

        let mut node_editor_state = NodeEditorState::new();
        let mut show_help = false;
        let mut show_playlist = false;
        let mut show_settings = false;
        let mut show_encode_dialog = false;
        let mut show_attributes_editor = false;
        let mut encode_dialog = None;
        let mut is_fullscreen = false;
        let mut fullscreen_dirty = false;
        let mut reset_settings_pending = false;

        for event in self.event_bus.poll() {
            if let Some(r) = handle_app_event(
                &event,
                &mut self.player,
                &mut self.project,
                timeline_state,
                &mut node_editor_state,
                viewport_state,
                settings,
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
                merged.merge(r);
                any_handled = true;
            }
        }

        if any_handled { Some(merged) } else { None }
    }

    /// Handle deferred actions from EventResult
    pub fn handle_deferred(&mut self, result: EventResult) {
        // Load sequences
        if let Some(paths) = result.load_sequences {
            for path in paths {
                self.load_sequence(&path.to_string_lossy());
            }
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
                    loaded.rebuild_with_manager(Arc::clone(&self.cache_manager), Some(self.comp_emitter.clone()));
                    self.project = loaded;
                    self.project.set_last_save_path(Some(path));
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
            let uuid = self.project.create_comp(&name, fps, self.comp_emitter.clone());
            self.player.set_active_comp(Some(uuid), &mut self.project);
            log::info!("Created comp: {}", name);
        }

        // Quick save
        if result.quick_save {
            if let Some(path) = self.project.last_save_path() {
                if let Err(e) = self.project.to_json(&path) {
                    self.error_msg = Some(format!("Quick save failed: {}", e));
                }
            }
        }
    }
}

impl Default for Shell {
    fn default() -> Self {
        Self::new()
    }
}

/// Initialize logging for standalone binaries
pub fn init_logger() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();
}

/// Default test sequence path (override with PLAYA_TEST_SEQUENCE env var)
pub fn test_sequence() -> String {
    std::env::var("PLAYA_TEST_SEQUENCE")
        .unwrap_or_else(|_| r"D:\_demo\Srcs\Kz\kz.0000.tif".to_string())
}
