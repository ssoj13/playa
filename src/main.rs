mod cache;
mod convert;
mod encode;
mod exr;
mod frame;
mod paths;
mod player;
mod prefs;
mod progress;
mod progress_bar;
mod scrub;
mod sequence;
mod shaders;
mod status_bar;
mod timeslider;
mod ui;
mod ui_encode;
mod utils;
mod video;
mod viewport;

use clap::Parser;
use eframe::{egui, glow};
use frame::Frame;
use log::{debug, error, info, warn};
use player::Player;
use prefs::{AppSettings, render_settings_window};
use scrub::Scrubber;
use sequence::Sequence;
use shaders::Shaders;
use status_bar::StatusBar;
use std::path::PathBuf;
use viewport::{ViewportRenderer, ViewportState};

/// Image sequence player
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the image file to load (EXR, PNG, JPEG, TIFF, TGA) - optional, can also drag-and-drop
    #[arg(value_name = "FILE")]
    file_path: Option<PathBuf>,

    /// Additional files to load (can be specified multiple times)
    #[arg(short = 'f', long = "file", value_name = "FILE")]
    files: Vec<PathBuf>,

    /// Load playlist from JSON file
    #[arg(short = 'p', long = "playlist", value_name = "PLAYLIST")]
    playlist: Option<PathBuf>,

    /// Start in fullscreen mode
    #[arg(short = 'F', long = "fullscreen")]
    fullscreen: bool,

    /// Start frame number (0-based)
    #[arg(long = "frame", value_name = "N")]
    start_frame: Option<usize>,

    /// Auto-play on startup
    #[arg(short = 'a', long = "autoplay")]
    autoplay: bool,

    /// Enable looping (default: true)
    #[arg(short = 'o', long = "loop", value_name = "0|1", default_value = "1")]
    loop_playback: u8,

    /// Play range start frame
    #[arg(long = "start", value_name = "N")]
    range_start: Option<usize>,

    /// Play range end frame
    #[arg(long = "end", value_name = "N")]
    range_end: Option<usize>,

    /// Play range (shorthand for --start and --end)
    #[arg(long = "range", value_names = ["START", "END"], num_args = 2)]
    range: Option<Vec<usize>>,

    /// Enable debug logging to file (default: playa.log)
    #[arg(short = 'l', long = "log", value_name = "LOG_FILE")]
    log_file: Option<Option<PathBuf>>,

    /// Increase logging verbosity (default: warn, -v: info, -vv: debug, -vvv+: trace)
    #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count)]
    verbosity: u8,

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
    show_encode_dialog: bool,
    #[serde(skip)]
    encode_dialog: Option<ui_encode::EncodeDialog>,
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
            show_encode_dialog: false,
            encode_dialog: None,
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

                // Clear error message on successful load
                self.error_msg = None;

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

        if input.key_pressed(egui::Key::F4) {
            self.show_encode_dialog = !self.show_encode_dialog;
            // Load dialog state from settings when opening
            if self.show_encode_dialog && self.encode_dialog.is_none() {
                debug!("[F4] Opening encode dialog, loading settings from AppSettings");
                self.encode_dialog = Some(ui_encode::EncodeDialog::load_from_settings(
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
                if let Some(ref mut dialog) = self.encode_dialog {
                    if dialog.is_encoding() {
                        dialog.stop_encoding();
                    }
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
            self.player.toggle_play_pause();
        }

        // Stop (K)
        if input.key_pressed(egui::Key::K) {
            self.player.stop();
        }

        // Only process playback hotkeys when no widget has keyboard focus
        // (prevents arrow keys from triggering playback while editing text fields)
        if !ctx.wants_keyboard_input() {
            // Jump to start (1, Home)
            if input.key_pressed(egui::Key::Num1) || input.key_pressed(egui::Key::Home) {
                self.player.to_start();
            }

            // Jump to end (2, End)
            if input.key_pressed(egui::Key::Num2) || input.key_pressed(egui::Key::End) {
                self.player.to_end();
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

            // Shift+ArrowLeft - step 25 frames backward
            if input.modifiers.shift && input.key_pressed(egui::Key::ArrowLeft) {
                self.player.step(-crate::player::FRAME_JUMP_STEP);
            }
            // Shift+ArrowRight - step 25 frames forward
            else if input.modifiers.shift && input.key_pressed(egui::Key::ArrowRight) {
                self.player.step(crate::player::FRAME_JUMP_STEP);
            }
            // J, <, Left Arrow - jog backward (no modifiers)
            else if input.key_pressed(egui::Key::J)
                || (!input.modifiers.any() && input.key_pressed(egui::Key::ArrowLeft))
                || input.key_pressed(egui::Key::Comma)
            {
                self.player.jog_backward();
            }
            // L, >, Right Arrow - jog forward (no modifiers)
            else if input.key_pressed(egui::Key::L)
                || (!input.modifiers.any() && input.key_pressed(egui::Key::ArrowRight))
                || input.key_pressed(egui::Key::Period)
            {
                self.player.jog_forward();
            }

            // ArrowDown - decrease play FPS (only when playing)
            if input.key_pressed(egui::Key::ArrowDown) {
                self.player.decrease_fps_play();
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

            // Set play range start (B = Begin)
            if !input.modifiers.ctrl && input.key_pressed(egui::Key::B) {
                let current = self.player.cache.frame();
                let (_, end) = self.player.cache.get_play_range();
                self.player.cache.set_play_range(current, end);
            }

            // Set play range end (N = eNd)
            if input.key_pressed(egui::Key::N) {
                let current = self.player.cache.frame();
                let (start, _) = self.player.cache.get_play_range();
                self.player.cache.set_play_range(start, current);
            }

            // Reset play range to full sequence (Ctrl+B)
            if input.modifiers.ctrl && input.key_pressed(egui::Key::B) {
                self.player.cache.reset_play_range();
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

            // Viewport controls
            if input.key_pressed(egui::Key::F) {
                self.viewport_state.set_mode_fit();
            }

            // 100% zoom (A, H only - 1/Home now used for jump to start)
            if input.key_pressed(egui::Key::A) || input.key_pressed(egui::Key::H) {
                self.viewport_state.set_mode_100();
            }
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
        let desired_mem_fraction =
            (self.settings.cache_mem_percent as f64 / 100.0).clamp(0.05, 0.95);
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
                if let Some(path) = &file.path {
                    dropped.push(path.clone());
                }
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
                            // Clear error message on successful load
                            self.error_msg = None;
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
                        // Clear error message on successful load
                        self.error_msg = None;
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
                self.settings.show_frame_numbers,
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
                    // Clear error message on successful load
                    self.error_msg = None;
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

        // Encode dialog (can be shown even in cinema mode)
        if self.show_encode_dialog
            && let Some(ref mut dialog) = self.encode_dialog
        {
            let should_stay_open = dialog.render(ctx, &self.player.cache);

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

        // Save cache state separately (sequences + current frame)
        let cache_path = paths::data_file("playa_cache.json", &self.path_config);

        if let Err(e) = self.player.cache.to_json(&cache_path) {
            warn!("Failed to save cache state: {}", e);
        }

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
        || args.config_dir.is_some()
        || args.mem_percent.is_some()
        || args.workers.is_some();

    if !has_any_args {
        // Print help in GUI mode (no CLI arguments provided)
        use clap::CommandFactory;
        let mut cmd = Args::command();
        let _ = cmd.print_help();
        println!("\n");
    }

    // Create path configuration from CLI args and environment
    let path_config = paths::PathConfig::from_env_and_cli(args.config_dir.clone());

    // Ensure directories exist
    if let Err(e) = paths::ensure_dirs(&path_config) {
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
            .unwrap_or_else(|| paths::data_file("playa.log", &path_config));

        let file = std::fs::File::create(&log_path).expect("Failed to create log file");

        env_logger::Builder::new()
            .filter_level(log_level)
            .format_timestamp_millis()
            .target(env_logger::Target::Pipe(Box::new(file)))
            .init();

        info!("Logging to file: {} (level: {:?})", log_path.display(), log_level);
    } else {
        // Console logging with specified verbosity level (respects RUST_LOG if set)
        let default_level = match args.verbosity {
            0 => "warn",
            1 => "info",
            2 => "debug",
            _ => "trace",
        };

        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(default_level))
            .format_timestamp_millis()
            .init();
    }

    info!("Playa Image Sequence Player starting...");
    debug!("Command-line args: {:?}", args);

    // Log application paths
    info!(
        "Config path: {}",
        paths::config_file("playa.json", &path_config).display()
    );
    info!(
        "Data path: {}",
        paths::data_file("playa_cache.json", &path_config)
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
            let mut app = cc
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
                .or_else(|| Some((app.settings.cache_mem_percent as f64 / 100.0).clamp(0.05, 0.95)))
                .unwrap_or(0.75);
            let workers = args.workers.or(if app.settings.workers_override > 0 {
                Some(app.settings.workers_override as usize)
            } else {
                None
            });
            let (player, ui_rx) = Player::new_with_config(mem_fraction, workers);
            app.player = player;
            app.status_bar = StatusBar::new(ui_rx);
            app.applied_mem_fraction = mem_fraction;
            app.applied_workers = workers;
            app.path_config = path_config_for_app;

            // Attempt to load shaders from the shaders directory
            if let Err(_) = app
                .shader_manager
                .load_shader_directory(&std::path::PathBuf::from("shaders"))
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

            // Fast cache restoration (sequences + current frame)
            let cache_path = paths::data_file("playa_cache.json", &path_config);

            // CLI arguments have priority
            let has_cli_input = args.file_path.is_some() || !args.files.is_empty() || args.playlist.is_some();

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
                    match Sequence::detect(all_files.clone()) {
                        Ok(sequences) => {
                            for seq in sequences {
                                app.player.cache.append_seq(seq);
                            }
                            info!("Loaded {} files", all_files.len());
                        }
                        Err(e) => {
                            warn!("Failed to load files: {}", e);
                        }
                    }
                }

                // Load playlist
                if let Some(ref playlist_path) = args.playlist {
                    info!("Loading playlist: {}", playlist_path.display());
                    match app.player.cache.from_json(playlist_path, false) {
                        Ok(count) => {
                            info!("Playlist loaded: {} sequences", count);
                        }
                        Err(e) => {
                            warn!("Failed to load playlist {}: {}", playlist_path.display(), e);
                        }
                    }
                }

                // Apply CLI options
                if let Some(frame) = args.start_frame {
                    app.player.cache.set_frame(frame);
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
                    app.player.cache.set_play_range(start, end);
                }

                // Set fullscreen
                if args.fullscreen {
                    app.set_cinema_mode(&cc.egui_ctx, true);
                }

                // Trigger preload
                app.player.cache.signal_preload();
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
