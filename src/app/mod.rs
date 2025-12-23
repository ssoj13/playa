//! Application module - PlayaApp and related functionality.
//!
//! This module organizes the main application logic into focused submodules:
//! - `events` - Event handling (handle_events, handle_effect_actions, handle_keyboard_input)
//! - `api` - REST API server and commands
//! - `project_io` - Project/sequence loading and saving

mod api;
mod events;
mod layout;
mod project_io;
mod run;
mod tabs;

pub use tabs::DockTabs;

use crate::config;
use crate::core::cache_man::CacheManager;
use crate::core::event_bus::{CompEventEmitter, EventBus};
use crate::core::player::Player;
use crate::core::workers::Workers;
use crate::core::DebouncedPreloader;
use crate::dialogs::encode::EncodeDialog;
use crate::dialogs::prefs::{AppSettings, HotkeyHandler};
use crate::dialogs::prefs::prefs_events::HotkeyWindow;
use crate::entities;
use crate::entities::{Frame, Project};
use crate::widgets::ae::AttributesState;
use crate::widgets::node_editor::NodeEditorState;
use crate::widgets::status::StatusBar;
use crate::widgets::viewport::{Shaders, ViewportRenderer, ViewportState};

use egui_dock::DockState;
use std::sync::Arc;
use uuid::Uuid;

/// Dock tab identifiers for the main UI layout.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum DockTab {
    Viewport,
    Timeline,
    Project,
    Attributes,
    NodeEditor,
}

/// Main application state.
///
/// Contains all runtime state for the Playa application including:
/// - Current frame and playback state
/// - Project data and cache management
/// - UI state (viewport, timeline, panels)
/// - Event bus for decoupled communication
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct PlayaApp {
    #[serde(skip)]
    pub frame: Option<Frame>,
    #[serde(skip)]
    pub player: Player,
    #[serde(skip)]
    pub error_msg: Option<String>,
    #[serde(skip)]
    pub status_bar: StatusBar,
    #[serde(skip)]
    pub viewport_renderer: Arc<std::sync::Mutex<ViewportRenderer>>,
    pub viewport_state: ViewportState,
    pub timeline_state: crate::widgets::timeline::TimelineState,
    #[serde(skip)]
    pub shader_manager: Shaders,
    /// Selected media item UUID in Project panel (persistent)
    pub selected_media_uuid: Option<Uuid>,
    #[serde(skip)]
    pub last_render_time_ms: f32,
    /// Last time cache stats were logged (for periodic logging)
    #[serde(skip)]
    pub last_stats_log_time: f64,
    pub settings: AppSettings,
    /// Persisted project (playlist)
    pub project: Project,
    #[serde(skip)]
    pub show_help: bool,
    #[serde(skip)]
    pub show_playlist: bool,
    #[serde(skip)]
    pub show_settings: bool,
    #[serde(skip)]
    pub show_encode_dialog: bool,
    #[serde(skip)]
    pub encode_dialog: Option<EncodeDialog>,
    #[serde(skip)]
    pub show_attributes_editor: bool,
    #[serde(skip)]
    pub is_fullscreen: bool,
    #[serde(skip)]
    pub fullscreen_dirty: bool,
    #[serde(skip)]
    pub reset_settings_pending: bool,
    #[serde(skip)]
    pub applied_mem_fraction: f64,
    #[serde(skip)]
    pub applied_cache_strategy: entities::CacheStrategy,
    #[serde(skip)]
    pub applied_workers: Option<usize>,
    #[serde(skip)]
    pub path_config: config::PathConfig,
    /// Global cache manager (memory tracking + epoch)
    #[serde(skip)]
    pub cache_manager: Arc<CacheManager>,
    /// Debounced preloader - delays full cache preload after attribute changes
    #[serde(skip)]
    pub debounced_preloader: DebouncedPreloader,
    /// Global worker pool for background tasks (frame loading, encoding)
    #[serde(skip)]
    pub workers: Arc<Workers>,
    /// Event emitter for compositions (shared across all comps)
    #[serde(skip)]
    pub comp_event_emitter: CompEventEmitter,
    /// Global event bus for application-wide events
    #[serde(skip)]
    pub event_bus: EventBus,
    #[serde(default = "PlayaApp::default_dock_state")]
    pub dock_state: DockState<DockTab>,
    /// Hotkey handler for context-aware keyboard shortcuts
    #[serde(skip)]
    pub hotkey_handler: HotkeyHandler,
    /// Currently focused window for input routing
    #[serde(skip)]
    pub focused_window: HotkeyWindow,
    /// Hover states for input routing
    #[serde(skip)]
    pub viewport_hovered: bool,
    #[serde(skip)]
    pub timeline_hovered: bool,
    #[serde(skip)]
    pub project_hovered: bool,
    #[serde(skip)]
    pub node_editor_hovered: bool,
    /// True when NodeEditor tab is the active/visible tab (for hotkey routing)
    #[serde(skip)]
    pub node_editor_tab_active: bool,
    /// Current selection focus for AE panel - last clicked entities
    #[serde(skip)]
    pub ae_focus: Vec<Uuid>,
    pub attributes_state: AttributesState,
    /// Node editor state (snarl graph for composition visualization)
    pub node_editor_state: NodeEditorState,
    /// Gizmo state for viewport transform manipulation
    #[serde(skip)]
    pub gizmo_state: crate::widgets::viewport::gizmo::GizmoState,
    /// REST API shared state (updated each frame for remote clients)
    #[serde(skip)]
    pub api_state: Arc<crate::server::SharedApiState>,
    /// REST API command receiver (polled each frame)
    #[serde(skip)]
    pub api_command_rx: Option<std::sync::mpsc::Receiver<crate::server::ApiCommand>>,
    /// Pending screenshot requests (full window capture via glReadPixels)
    /// Multiple clients can wait - all receive the same screenshot (broadcast)
    /// (viewport_only, response_channel) - viewport_only=true means full window, false means raw frame
    #[serde(skip)]
    pub pending_screenshots: Vec<(bool, crossbeam_channel::Sender<Result<Vec<u8>, String>>)>,
    /// Exit requested via REST API
    #[serde(skip)]
    pub exit_requested: bool,
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

        // Create global event bus and comp event emitter
        let event_bus = EventBus::new();
        let comp_event_emitter = CompEventEmitter::from_emitter(event_bus.emitter());

        Self {
            frame: None,
            player,
            error_msg: None,
            status_bar,
            viewport_renderer: Arc::new(std::sync::Mutex::new(ViewportRenderer::new())),
            viewport_state: ViewportState::new(),
            timeline_state: crate::widgets::timeline::TimelineState::default(),
            shader_manager: Shaders::new(),
            selected_media_uuid: None,
            last_render_time_ms: 0.0,
            last_stats_log_time: 0.0,
            settings: AppSettings::default(),
            project: {
                let settings = AppSettings::default();
                let mut project = Project::new_with_strategy(Arc::clone(&cache_manager), settings.cache_strategy);
                // Set event emitter for auto-emit of AttrsChangedEvent on comp modifications
                project.set_event_emitter(event_bus.emitter());
                project
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
            applied_cache_strategy: entities::CacheStrategy::All,
            applied_workers: None,
            path_config: config::PathConfig::from_env_and_cli(None),
            cache_manager,
            debounced_preloader: DebouncedPreloader::default(),
            workers,
            comp_event_emitter,
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
            node_editor_hovered: false,
            node_editor_tab_active: false,
            ae_focus: Vec::new(),
            attributes_state: AttributesState::default(),
            node_editor_state: NodeEditorState::new(),
            gizmo_state: crate::widgets::viewport::gizmo::GizmoState::default(),
            api_state: Arc::new(crate::server::SharedApiState::default()),
            api_command_rx: None, // Started later when settings are loaded
            pending_screenshots: Vec::new(),
            exit_requested: false,
        }
    }
}

impl PlayaApp {
    /// Default dock state with standard layout.
    pub fn default_dock_state() -> DockState<DockTab> {
        // Default layout with saved proportions (Project/Attributes split at 33%)
        Self::build_dock_state(true, true, 0.33)
    }

    /// Build dock state with configurable panels.
    pub fn build_dock_state(show_project: bool, show_attributes: bool, split_pos: f32) -> DockState<DockTab> {
        use egui_dock::NodeIndex;
        
        let mut dock_state = DockState::new(vec![DockTab::Viewport]);

        // Always split viewport and timeline vertically (timeline at bottom ~23%)
        // NodeEditor is a tab next to Timeline (same panel, tab switching)
        let [viewport, _timeline] = dock_state.main_surface_mut().split_below(
            NodeIndex::root(),
            0.77,
            vec![DockTab::Timeline, DockTab::NodeEditor],
        );

        if show_project || show_attributes {
            if show_project && show_attributes {
                // Both: create right panel with Project, then split it to add Attributes below
                let [_viewport, right_panel] = dock_state
                    .main_surface_mut()
                    .split_right(viewport, 0.77, vec![DockTab::Project]);

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
                    .split_right(viewport, 0.77, vec![DockTab::Project]);
            } else {
                // Only Attributes
                let _ = dock_state
                    .main_surface_mut()
                    .split_right(viewport, 0.77, vec![DockTab::Attributes]);
            }
        }

        dock_state
    }
}
