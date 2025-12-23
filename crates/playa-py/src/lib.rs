//! Python bindings for playa - image sequence player.
//!
//! Usage:
//! ```python
//! import playa
//!
//! # Run player with file
//! playa.run(file="path/to/image.exr")
//!
//! # Run with options
//! playa.run(
//!     file="path/to/sequence.0001.exr",
//!     autoplay=True,
//!     loop_playback=True,
//!     fullscreen=False,
//!     frame=0,
//! )
//! ```

use clap::Parser;
use pyo3::prelude::*;

use ::playa::cli::Args;
use ::playa::run_app;

/// Run playa player with the given options.
///
/// Args:
///     file: Path to image file or sequence (EXR, PNG, JPEG, TIFF, TGA, MP4)
///     files: Additional files to load
///     autoplay: Start playing immediately (default: False)
///     loop_playback: Enable loop mode (default: True)
///     fullscreen: Start in fullscreen mode (default: False)
///     frame: Start at specific frame number (default: 0)
///     start: Play range start frame
///     end: Play range end frame
///
/// Returns:
///     None
///
/// Raises:
///     RuntimeError: If player fails to start
#[pyfunction]
#[pyo3(signature = (
    file = None,
    files = None,
    autoplay = false,
    loop_playback = true,
    fullscreen = false,
    frame = None,
    start = None,
    end = None,
))]
fn run(
    file: Option<&str>,
    files: Option<Vec<String>>,
    autoplay: bool,
    loop_playback: bool,
    fullscreen: bool,
    frame: Option<i32>,
    start: Option<i32>,
    end: Option<i32>,
) -> PyResult<()> {
    // Init logging (only once)
    let _ = env_logger::try_init();

    // Build args vector matching CLI interface
    let mut args: Vec<String> = vec!["playa".to_string()];

    // Add primary file
    if let Some(f) = file {
        args.push(f.to_string());
    }

    // Add additional files
    if let Some(fs) = files {
        for f in fs {
            args.push("-f".to_string());
            args.push(f);
        }
    }

    // Flags
    if autoplay {
        args.push("--autoplay".to_string());
    }
    if !loop_playback {
        args.push("--loop".to_string());
        args.push("0".to_string());
    }
    if fullscreen {
        args.push("--fullscreen".to_string());
    }

    // Frame options
    if let Some(n) = frame {
        args.push("--frame".to_string());
        args.push(n.to_string());
    }
    if let Some(n) = start {
        args.push("--start".to_string());
        args.push(n.to_string());
    }
    if let Some(n) = end {
        args.push("--end".to_string());
        args.push(n.to_string());
    }

    log::info!("playa-py: run({:?})", args);

    // Parse args using clap
    let cli_args = Args::try_parse_from(&args)
        .map_err(|e: clap::Error| PyErr::new::<pyo3::exceptions::PyValueError, _>(e.to_string()))?;

    // Run the application
    run_app(cli_args)
        .map_err(|e: Box<dyn std::error::Error>| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

    Ok(())
}

/// Get playa version string.
#[pyfunction]
fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Python module definition.
#[pymodule]
fn playa(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(run, m)?)?;
    m.add_function(wrap_pyfunction!(version, m)?)?;
    Ok(())
}
