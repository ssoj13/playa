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
use playa_engine::core::DebouncedPreloader;
use playa_engine::core::cache_man::CacheManager;
use playa_engine::core::event_bus::{CompEventEmitter, EventBus};
use playa_engine::core::player::Player;
use playa_engine::core::workers::Workers;
use playa_engine::entities;
use playa_engine::entities::{Frame, GpuBlendBridge, GpuBlendRequest, Project, gpu_blend_arc_pair};
#[cfg(feature = "jobs")]
use playa_jobs::{JobQueue, JobQueueConfig};
use playa_ui::dialogs::encode::EncodeDialog;
use playa_ui::dialogs::prefs::prefs_events::HotkeyWindow;
use playa_ui::dialogs::prefs::{AppSettings, HotkeyHandler};
use playa_ui::widgets::ae::AttributesState;
use playa_ui::widgets::node_editor::NodeEditorState;
use playa_ui::widgets::status::StatusBar;
use playa_ui::widgets::viewport::{Shaders, ViewportRenderer, ViewportState};

use egui_dock::DockState;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc::Receiver;
use uuid::Uuid;

/// Dock tab identifiers for the main UI layout.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum DockTab {
    Viewport,
    Timeline,
    Project,
    Attributes,
    NodeEditor,
    /// Long-running jobs queue panel (Seedance video-gen, ffmpeg encodes,
    /// etc). Feature-gated under `jobs` (default on).
    #[cfg(feature = "jobs")]
    Jobs,
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
    pub timeline_state: playa_ui::widgets::timeline::TimelineState,
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
    pub gizmo_state: playa_ui::widgets::viewport::gizmo::GizmoState,
    /// REST API shared state (updated each frame for remote clients)
    #[serde(skip)]
    pub api_state: Arc<crate::server::SharedApiState>,
    /// REST API command receiver (polled each frame)
    #[serde(skip)]
    pub api_command_rx: Option<std::sync::mpsc::Receiver<crate::server::ApiCommand>>,
    /// Pending screenshot requests (broadcast via [`egui::ViewportCommand::Screenshot`] + CPU path for raw frame)
    /// Multiple clients can wait - all receive the same screenshot (broadcast)
    /// (viewport_only, response_channel) - viewport_only=true means full window, false means raw frame
    #[serde(skip)]
    pub pending_screenshots: Vec<(bool, crossbeam_channel::Sender<Result<Vec<u8>, String>>)>,
    /// Exit requested via REST API
    #[serde(skip)]
    pub exit_requested: bool,
    /// Last dark_mode value applied to egui visuals (avoids rebuilding Visuals every frame)
    #[serde(skip)]
    pub last_applied_dark_mode: Option<bool>,
    /// Last font_size value applied to egui style (avoids cloning style every frame)
    #[serde(skip)]
    pub last_applied_font_size: f32,
    /// Whether ctx.options_mut(max_passes) has been applied (one-time init)
    #[serde(skip)]
    pub options_initialized: bool,
    /// Send-side handle cloned into [`playa_engine::entities::ComputeContext`] when Gpu blending is enabled.
    ///
    /// Held as [`Option`] so deserialization can omit both bridge + receiver; [`Self::ensure_gpu_blend_initialized`]
    /// restores a fresh pair via [`gpu_blend_arc_pair`](playa_engine::entities::gpu_blend_arc_pair) when needed.
    #[serde(skip)]
    pub gpu_blend_bridge: Option<Arc<GpuBlendBridge>>,
    /// Main-thread ingest queue for [`GpuBlendBridge::drain_into_compositor`].
    ///
    /// Wrapped in [`Mutex`] so `update()` can temporarily borrow alongside `project.compositor`.
    /// `Mutex<Option<_>>::default()` is `Mutex::new(None)`, which is exactly the state
    /// [`PlayaApp::ensure_gpu_blend_initialized`] expects after deserialize.
    #[serde(skip)]
    pub gpu_blend_rx: Mutex<Option<Receiver<GpuBlendRequest>>>,
    /// Long-running IO job queue (Seedance video-gen, ffmpeg encodes that the
    /// user wants tracked outside the encode dialog, future media-import jobs).
    ///
    /// `None` immediately after deserialize; [`Self::ensure_jobs_initialized`]
    /// restores a fresh queue with the persistence log re-attached. Same
    /// pattern as [`Self::gpu_blend_bridge`].
    ///
    /// Feature-gated under `jobs` (default on) so callers building without
    /// jobs (`--no-default-features`) compile this struct cleanly.
    #[cfg(feature = "jobs")]
    #[serde(skip)]
    pub job_queue: Option<Arc<JobQueue>>,

    /// Per-frame state for the Jobs DockTab (sort, filter, selection).
    /// Ephemeral — not persisted across restarts.
    #[cfg(feature = "jobs")]
    #[serde(skip)]
    pub jobs_panel: playa_jobs::ui::JobsPanel,

    /// Modal state for the "Generate via Seedance…" dialog. Opened from
    /// the Jobs panel's `+ Generate` button. Ephemeral.
    #[cfg(feature = "jobs")]
    #[serde(skip)]
    pub submit_dialog: playa_jobs::ui::SubmitDialog,

    /// Atomic mirror of [`playa_jobs::JobsSettings::auto_attach_mp4`] so the
    /// `JobEvent::Completed` listener can read the live value without
    /// holding any borrow on `&self`. Synced from `settings.jobs` once per
    /// frame in `update()`.
    #[cfg(feature = "jobs")]
    #[serde(skip)]
    pub auto_attach_enabled: Arc<std::sync::atomic::AtomicBool>,

    /// Idempotency latch for [`Self::register_auto_attach_listener`]. Set
    /// the first time we subscribe to `JobEvent::Completed` so re-running
    /// `ensure_jobs_initialized` (or repeated boot paths) doesn't stack
    /// multiple listeners on the same `EventBus`.
    #[cfg(feature = "jobs")]
    #[serde(skip)]
    pub auto_attach_subscribed: bool,

    /// Producer end of the auto-attach hand-off. The listener captures a
    /// clone and pushes resolved mp4 paths through it from whatever thread
    /// the `EventBus` invokes callbacks on. `update()` drains the receiver
    /// on the UI thread (where `Project` mutators are safe) and routes the
    /// paths into [`Self::load_sequences`]. `None` until
    /// `register_auto_attach_listener` builds the channel.
    #[cfg(feature = "jobs")]
    #[serde(skip)]
    pub auto_attach_tx: Option<std::sync::mpsc::Sender<std::path::PathBuf>>,

    /// Consumer end of the auto-attach hand-off. `Mutex<Option<...>>` so
    /// it survives serde reload (default = `None`) and `register_…` can
    /// install a fresh receiver after deserialize without re-implementing
    /// `Default`. Drained per-frame in `update()`.
    #[cfg(feature = "jobs")]
    #[serde(skip)]
    pub auto_attach_rx: Mutex<Option<std::sync::mpsc::Receiver<std::path::PathBuf>>>,

    /// Pluggable preferences registry. Each module exposes a `pub fn render`
    /// for its slice of settings; the host registers an entry that calls
    /// that fn with `&mut SliceSettings` extracted from `AppSettings`.
    /// Built in [`Self::Default`] with the jobs entry pre-registered when
    /// the `jobs` feature is on.
    ///
    /// Generic over `AppSettings` from playa-ui for slice extraction.
    #[serde(skip)]
    pub prefs_registry: playa_prefs::PrefsRegistry<AppSettings>,

    /// Modal preferences window state machine. `Ctrl+,` opens it with a
    /// clone of `self.settings` as the working copy; Apply commits the
    /// working copy back, Cancel discards.
    #[serde(skip)]
    pub prefs_window: playa_prefs::PrefsWindow<AppSettings>,
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

        let (gpu_blend_bridge, gpu_blend_rx) = gpu_blend_arc_pair();

        #[cfg(feature = "jobs")]
        let job_queue = build_default_job_queue(Arc::new(event_bus.clone()));

        Self {
            frame: None,
            player,
            error_msg: None,
            status_bar,
            viewport_renderer: Arc::new(std::sync::Mutex::new(ViewportRenderer::new())),
            viewport_state: ViewportState::new(),
            timeline_state: playa_ui::widgets::timeline::TimelineState::default(),
            shader_manager: Shaders::new(),
            selected_media_uuid: None,
            last_render_time_ms: 0.0,
            last_stats_log_time: 0.0,
            settings: AppSettings::default(),
            project: {
                let settings = AppSettings::default();
                let mut project =
                    Project::new_with_strategy(Arc::clone(&cache_manager), settings.cache_strategy);
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
            gizmo_state: playa_ui::widgets::viewport::gizmo::GizmoState::default(),
            api_state: Arc::new(crate::server::SharedApiState::default()),
            api_command_rx: None, // Started later when settings are loaded
            pending_screenshots: Vec::new(),
            exit_requested: false,
            last_applied_dark_mode: None,
            last_applied_font_size: 0.0,
            options_initialized: false,
            gpu_blend_bridge: Some(gpu_blend_bridge),
            gpu_blend_rx: Mutex::new(Some(gpu_blend_rx)),
            #[cfg(feature = "jobs")]
            job_queue,
            #[cfg(feature = "jobs")]
            jobs_panel: playa_jobs::ui::JobsPanel::new(),
            #[cfg(feature = "jobs")]
            submit_dialog: playa_jobs::ui::SubmitDialog::default(),
            #[cfg(feature = "jobs")]
            auto_attach_enabled: Arc::new(std::sync::atomic::AtomicBool::new(
                playa_jobs::JobsSettings::default().auto_attach_mp4,
            )),
            #[cfg(feature = "jobs")]
            auto_attach_subscribed: false,
            #[cfg(feature = "jobs")]
            auto_attach_tx: None,
            #[cfg(feature = "jobs")]
            auto_attach_rx: Mutex::new(None),
            prefs_registry: {
                let mut registry = playa_prefs::PrefsRegistry::<AppSettings>::new();
                #[cfg(feature = "jobs")]
                playa_jobs::register_default_prefs::<AppSettings>(
                    &mut registry,
                    |s: &mut AppSettings| &mut s.jobs,
                );
                registry
            },
            prefs_window: playa_prefs::PrefsWindow::<AppSettings>::new(),
        }
    }
}

/// Construct the application-wide [`JobQueue`] using OS-standard config /
/// cache directories. Persistence is on by default (matches the user's Q3
/// answer); a write failure on the persist log demotes to non-persistent so
/// the rest of the app still boots.
#[cfg(feature = "jobs")]
fn build_default_job_queue(event_bus: Arc<playa_jobs::EventBus>) -> Option<Arc<JobQueue>> {
    use playa_jobs::JobEvent;

    let persist_path = dirs_next::config_dir().map(|d| d.join("playa").join("jobs.jsonl"));
    let files_dir = dirs_next::cache_dir()
        .map(|d| d.join("playa").join("jobs"))
        .unwrap_or_else(|| std::env::temp_dir().join("playa-jobs"));

    let cfg = JobQueueConfig {
        thread_count: std::thread::available_parallelism()
            .map(|n| n.get() / 4)
            .unwrap_or(2)
            .max(2),
        files_dir,
        persist_path: persist_path.clone(),
    };

    let queue = match JobQueue::new(cfg, Arc::clone(&event_bus)) {
        Ok(q) => q,
        Err(e) => {
            log::warn!(
                "JobQueue: failed to open persist log at {:?} ({}); falling back to non-persistent queue",
                persist_path,
                e
            );
            let fallback = JobQueueConfig {
                thread_count: 2,
                files_dir: std::env::temp_dir().join("playa-jobs-fallback"),
                persist_path: None,
            };
            match JobQueue::new(fallback, Arc::clone(&event_bus)) {
                Ok(q) => q,
                Err(e) => {
                    log::error!("JobQueue: fallback init also failed ({e}); disabling jobs");
                    return None;
                }
            }
        }
    };

    // Default visibility: log every event at debug level so dev sessions see
    // job activity without any UI hookup. Subscribed through the EventBus —
    // any other consumer (status bar, jobs panel) does the same.
    event_bus.subscribe::<JobEvent, _>(|event| log::debug!("[jobs] {event:?}"));

    // Register the Seedance provider iff a key is present. Lookup order:
    //   PLAYA_FAL_KEY env > FAL_KEY env > .env file in CWD or one parent.
    register_seedance_provider(&queue);

    Some(Arc::new(queue))
}

/// Lookup precedence for the fal.ai API key. Three names because each
/// ecosystem labels it differently:
/// - `PLAYA_FAL_KEY` — playa-namespaced override.
/// - `FAL_KEY` — fal.ai docs canonical (`Authorization: Key <FAL_KEY>`).
/// - `FAL_API_KEY` — name used by the fal JS/Python SDKs and visible in many
///   `.env` examples.
#[cfg(feature = "jobs")]
const FAL_KEY_NAMES: &[&str] = &["PLAYA_FAL_KEY", "FAL_KEY", "FAL_API_KEY"];

#[cfg(feature = "jobs")]
fn read_fal_key() -> Option<String> {
    let cwd_env = std::path::PathBuf::from(".env");
    let parent_env = std::path::PathBuf::from("../.env");
    playa_jobs::secret::lookup(FAL_KEY_NAMES, &[cwd_env, parent_env])
}

#[cfg(feature = "jobs")]
fn register_seedance_provider(queue: &JobQueue) {
    let Some(key) = read_fal_key() else {
        log::info!(
            "Seedance providers NOT registered: set FAL_KEY (or PLAYA_FAL_KEY / FAL_API_KEY) env var or place it in a .env file at the repo root"
        );
        return;
    };
    // Register both fal.ai Seedance endpoints. `kind()` distinguishes them in
    // `JobQueue::submit`. Same key serves both.
    queue.register_provider(playa_jobs::seedance::SeedanceProvider::image_to_video(key.clone()));
    queue.register_provider(playa_jobs::seedance::SeedanceProvider::text_to_video(key));
    log::info!(
        "Seedance providers registered (kinds=`{}`, `{}`)",
        playa_jobs::seedance::kinds::IMAGE_TO_VIDEO,
        playa_jobs::seedance::kinds::TEXT_TO_VIDEO
    );
}

impl PlayaApp {
    /// Recreates a fresh [`GpuBlendBridge`](playa_engine::entities::GpuBlendBridge)/[`Receiver`] pair when serde skipped runtime wiring.
    ///
    /// Persisted layouts omit channel state; deserialization therefore leaves handles empty unless we
    /// rebuild them **before** worker threads enqueue [`GpuBlendRequest`](playa_engine::entities::GpuBlendRequest)s again.
    /// Otherwise callers observe repeated [`GpuBlendReport::NotQueued`](playa_engine::entities::GpuBlendReport::NotQueued) fallbacks (`comp_node`).
    ///
    /// [`Receiver`]: std::sync::mpsc::Receiver
    /// Recreate the [`JobQueue`] when serde dropped the runtime handle on
    /// load. Mirrors [`Self::ensure_gpu_blend_initialized`]: idempotent if
    /// `job_queue` is already `Some`. Boot path (`runner.rs`) calls this
    /// before any provider registration so [`JobQueue::replay_persisted`]
    /// resumes orphaned jobs once providers are registered.
    #[cfg(feature = "jobs")]
    pub fn ensure_jobs_initialized(&mut self) {
        if self.job_queue.is_none() {
            self.job_queue = build_default_job_queue(Arc::new(self.event_bus.clone()));
        }
        // Subscribe AFTER queue init so the EventBus is the same handle
        // the queue emits through. Idempotent — see auto_attach_subscribed
        // doc.
        self.register_auto_attach_listener();
    }

    /// Subscribe a `JobEvent::Completed` listener that auto-attaches the
    /// completed mp4 as a layer when the user has the setting enabled. The
    /// flag is read AT EVENT TIME via [`Self::auto_attach_enabled`] so
    /// toggling the preference takes effect for future jobs without
    /// re-subscribing.
    ///
    /// Threading: `EventBus` callbacks may fire on any worker thread, and
    /// `Project` mutators expect the UI thread. We hand the path through
    /// an `mpsc` channel; `update()` drains it and calls
    /// [`Self::load_sequences`] (same canonical import path as drag-drop).
    ///
    /// Idempotent: a latch (`auto_attach_subscribed`) prevents stacking
    /// listeners on repeated calls. The channel is rebuilt fresh inside
    /// this method so a post-deserialize re-init gets a working pair.
    #[cfg(feature = "jobs")]
    fn register_auto_attach_listener(&mut self) {
        if self.auto_attach_subscribed {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel::<std::path::PathBuf>();
        self.auto_attach_tx = Some(tx.clone());
        *self
            .auto_attach_rx
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(rx);

        let flag = Arc::clone(&self.auto_attach_enabled);
        self.event_bus
            .subscribe::<playa_jobs::JobEvent, _>(move |event| {
                if let playa_jobs::JobEvent::Completed(id, value) = event
                    && flag.load(std::sync::atomic::Ordering::Relaxed)
                    && let Some(path) = value.get("mp4_path").and_then(|v| v.as_str())
                {
                    let pb = std::path::PathBuf::from(path);
                    log::info!("auto-attach: job {id} → queuing mp4 import: {pb:?}");
                    // Receiver dropped means the app is shutting down; the
                    // send error is non-fatal.
                    let _ = tx.send(pb);
                }
            });
        self.auto_attach_subscribed = true;
    }

    /// Drain the auto-attach receiver and route any queued mp4 paths into
    /// `load_sequences`. Called once per frame from `update()`. Same
    /// import path that drag-drop uses (`FileNode::detect_from_paths` →
    /// add to media pool → activate as first sequence if no active comp).
    #[cfg(feature = "jobs")]
    pub fn drain_auto_attach_queue(&mut self) {
        let paths: Vec<std::path::PathBuf> = {
            let guard = self
                .auto_attach_rx
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let Some(rx) = guard.as_ref() else {
                return;
            };
            std::iter::from_fn(|| rx.try_recv().ok()).collect()
        };
        if paths.is_empty() {
            return;
        }
        log::info!("auto-attach: importing {} mp4 path(s)", paths.len());
        if let Err(e) = self.load_sequences(paths) {
            log::warn!("auto-attach: load_sequences failed: {e}");
        }
    }

    /// Render the Jobs dock tab content. Pulls from `self.job_queue` if
    /// present; otherwise shows a "jobs disabled" hint. Dispatches the
    /// returned [`playa_jobs::ui::JobsAction`] to queue methods.
    #[cfg(feature = "jobs")]
    pub fn render_jobs_tab(&mut self, ui: &mut eframe::egui::Ui) {
        use playa_jobs::ui::JobsAction;

        let Some(queue) = self.job_queue.as_ref().map(Arc::clone) else {
            ui.centered_and_justified(|ui| {
                ui.weak("Job queue not initialized.");
            });
            return;
        };

        // Inline jobs prefs as a collapsing header above the table.
        // Mirrors what the central Preferences modal (Ctrl+,) shows — kept
        // here as a quick edit path so users don't have to leave the tab
        // to flip auto-attach. Same `&mut self.settings.jobs`, so changes
        // reflect immediately in both surfaces.
        ui.collapsing("⚙ Settings", |ui| {
            playa_jobs::ui::prefs::render(ui, &mut self.settings.jobs);
        });
        ui.separator();

        match self.jobs_panel.ui(ui, &queue) {
            JobsAction::None => {}
            JobsAction::Cancel(ids) => {
                for id in ids {
                    queue.cancel(id);
                }
            }
            JobsAction::Retry(ids) => {
                for id in ids {
                    if let Err(e) = queue.retry(id) {
                        log::warn!("retry({id}) failed: {e}");
                    }
                }
            }
            JobsAction::Delete(ids) => {
                for id in ids {
                    if let Err(e) = queue.remove(id) {
                        log::warn!("remove({id}) failed: {e}");
                    }
                }
            }
            JobsAction::RevealMp4(id) => {
                // Open the containing directory in the platform's file
                // manager. `opener::open` does the right thing per-OS:
                // Explorer on Windows, Finder on macOS, xdg-open on
                // Linux. We open the parent rather than the file itself
                // so the user gets a folder view (not the default media
                // player). Falls back to logging if open fails so the
                // user still has the path.
                if let Some(j) = queue.get(id)
                    && let Some(path) = j
                        .result
                        .as_ref()
                        .and_then(|v| v.get("mp4_path"))
                        .and_then(|v| v.as_str())
                {
                    let p = std::path::Path::new(path);
                    let target = p.parent().unwrap_or(p);
                    match opener::open(target) {
                        Ok(()) => log::info!("Reveal mp4: {path}"),
                        Err(e) => log::warn!(
                            "Reveal mp4 failed ({e}); path is {path}"
                        ),
                    }
                }
            }
            JobsAction::OpenSubmit => {
                self.submit_dialog.open();
            }
        }
    }

    pub fn ensure_gpu_blend_initialized(&mut self) {
        if self.gpu_blend_bridge.is_some()
            && self
                .gpu_blend_rx
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_some()
        {
            return;
        }
        let (b, rx) = gpu_blend_arc_pair();
        self.gpu_blend_bridge = Some(b);
        *self
            .gpu_blend_rx
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(rx);
    }

    /// Returns `Some` only when the project selects the Gpu backend ([`CompositorType`](playa_engine::entities::CompositorType)).
    ///
    /// Cpu compositor ⇒ `None`, which keeps preload-time [`ComputeContext`](playa_engine::entities::ComputeContext)
    /// off the Gpu offload path (matches encode/worker-safe defaults elsewhere).
    pub(crate) fn gpu_blend_bridge_ref_for_preload(&self) -> Option<&GpuBlendBridge> {
        use entities::CompositorType;
        let is_gpu = matches!(
            *self
                .project
                .compositor
                .lock()
                .unwrap_or_else(|e| e.into_inner()),
            CompositorType::Wgpu(_)
        );
        if !is_gpu {
            return None;
        }
        self.gpu_blend_bridge.as_deref()
    }

    /// Batches [`GpuBlendBridge::drain_into_compositor`](playa_engine::entities::GpuBlendBridge::drain_into_compositor)
    /// **after** `update_compositor_backend` so Gpu resources match the current wgpu device/queue wiring.
    ///
    /// Returning `usize > 0` triggers `request_repaint` because workers flushed pixels asynchronously
    /// and egui otherwise idles until the next input tick.
    pub(crate) fn drain_gpu_blend_queue(&mut self, ctx: &eframe::egui::Context) {
        let rx_guard = self
            .gpu_blend_rx
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let Some(rx) = rx_guard.as_ref() else {
            return;
        };
        let mut comp = self
            .project
            .compositor
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let n = GpuBlendBridge::drain_into_compositor(rx, &mut comp);
        if n > 0 {
            ctx.request_repaint();
        }
    }

    /// Default dock state with standard layout.
    pub fn default_dock_state() -> DockState<DockTab> {
        // Default layout with saved proportions (Project/Attributes split at 33%)
        Self::build_dock_state(true, true, 0.33)
    }

    /// Build dock state with configurable panels.
    pub fn build_dock_state(
        show_project: bool,
        show_attributes: bool,
        split_pos: f32,
    ) -> DockState<DockTab> {
        use egui_dock::NodeIndex;

        let mut dock_state = DockState::new(vec![DockTab::Viewport]);

        // Always split viewport and timeline vertically (timeline at bottom ~23%)
        // NodeEditor is a tab next to Timeline (same panel, tab switching);
        // when the `jobs` feature is on, Jobs is a third tab in that bottom
        // panel so it shares the timeline strip without claiming new screen
        // real estate.
        #[cfg(feature = "jobs")]
        let bottom_tabs = vec![DockTab::Timeline, DockTab::NodeEditor, DockTab::Jobs];
        #[cfg(not(feature = "jobs"))]
        let bottom_tabs = vec![DockTab::Timeline, DockTab::NodeEditor];
        let [viewport, _timeline] =
            dock_state
                .main_surface_mut()
                .split_below(NodeIndex::root(), 0.77, bottom_tabs);

        if show_project || show_attributes {
            if show_project && show_attributes {
                // Both: create right panel with Project, then split it to add Attributes below
                let [_viewport, right_panel] = dock_state.main_surface_mut().split_right(
                    viewport,
                    0.77,
                    vec![DockTab::Project],
                );

                // Split right panel vertically: Project stays on top, Attributes below
                // Use saved split position
                let _ = dock_state.main_surface_mut().split_below(
                    right_panel,
                    split_pos,
                    vec![DockTab::Attributes],
                );
            } else if show_project {
                // Only Project
                let _ = dock_state.main_surface_mut().split_right(
                    viewport,
                    0.77,
                    vec![DockTab::Project],
                );
            } else {
                // Only Attributes
                let _ = dock_state.main_surface_mut().split_right(
                    viewport,
                    0.77,
                    vec![DockTab::Attributes],
                );
            }
        }

        dock_state
    }
}
