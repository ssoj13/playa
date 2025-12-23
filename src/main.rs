//! Playa - Video editing application main entry point.
//!
//! This is the binary crate entry point. All application logic
//! is in the library crate (`playa::app` module).

use clap::{CommandFactory, Parser};
use log::info;

use playa::cli::Args;
use playa::config;

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
        let mut cmd = Args::command();
        let _ = cmd.print_help();
        println!("\n");
    }

    // Create path configuration from CLI args and environment
    let path_config = config::PathConfig::from_env_and_cli(args.config_dir.clone());

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
            .filter_module("egui", log::LevelFilter::Info)
            .filter_module("egui_taffy", log::LevelFilter::Warn)
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
            .filter_module("egui", log::LevelFilter::Info)
            .filter_module("egui_taffy", log::LevelFilter::Warn)
            .format_timestamp_millis()
            .init();
    }

    // Run the application
    playa::run_app(args)
}
