//! End-to-end integration tests using **only** the `playa_jobs` facade.
//!
//! No imports from `playa_jobs_core`, `playa_jobs_ui`, `playa_prefs`, or
//! `playa_job_seedance` — proving the facade re-exports are sufficient for
//! a downstream consumer to wire the whole queue without knowing about the
//! underlying crates.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use playa_jobs::{
    Job, JobContext, JobError, JobId, JobProvider, JobQueue, JobQueueConfig, JobState,
};

struct EchoProvider;

impl JobProvider for EchoProvider {
    fn kind(&self) -> &'static str {
        "test.echo"
    }
    fn run(&self, ctx: &JobContext, params: serde_json::Value) -> Result<serde_json::Value, JobError> {
        ctx.set_state(JobState::Submitting);
        ctx.cancel.check_err()?;
        Ok(params)
    }
}

fn config_no_persist() -> JobQueueConfig {
    let mut cfg = JobQueueConfig::default();
    cfg.thread_count = 1;
    cfg.files_dir = std::env::temp_dir().join(format!("playa-jobs-facade-{}", uuid::Uuid::new_v4()));
    #[cfg(feature = "persist")]
    {
        cfg.persist_path = None;
    }
    cfg
}

fn poll_until<F: Fn() -> bool>(timeout: Duration, cond: F) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

#[test]
fn facade_exposes_jobqueue_and_provider() {
    let event_bus = Arc::new(playa_jobs::EventBus::new());
    let queue = JobQueue::new(config_no_persist(), Arc::clone(&event_bus)).unwrap();
    queue.register_provider(EchoProvider);

    let id = queue
        .submit("test.echo", serde_json::json!({"hello": "world"}))
        .unwrap();
    assert!(poll_until(Duration::from_secs(2), || queue
        .get(id)
        .map(|j| j.state == JobState::Complete)
        .unwrap_or(false)));

    let job = queue.get(id).unwrap();
    assert_eq!(job.result.as_ref().unwrap()["hello"], "world");
    queue.shutdown();
}

#[test]
fn facade_jobs_settings_round_trips_through_serde_json() {
    use playa_jobs::JobsSettings;
    let s = JobsSettings::default();
    let json = serde_json::to_string(&s).unwrap();
    let back: JobsSettings = serde_json::from_str(&json).unwrap();
    assert_eq!(back, s);
}

#[cfg(feature = "seedance")]
#[test]
fn facade_seedance_endpoint_kinds_visible() {
    use playa_jobs::seedance::kinds;
    assert_eq!(kinds::IMAGE_TO_VIDEO, "seedance.image_to_video");
    assert_eq!(kinds::TEXT_TO_VIDEO, "seedance.text_to_video");
}

#[cfg(feature = "seedance")]
#[test]
fn facade_seedance_provider_constructor_compiles() {
    // Smoke: just construct a SeedanceProvider via the facade re-export.
    // Don't submit (would need a queue + mock; covered by playa-job-seedance's
    // own tests).
    let _ = playa_jobs::seedance::SeedanceProvider::text_to_video("dummy-key");
}

#[cfg(feature = "ui")]
#[test]
fn facade_jobs_panel_constructible() {
    let p = playa_jobs::ui::JobsPanel::new();
    assert!(!p.filter_active_only);
    assert!(p.filter_search.is_empty());
}

#[cfg(feature = "ui")]
#[test]
fn facade_submit_dialog_round_trips_params() {
    let mut d = playa_jobs::ui::SubmitDialog::default();
    d.prompt = "test prompt".into();
    d.duration_secs = 5;
    let body = d.build_params();
    assert_eq!(body["prompt"], "test prompt");
    assert_eq!(body["duration"], "5"); // text-to-video default = string duration
}

#[cfg(all(feature = "ui", feature = "prefs"))]
#[test]
fn facade_register_default_prefs_attaches_jobs_entry() {
    use playa_jobs::JobsSettings;

    #[derive(Clone, PartialEq, Default)]
    struct HostSettings {
        jobs: JobsSettings,
    }

    let mut registry = playa_jobs::prefs::PrefsRegistry::<HostSettings>::new();
    playa_jobs::register_default_prefs(&mut registry, |s| &mut s.jobs);

    assert_eq!(registry.len(), 1);
    let entry = registry.find_by_id("jobs").unwrap();
    assert_eq!(entry.label, "Jobs & Rendering");
    assert_eq!(entry.category, "Integrations");
    assert!(entry
        .search_keywords
        .iter()
        .any(|k| *k == "seedance"));
}

#[test]
fn facade_secret_lookup_reachable() {
    // Sanity: `playa_jobs::secret::lookup` is reachable through the re-export.
    let v = playa_jobs::secret::lookup(
        &["PLAYA_DEFINITELY_NOT_SET_KEY"],
        &[std::path::PathBuf::from("/nonexistent/.env")],
    );
    assert!(v.is_none());
}

#[test]
fn facade_cancel_token_round_trips() {
    let t = playa_jobs::CancelToken::new();
    assert!(!t.is_cancelled());
    t.cancel();
    assert!(t.is_cancelled());
}

#[test]
fn facade_listener_pattern_works_for_complete_event() {
    let event_bus = Arc::new(playa_jobs::EventBus::new());
    let queue = JobQueue::new(config_no_persist(), Arc::clone(&event_bus)).unwrap();
    queue.register_provider(EchoProvider);

    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = Arc::clone(&counter);
    event_bus.subscribe::<playa_jobs::JobEvent, _>(move |ev| {
        if matches!(ev, playa_jobs::JobEvent::Completed(_, _)) {
            counter_clone.fetch_add(1, Ordering::Relaxed);
        }
    });

    let _ = queue.submit("test.echo", serde_json::json!({})).unwrap();
    assert!(poll_until(Duration::from_secs(2), || counter.load(Ordering::Relaxed) > 0));

    queue.shutdown();
}

// Suppress unused-import warning on Job + JobId for variants that exist
// solely as re-export sanity for downstream consumers.
fn _re_export_sanity() {
    let _: Option<Job> = None;
    let _: Option<JobId> = None;
}
