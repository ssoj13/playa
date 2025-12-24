use clap::Parser;
use std::path::PathBuf;

// EXR backend info (compile-time)
#[cfg(feature = "openexr")]
const EXR_BACKEND: &str = "openexr-rs 0.11 (C++, DWAA/DWAB)";
#[cfg(not(feature = "openexr"))]
const EXR_BACKEND: &str = "exrs (pure Rust)";

// Build version with backend info
const VERSION_INFO: &str = const_format::concatcp!(
    env!("CARGO_PKG_VERSION"), "\n",
    "EXR:    ", EXR_BACKEND, "\n",
    "Video:  playa-ffmpeg 8.0 (static)\n",
    "Target: ", std::env::consts::ARCH, "-", std::env::consts::OS
);

/// Image sequence player
#[derive(Parser, Debug)]
#[command(author, version = VERSION_INFO, about, long_about = None)]
pub struct Args {
    /// Path to the image file to load (EXR, PNG, JPEG, TIFF, TGA) - optional, can also drag-and-drop
    #[arg(value_name = "FILE")]
    pub file_path: Option<PathBuf>,

    /// Additional files to load (can be specified multiple times)
    #[arg(short = 'f', long = "file", value_name = "FILE")]
    pub files: Vec<PathBuf>,

    /// Load playlist from JSON file
    #[arg(short = 'p', long = "playlist", value_name = "PLAYLIST")]
    pub playlist: Option<PathBuf>,

    /// Start in fullscreen mode
    #[arg(short = 'F', long = "fullscreen")]
    pub fullscreen: bool,

    /// Start frame number (0-based)
    #[arg(long = "frame", value_name = "N")]
    pub start_frame: Option<i32>,

    /// Auto-play on startup
    #[arg(short = 'a', long = "autoplay")]
    pub autoplay: bool,

    /// Enable looping (default: true)
    #[arg(short = 'o', long = "loop", value_name = "0|1", default_value = "1")]
    pub loop_playback: u8,

    /// Play range start frame
    #[arg(long = "start", value_name = "N")]
    pub range_start: Option<i32>,

    /// Play range end frame
    #[arg(long = "end", value_name = "N")]
    pub range_end: Option<i32>,

    /// Play range (shorthand for --start and --end)
    #[arg(long = "range", value_names = ["START", "END"], num_args = 2)]
    pub range: Option<Vec<i32>>,

    /// Enable debug logging to file (default: playa.log)
    #[arg(short = 'l', long = "log", value_name = "LOG_FILE")]
    pub log_file: Option<Option<PathBuf>>,

    /// Increase logging verbosity (default: warn, -v: info, -vv: debug, -vvv+: trace)
    #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count)]
    pub verbosity: u8,

    /// Custom configuration directory (overrides default platform paths)
    #[arg(short = 'c', long = "config-dir", value_name = "DIR")]
    pub config_dir: Option<PathBuf>,

    /// Deprecated: cache memory budget (was used for old frame cache, now ignored)
    #[arg(long = "mem", value_name = "PERCENT", hide = true)]
    pub mem_percent: Option<f64>,

    /// Deprecated: worker threads override for old frame cache (now ignored)
    #[arg(long = "workers", value_name = "N", hide = true)]
    pub workers: Option<usize>,
}
