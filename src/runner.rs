//! Application runner - entry point for both CLI and Python bindings.

use std::sync::Arc;

use log::{info, trace, warn};

use crate::app::PlayaApp;
use crate::cli::Args;
use crate::config;
use crate::core::player::Player;
use crate::core::workers::Workers;
use crate::widgets::status::StatusBar;

/// Run the playa application with given arguments.
///
/// This is the main entry point used by both:
/// - CLI binary (main.rs)
/// - Python bindings (playa-py)
///
/// # Arguments
/// * `args` - Parsed command-line arguments
///
/// # Returns
/// * `Ok(())` on successful exit
/// * `Err` if initialization or runtime fails
pub fn run_app(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    // Create path configuration from CLI args and environment
    let path_config = config::PathConfig::from_env_and_cli(args.config_dir.clone());

    // Ensure directories exist
    if let Err(e) = config::ensure_dirs(&path_config) {
        eprintln!("Warning: Failed to create application directories: {}", e);
    }

    info!("Playa Image Sequence Player starting...");
    trace!("Command-line args: {:?}", args);

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
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title(format!(
                "Playa v{} - {} - F1 for help",
                env!("CARGO_PKG_VERSION"),
                BACKEND
            ))
            .with_inner_size([1852.0, 1089.0])
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
            let mem_fraction = args
                .mem_percent
                .map(|p| (p / 100.0).clamp(0.05, 0.95))
                .unwrap_or(0.75);

            // workers_override in settings controls App-level workers
            let desired_workers = args.workers.or(if app.settings.workers_override > 0 {
                Some(app.settings.workers_override as usize)
            } else {
                None
            });

            // Recreate worker pool with CLI/settings override if specified
            if let Some(num_workers) = desired_workers {
                let num_workers = num_workers.max(1);
                info!(
                    "Recreating worker pool with {} threads (CLI/settings override)",
                    num_workers
                );
                app.workers = Arc::new(Workers::new(num_workers, app.cache_manager.epoch_ref()));
            }

            // Recreate Player runtime (no longer owns project)
            let mut player = Player::new();

            // Attach schemas (not serialized, must restore after deserialize)
            app.project.attach_schemas();

            // Rebuild runtime + set cache manager (unified, lost during clone/deserialization)
            app.project.rebuild_with_manager(
                Arc::clone(&app.cache_manager),
                app.settings.cache_strategy,
                Some(app.comp_event_emitter.clone()),
            );
            // Restore event emitter (lost during serde deserialization - #[serde(skip)])
            app.project.set_event_emitter(app.event_bus.emitter());

            // Restore active from project or ensure default
            let active_uuid = app.project.active().or_else(|| {
                let uuid = app.project.ensure_default_comp();
                Some(uuid)
            });
            player.set_active_comp(active_uuid, &mut app.project);

            // Kick initial cache/preload after restore
            if let Some(active) = active_uuid {
                app.project.modify_comp(active, |comp| {
                    comp.attrs.mark_dirty();
                });
            }

            app.player = player;
            app.status_bar = StatusBar::new();
            app.applied_mem_fraction = mem_fraction;
            app.applied_cache_strategy = app.settings.cache_strategy;
            app.applied_workers = desired_workers;
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
            app.player.set_fps_play(app.settings.fps_base);
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
                            project.attach_schemas();

                            project.rebuild_with_manager(
                                Arc::clone(&app.cache_manager),
                                app.settings.cache_strategy,
                                Some(app.comp_event_emitter.clone()),
                            );
                            project.set_event_emitter(app.event_bus.emitter());

                            app.project = project;
                            info!("Playlist loaded via Project");

                            // Sync player + panels to playlist's active comp
                            let active_uuid = app.project.active().or_else(|| {
                                let uuid = app.project.ensure_default_comp();
                                Some(uuid)
                            });
                            app.player.set_active_comp(active_uuid, &mut app.project);
                            if let Some(active) = active_uuid {
                                app.node_editor_state.set_comp(active);
                                app.node_editor_state.mark_dirty();

                                app.project.modify_comp(active, |comp| {
                                    comp.attrs.mark_dirty();
                                });
                            }
                            app.selected_media_uuid = app.project.selection().last().cloned();
                        }
                        Err(e) => {
                            warn!("Failed to load playlist {}: {}", playlist_path.display(), e);
                        }
                    }
                }

                // Apply CLI options
                if let Some(frame) = args.start_frame {
                    app.player.set_frame(frame, &mut app.project);
                    app.enqueue_current_frame_only();
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
