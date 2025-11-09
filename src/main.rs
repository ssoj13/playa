mod frame;
mod exr;
mod video;
mod sequence;
mod progress;
mod player;
mod cache;
mod scrub;
mod viewport;
mod shaders;
mod timeslider;
mod status_bar;
mod progress_bar;
mod ui;
mod prefs;
mod paths;
mod utils;

use clap::Parser;
use eframe::{egui, glow};
use frame::Frame;
use log::{debug, error, info, warn};
use player::Player;
use prefs::{AppSettings, render_settings_window};
use scrub::Scrubber;
use sequence::Sequence;
use status_bar::StatusBar;
use std::path::PathBuf;
use shaders::Shaders;
use viewport::{ViewportRenderer, ViewportState};

/// Image sequence player for VFX workflows
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the image file to load (EXR, PNG, JPEG, TIFF, TGA) - optional, can also drag-and-drop
    #[arg(value_name = "FILE")]
    file_path: Option<PathBuf>,

    /// Enable debug logging to file (default: playa.log)
    #[arg(short = 'l', long = "log", value_name = "LOG_FILE")]
    log_file: Option<Option<PathBuf>>,

    /// Custom configuration directory (overrides default platform paths)
    #[arg(short = 'c', long = "config-dir", value_name = "DIR")]
    config_dir: Option<PathBuf>,

    /// Memory budget percentage for cache (e.g., 75 for 75%)
    #[arg(long = "mem", value_name = "PERCENT")]
    mem_percent: Option<f64>,

    /// Worker threads override (default: 75% of CPU cores)
    #[arg(long = "workers", value_name = "N")]
    workers: Option<usize>,
}

/// Main application state
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)]
struct PlayaApp {
    #[serde(skip)]
    frame: Option<Frame>,
    #[serde(skip)]
    displayed_frame: Option<usize>,
    #[serde(skip)]
    player: Player,
    #[serde(skip)]
    error_msg: Option<String>,
    #[serde(skip)]
    scrubber: Option<Scrubber>,
    #[serde(skip)]
    status_bar: StatusBar,
    #[serde(skip)]
    viewport_renderer: std::sync::Arc<std::sync::Mutex<ViewportRenderer>>,
    viewport_state: ViewportState,
    #[serde(skip)]
    shader_manager: Shaders,
    #[serde(skip)]
    last_render_time_ms: f32,
    settings: AppSettings,
    #[serde(skip)]
    show_help: bool,
    #[serde(skip)]
    show_playlist: bool,
    #[serde(skip)]
    show_settings: bool,
    #[serde(skip)]
    is_fullscreen: bool,
    #[serde(skip)]
    cached_seq_ranges: Vec<timeslider::SequenceRange>,
    #[serde(skip)]
    last_seq_version: usize,
    #[serde(skip)]
    applied_mem_fraction: f64,
    #[serde(skip)]
    applied_workers: Option<usize>,
    #[serde(skip)]
    path_config: paths::PathConfig,
}

impl Default for PlayaApp {
    fn default() -> Self {
        let (player, ui_rx) = Player::new();
        let status_bar = StatusBar::new(ui_rx);

        Self {
            frame: None,
            displayed_frame: None,
            player,
            error_msg: None,
            scrubber: Some(Scrubber::new()),
            status_bar,
            viewport_renderer: std::sync::Arc::new(std::sync::Mutex::new(ViewportRenderer::new())),
            viewport_state: ViewportState::new(),
            shader_manager: Shaders::new(),
            last_render_time_ms: 0.0,
            settings: AppSettings::default(),
            show_help: true,
            show_playlist: true,
            show_settings: false,
            is_fullscreen: false,
            cached_seq_ranges: Vec::new(),
            last_seq_version: 0,
            applied_mem_fraction: 0.75,
            applied_workers: None,
            path_config: paths::PathConfig::from_env_and_cli(None),
        }
    }
}

impl PlayaApp {
    /// Enable or disable "cinema mode": borderless fullscreen, hidden UI, black background.
    fn set_cinema_mode(&mut self, ctx: &egui::Context, enabled: bool) {
        self.is_fullscreen = enabled;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(enabled));
        // Hide window decorations in cinema mode for a cleaner look
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(!enabled));
        // Request repaint to immediately reflect UI visibility/background changes
        ctx.request_repaint();
    }
    /// Save playlist to JSON file
    fn save_playlist(&mut self, path: PathBuf) {
        if let Err(e) = self.player.cache.to_json(&path) {
            error!("{}", e);
            self.error_msg = Some(e);
        }
    }

    /// Load playlist from JSON file (append=true by default)
    fn load_playlist(&mut self, path: PathBuf) {
        match self.player.cache.from_json(&path, true) {
            Ok(count) => {
                info!("Added {} sequence(s) from playlist", count);

                // Get current frame for display
                let current_frame_idx = self.player.current_frame();
                if let Some(frame) = self.player.get_current_frame() {
                    self.frame = Some(frame.clone());
                    self.displayed_frame = Some(current_frame_idx);

                    let (width, height) = frame.resolution();
                    self.viewport_state.image_size = egui::Vec2::new(width as f32, height as f32);

                    // Trigger background preload
                    self.player.cache.signal_preload();
                }
            }
            Err(e) => {
                error!("{}", e);
                self.error_msg = Some(e);
            }
        }
    }

    fn handle_keyboard_input(&mut self, ctx: &egui::Context) {
        let input = ctx.input(|i| i.clone());

        if input.key_pressed(egui::Key::F1) {
            self.show_help = !self.show_help;
        }

        if input.key_pressed(egui::Key::F2) {
            self.show_playlist = !self.show_playlist;
        }

        if input.key_pressed(egui::Key::F3) {
            self.show_settings = !self.show_settings;
        }

        // ESC/Q: one handler. ESC leaves cinema/fullscreen first; Q always quits.
        if input.key_pressed(egui::Key::Escape) || input.key_pressed(egui::Key::Q) {
            if input.key_pressed(egui::Key::Escape) && self.is_fullscreen {
                self.set_cinema_mode(ctx, false);
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }

        // Play/Pause
        if input.key_pressed(egui::Key::Space) {
            self.player.toggle_play_pause();
        }

        // Rewind to start
        if input.key_pressed(egui::Key::ArrowUp) {
            self.player.to_start();
        }

        // J, <, Left Arrow - jog backward
        if input.key_pressed(egui::Key::J)
            || (!input.modifiers.ctrl && input.key_pressed(egui::Key::ArrowLeft))
            || input.key_pressed(egui::Key::Comma)
        {
            self.player.jog_backward();
        }

        // K, Down Arrow - stop playback or decrease fps
        if input.key_pressed(egui::Key::K) || input.key_pressed(egui::Key::ArrowDown) {
            self.player.stop_or_decrease_fps();
        }

        // L, >, Right Arrow - jog forward
        if input.key_pressed(egui::Key::L)
            || (!input.modifiers.ctrl && input.key_pressed(egui::Key::ArrowRight))
            || input.key_pressed(egui::Key::Period)
        {
            self.player.jog_forward();
        }

        // Toggle Loop with ' and `
        if input.key_pressed(egui::Key::Quote) || input.key_pressed(egui::Key::Backtick) {
            self.player.loop_enabled = !self.player.loop_enabled;
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
            if self.is_fullscreen { self.set_cinema_mode(ctx, false); }
        }

        // Z: toggle cinema/fullscreen
        if input.key_pressed(egui::Key::Z) {
            let enable = !self.is_fullscreen;
            self.set_cinema_mode(ctx, enable);
        }

        // Viewport controls
        if input.key_pressed(egui::Key::F) {
            self.viewport_state.set_mode_fit();
        }

        if input.key_pressed(egui::Key::A) || input.key_pressed(egui::Key::Num1)
            || input.key_pressed(egui::Key::Home)
            || input.key_pressed(egui::Key::H)
        {
            self.viewport_state.set_mode_100();
        }
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
            self.viewport_state.set_image_size(egui::vec2(width as f32, height as f32));
            self.viewport_state.set_mode_fit();
        }
    }
}

impl eframe::App for PlayaApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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

        self.handle_keyboard_input(ctx);

        // Apply live cache memory budget from settings if changed
        let desired_mem_fraction = (self.settings.cache_mem_percent as f64 / 100.0).clamp(0.05, 0.95);
        if (desired_mem_fraction - self.applied_mem_fraction).abs() > f64::EPSILON {
            self.player.cache.set_memory_fraction(desired_mem_fraction);
            self.applied_mem_fraction = desired_mem_fraction;
        }
        self.player.update();

        // Process loaded frames from worker threads (updates cache and sends progress to UI)
        self.player.cache.process_loaded_frames();

        // Handle drag-and-drop files/folders - queue for async loading
        ctx.input(|i| {
            let mut dropped: Vec<std::path::PathBuf> = Vec::new();
            for file in &i.raw.dropped_files {
                if let Some(path) = &file.path { dropped.push(path.clone()); }
            }
            if !dropped.is_empty() {
                info!("Files dropped: {:?}", dropped);
                for path in dropped {
                    // Validate and load sequence directly
                    match Sequence::detect(vec![path.clone()]) {
                        Ok(sequences) => {
                            for seq in sequences {
                                self.player.cache.append_seq(seq);
                            }
                        }
                        Err(e) => {
                            warn!("Failed to load {}: {}", path.display(), e);
                        }
                    }
                }
            }
        });

        if self.player.is_playing {
            ctx.request_repaint();
        }

        // Determine if the texture needs to be re-uploaded by checking if the frame has changed
        let texture_needs_upload = self.displayed_frame != Some(self.player.current_frame());

        // If the frame has changed, update our cached frame
        if texture_needs_upload {
            self.frame = self.player.get_current_frame().cloned();
            self.displayed_frame = Some(self.player.current_frame());
        }

        // Update status messages BEFORE laying out panels
        self.status_bar.update(ctx);

        // Playlist panel on the right (hidden in cinema mode or when toggled off)
        if !self.is_fullscreen && self.show_playlist {
            let playlist_actions = ui::render_playlist(ctx, &mut self.player);
            if let Some(path) = playlist_actions.load_sequence {
                // Validate and load sequence directly
                match Sequence::detect(vec![path.clone()]) {
                    Ok(sequences) => {
                        for seq in sequences {
                            self.player.cache.append_seq(seq);
                        }
                    }
                    Err(e) => {
                        warn!("Failed to load {}: {}", path.display(), e);
                    }
                }
            }
            if playlist_actions.clear_all {
                self.frame = None;
                self.displayed_frame = None;
                let (player, ui_rx) = Player::new();
                self.player = player;
                self.status_bar = StatusBar::new(ui_rx);
            }
            if let Some(path) = playlist_actions.save_playlist {
                self.save_playlist(path);
            }
            if let Some(path) = playlist_actions.load_playlist {
                self.load_playlist(path);
            }
        }

                if !self.is_fullscreen {
            let shader_changed = ui::render_controls(
                ctx,
                &mut self.player,
                &mut self.shader_manager,
                &mut self.cached_seq_ranges,
                &mut self.last_seq_version,
            );
            if shader_changed {
                let mut renderer = self.viewport_renderer.lock().unwrap();
                renderer.update_shader(&self.shader_manager);
                log::info!("Shader changed to: {}", self.shader_manager.current_shader);
            }
        }

                if !self.is_fullscreen {
            self.status_bar.render(
                ctx,
                self.frame.as_ref(),
                &self.player,
                &self.viewport_state,
                self.last_render_time_ms,
            );
        }

        // Render viewport (central panel)
        let (viewport_actions, render_time) = ui::render_viewport(
            ctx,
            self.frame.as_ref(),
            self.error_msg.as_ref(),
            &mut self.player,
            &mut self.viewport_state,
            &self.viewport_renderer,
            &mut self.scrubber,
            self.show_help,
            self.is_fullscreen,
            texture_needs_upload,
        );
        self.last_render_time_ms = render_time;
        if let Some(path) = viewport_actions.load_sequence {
            // Validate and load sequence directly
            match Sequence::detect(vec![path.clone()]) {
                Ok(sequences) => {
                    for seq in sequences {
                        self.player.cache.append_seq(seq);
                    }
                }
                Err(e) => {
                    warn!("Failed to load {}: {}", path.display(), e);
                }
            }
        }

        
        
        
        // Settings window (can be shown even in cinema mode)
        if self.show_settings {
            render_settings_window(ctx, &mut self.show_settings, &mut self.settings);
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        // Gather all settings from components
        self.settings.fps = self.player.fps;
        self.settings.loop_enabled = self.player.loop_enabled;
        self.settings.current_shader = self.shader_manager.current_shader.clone();
        self.settings.show_help = self.show_help;
        self.settings.show_playlist = self.show_playlist;

        // Save cache state separately (sequences + current frame)
        let cache_path = paths::data_file("playa_cache.json", &self.path_config);

        if let Err(e) = self.player.cache.to_json(&cache_path) {
            warn!("Failed to save cache state: {}", e);
        }

        // Serialize and save app settings
        if let Ok(json) = serde_json::to_string(self) {
            storage.set_string(eframe::APP_KEY, json);
            debug!("App state saved: FPS={}, Loop={}, Shader={}",
                   self.settings.fps, self.settings.loop_enabled,
                   self.settings.current_shader);
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

    // Create path configuration from CLI args and environment
    let path_config = paths::PathConfig::from_env_and_cli(args.config_dir.clone());

    // Ensure directories exist
    if let Err(e) = paths::ensure_dirs(&path_config) {
        eprintln!("Warning: Failed to create application directories: {}", e);
    }

    // Initialize logger based on --log flag
    if let Some(log_path_opt) = &args.log_file {
        // File logging with debug level
        let log_path = log_path_opt.as_ref()
            .map(|p| p.clone())
            .unwrap_or_else(|| paths::data_file("playa.log", &path_config));

        let file = std::fs::File::create(&log_path)
            .expect("Failed to create log file");

        env_logger::Builder::new()
            .filter_level(log::LevelFilter::Debug)
            .format_timestamp_millis()
            .target(env_logger::Target::Pipe(Box::new(file)))
            .init();

        info!("Debug logging enabled to file: {}", log_path.display());
    } else {
        // Normal console logging (set RUST_LOG env var to control level)
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
            .format_timestamp_millis()
            .init();
    }

    info!("Playa Image Sequence Player starting...");
    debug!("Command-line args: {:?}", args);

    // Log application paths
    info!("Config path: {}", paths::config_file("playa.json", &path_config).display());
    info!("Data path: {}", paths::data_file("playa_cache.json", &path_config).parent().unwrap().display());

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
            .with_title(&format!("Playa v{} • {} • F1 for help",
                env!("CARGO_PKG_VERSION"), BACKEND))
            .with_resizable(true)
            .with_drag_and_drop(true),
        persist_window: true,
        #[cfg(not(target_arch = "wasm32"))]
        persistence_path: Some(paths::config_file("playa.json", &path_config)),
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
            let mut app = cc.storage
                .and_then(|storage| storage.get_string(eframe::APP_KEY))
                .and_then(|json| serde_json::from_str(&json).ok())
                .unwrap_or_else(|| {
                    info!("No persisted state found, creating default app");
                    PlayaApp::default()
                });

            // Recreate Player with CLI- or Settings-configured cache memory/worker settings
            // and rewire status bar + path sender
            let mem_fraction = args.mem_percent
                .map(|p| (p / 100.0).clamp(0.05, 0.95))
                .or_else(|| Some((app.settings.cache_mem_percent as f64 / 100.0).clamp(0.05, 0.95)))
                .unwrap_or(0.75);
            let workers = args.workers
                .or_else(|| if app.settings.workers_override > 0 { Some(app.settings.workers_override as usize) } else { None });
            let (player, ui_rx) = Player::new_with_config(mem_fraction, workers);
            app.player = player;
            app.status_bar = StatusBar::new(ui_rx);
            app.applied_mem_fraction = mem_fraction;
            app.applied_workers = workers;
            app.path_config = path_config_for_app;

            // Attempt to load shaders from the shaders directory
            if let Err(e) = app.shader_manager.load_shader_directory(&std::path::PathBuf::from("shaders")) {
                log::warn!("Could not load shader directory: {}", e);
                log::info!("Using default built-in shaders");
            }

            // Apply persisted settings to components
            app.player.fps = app.settings.fps;
            app.player.loop_enabled = app.settings.loop_enabled;
            app.shader_manager.current_shader = app.settings.current_shader.clone();
            app.show_help = app.settings.show_help;
            info!("Applied settings: FPS={}, Loop={}, Shader={}, Help={}",
                  app.settings.fps, app.settings.loop_enabled,
                  app.settings.current_shader, app.show_help);

            // Fast cache restoration (sequences + current frame)
            let cache_path = paths::data_file("playa_cache.json", &path_config);

            // CLI argument has priority
            if let Some(file_path) = args.file_path {
                info!("CLI argument provided, loading sequence");
                match Sequence::detect(vec![file_path.clone()]) {
                    Ok(sequences) => {
                        for seq in sequences {
                            app.player.cache.append_seq(seq);
                        }
                    }
                    Err(e) => {
                        warn!("Failed to load {}: {}", file_path.display(), e);
                    }
                }
            } else if cache_path.exists() {
                // Restore cache state (instant UI, no I/O)
                info!("Restoring cache from {}", cache_path.display());
                match app.player.cache.from_json(&cache_path, false) {
                    Ok(count) => {
                        info!("Cache restored: {} sequences", count);

                        // Trigger frame loading from current position
                        app.player.cache.signal_preload();
                    }
                    Err(e) => {
                        warn!("Failed to restore cache: {}", e);
                    }
                }
            } else {
                info!("No cache file found, starting with empty state");
            }

            Ok(Box::new(app))
        }),
    )?;

    info!("Application exiting");
    Ok(())
}
