//! All-in-one facade for the playa-jobs subsystem.
//!
//! Re-exports the core queue + (optional, gated by Cargo features) UI
//! widgets, pluggable prefs, and the Seedance provider. External consumers
//! wanting "jobs in a box" depend only on this crate; advanced consumers
//! depend on the underlying crates directly for finer control.
//!
//! # Cargo features
//!
//! - `ui` (default): includes [`playa_jobs_ui`] (egui widgets — JobsPanel,
//!   SubmitDialog, prefs::render). Implies `prefs`.
//! - `prefs` (default): includes [`egui_prefs`] (PrefsRegistry +
//!   PrefsTreeView).
//! - `seedance` (default): includes [`playa_job_seedance`] (fal.ai
//!   provider — image-to-video, text-to-video).
//! - `persist` (default): forwards to `playa-jobs-core/persist` so the
//!   queue's JSONL append-log is compiled in.
//!
//! Disable any of them with `default-features = false` plus `features = [..]`.
//!
//! # Example
//!
//! ```ignore
//! use std::sync::Arc;
//! use std::path::PathBuf;
//! use playa_jobs::{JobQueue, JobQueueConfig};
//!
//! let cfg = JobQueueConfig {
//!     thread_count: 2,
//!     files_dir: dirs_next::cache_dir().unwrap().join("myapp/jobs"),
//!     persist_path: Some(dirs_next::config_dir().unwrap().join("myapp/jobs.jsonl")),
//! };
//! let event_bus = Arc::new(playa_jobs::EventBus::new());
//! let queue = playa_jobs::setup_with_fal(event_bus, cfg, &[PathBuf::from(".env")])?;
//! // ... use queue ...
//! ```

#![forbid(unsafe_code)]

pub use playa_jobs_core::*;

#[cfg(feature = "ui")]
pub mod ui {
    //! Re-export of [`playa_jobs_ui`].
    pub use playa_jobs_ui::*;
}

#[cfg(feature = "prefs")]
pub mod prefs {
    //! Re-export of [`egui_prefs`].
    pub use egui_prefs::*;
}

#[cfg(feature = "seedance")]
pub mod seedance {
    //! Re-export of [`playa_job_seedance`].
    pub use playa_job_seedance::*;
}

#[cfg(feature = "inpaint")]
pub mod inpaint {
    //! Re-export of [`playa_job_inpaint`].
    pub use playa_job_inpaint::*;
}

// =============================================================================
// Setup helpers
// =============================================================================

/// Convenience constructor for the common case: build a [`JobQueue`] and
/// (when the `seedance` feature is on) auto-register both Seedance endpoints
/// if a fal.ai key is found in env or any of the supplied `.env` paths.
///
/// Lookup order for the API key matches
/// [`playa_jobs_core::secret::lookup`]:
/// `PLAYA_FAL_KEY → FAL_KEY → FAL_API_KEY` env vars first; then each path
/// in `fal_key_paths` is parsed as a `KEY=value` file.
///
/// Returns the queue inside an `Arc` so the caller can clone it freely
/// across threads / UI panels / providers.
#[cfg(feature = "seedance")]
pub fn setup_with_fal(
    event_bus: std::sync::Arc<EventBus>,
    config: JobQueueConfig,
    fal_key_paths: &[std::path::PathBuf],
) -> std::io::Result<std::sync::Arc<JobQueue>> {
    let queue = std::sync::Arc::new(JobQueue::new(config, event_bus)?);
    if let Some(key) = secret::lookup(
        &["PLAYA_FAL_KEY", "FAL_KEY", "FAL_API_KEY"],
        fal_key_paths,
    ) {
        queue.register_provider(seedance::SeedanceProvider::image_to_video(key.clone()));
        queue.register_provider(seedance::SeedanceProvider::text_to_video(key));
        log::info!(
            "playa-jobs: registered Seedance providers ({}, {})",
            seedance::kinds::IMAGE_TO_VIDEO,
            seedance::kinds::TEXT_TO_VIDEO,
        );
    } else {
        log::info!(
            "playa-jobs: no FAL key found in env or .env files; Seedance providers NOT registered"
        );
    }
    Ok(queue)
}

/// Add the default "Jobs & Rendering" preferences entry into a host's
/// [`prefs::PrefsRegistry`]. Caller passes `extract` — a closure that
/// projects from their `AppSettings` slice down to a `&mut JobsSettings`.
///
/// ```ignore
/// playa_jobs::register_default_prefs(&mut registry, |s| &mut s.jobs);
/// ```
#[cfg(all(feature = "ui", feature = "prefs"))]
pub fn register_default_prefs<S>(
    registry: &mut prefs::PrefsRegistry<S>,
    mut extract: impl FnMut(&mut S) -> &mut JobsSettings + Send + Sync + 'static,
) where
    S: 'static,
{
    registry.add(prefs::PrefsEntry {
        id: "jobs",
        label: "Jobs & Rendering",
        category: "Integrations",
        search_keywords: vec![
            "seedance", "fal.ai", "fal", "budget", "queue", "cost", "video",
            "render",
        ],
        render: Box::new(move |ui, state: &mut S| {
            let jobs_settings = extract(state);
            ui::prefs::render(ui, jobs_settings);
        }),
    });
}
