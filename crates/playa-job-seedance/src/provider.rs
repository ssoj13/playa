//! [`SeedanceProvider`] implementation. Plug into [`playa_jobs::JobQueue`] via
//! [`playa_jobs::JobQueue::register_provider`].

use std::sync::Arc;
use std::time::Duration;

use playa_jobs::{JobContext, JobError, JobProgress, JobProvider, JobState};
use serde_json::{Value, json};

use crate::http::{FalHttp, UreqFalHttp};

/// Stable kind strings used by callers when submitting via
/// [`playa_jobs::JobQueue::submit`]. Kept in a sub-module so consumers can
/// `use playa_job_seedance::kinds::*` without polluting their namespace.
pub mod kinds {
    pub const IMAGE_TO_VIDEO: &str = "seedance.image_to_video";
    pub const TEXT_TO_VIDEO: &str = "seedance.text_to_video";
}

/// Legacy alias retained for backward compatibility — use [`kinds::IMAGE_TO_VIDEO`].
pub const KIND: &str = kinds::IMAGE_TO_VIDEO;

/// Which Seedance endpoint a [`SeedanceProvider`] instance targets. The two
/// fal endpoints share the queue API shape (submit→poll→download) but carry
/// different request body fields, so we keep one provider type per endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeedanceEndpoint {
    /// `bytedance/seedance-2.0/image-to-video` — requires `image_url`.
    ImageToVideo,
    /// `bytedance/seedance-2.0/text-to-video` — text-only prompt.
    TextToVideo,
}

impl SeedanceEndpoint {
    pub const fn submit_url(self) -> &'static str {
        match self {
            Self::ImageToVideo => "https://queue.fal.run/bytedance/seedance-2.0/image-to-video",
            Self::TextToVideo => "https://queue.fal.run/bytedance/seedance-2.0/text-to-video",
        }
    }

    pub const fn kind(self) -> &'static str {
        match self {
            Self::ImageToVideo => kinds::IMAGE_TO_VIDEO,
            Self::TextToVideo => kinds::TEXT_TO_VIDEO,
        }
    }
}

/// Configuration knobs for [`SeedanceProvider`]. Tests adjust the poll
/// interval to keep them fast; production sticks with the default.
#[derive(Debug, Clone)]
pub struct SeedanceProviderConfig {
    pub poll_interval: Duration,
    /// Stop polling after this many ticks even if fal is still queued —
    /// safety net against forever-stuck queue positions. The provider returns
    /// `JobError::Provider("poll budget exhausted")` in that case.
    pub max_poll_ticks: usize,
}

impl Default for SeedanceProviderConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(5),
            // 5 s × 1440 = 2 hours. fal's published max queue wait is far
            // shorter; if we hit this, something is wrong.
            max_poll_ticks: 1440,
        }
    }
}

pub struct SeedanceProvider {
    api_key: String,
    http: Arc<dyn FalHttp>,
    config: SeedanceProviderConfig,
    endpoint: SeedanceEndpoint,
}

impl SeedanceProvider {
    /// Production constructor with explicit endpoint.
    pub fn new(endpoint: SeedanceEndpoint, api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            http: Arc::new(UreqFalHttp::new()),
            config: SeedanceProviderConfig::default(),
            endpoint,
        }
    }

    /// Production shortcut for image-to-video.
    pub fn image_to_video(api_key: impl Into<String>) -> Self {
        Self::new(SeedanceEndpoint::ImageToVideo, api_key)
    }

    /// Production shortcut for text-to-video.
    pub fn text_to_video(api_key: impl Into<String>) -> Self {
        Self::new(SeedanceEndpoint::TextToVideo, api_key)
    }

    /// Test / advanced constructor. Pass a mock [`FalHttp`] and a tight
    /// poll interval so unit tests run in milliseconds.
    pub fn with_http(
        endpoint: SeedanceEndpoint,
        api_key: impl Into<String>,
        http: Arc<dyn FalHttp>,
        config: SeedanceProviderConfig,
    ) -> Self {
        Self {
            api_key: api_key.into(),
            http,
            config,
            endpoint,
        }
    }

    fn poll_until_complete_then_download(
        &self,
        ctx: &JobContext,
        status_url: &str,
        response_url: &str,
    ) -> Result<Value, JobError> {
        for tick in 0..self.config.max_poll_ticks {
            ctx.cancel.check_err()?;
            std::thread::sleep(self.config.poll_interval);
            ctx.cancel.check_err()?;

            let status_resp = self
                .http
                .get_json(status_url, &self.api_key)
                .map_err(JobError::Provider)?;

            let status = status_resp
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if let Some(qpos) = status_resp.get("queue_position").and_then(|v| v.as_u64()) {
                ctx.update(JobProgress {
                    stage: status.to_string(),
                    fraction: None,
                    message: Some(format!("queue position: {qpos} (tick {tick})")),
                });
            } else {
                ctx.update(JobProgress {
                    stage: status.to_string(),
                    fraction: None,
                    message: Some(format!("tick {tick}")),
                });
            }

            match status {
                "COMPLETED" => break,
                "IN_QUEUE" | "IN_PROGRESS" => continue,
                other => {
                    return Err(JobError::Provider(format!(
                        "unexpected fal status `{other}`"
                    )));
                }
            }
        }

        // Fetch the full result envelope.
        ctx.cancel.check_err()?;
        let result = self
            .http
            .get_json(response_url, &self.api_key)
            .map_err(JobError::Provider)?;

        let video_url = result
            .pointer("/video/url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                JobError::Provider(format!(
                    "fal response missing video.url (got: {})",
                    truncate(&result.to_string(), 200)
                ))
            })?
            .to_string();

        ctx.set_state(JobState::Downloading);
        ctx.cancel.check_err()?;

        let mp4_path = ctx.files_dir.join("output.mp4");
        let bytes = self
            .http
            .download(&video_url, &mp4_path)
            .map_err(JobError::Io)?;
        log::info!(
            "Seedance: downloaded {bytes} bytes to {}",
            mp4_path.display()
        );

        ctx.set_state(JobState::Staging);

        Ok(json!({
            "mp4_path": mp4_path.to_string_lossy(),
            "video_url": video_url,
            "bytes": bytes,
            "fal_response": result,
        }))
    }
}

impl JobProvider for SeedanceProvider {
    fn kind(&self) -> &'static str {
        self.endpoint.kind()
    }

    fn run(&self, ctx: &JobContext, params: Value) -> Result<Value, JobError> {
        ctx.set_state(JobState::Submitting);
        ctx.cancel.check_err()?;

        // Forward params verbatim — let fal own validation. The
        // SeedanceImageToVideoParams / SeedanceTextToVideoParams helpers exist
        // for callers who want type safety, but are not required here.
        let submit_resp = self
            .http
            .post_json(self.endpoint.submit_url(), &self.api_key, &params)
            .map_err(JobError::Provider)?;

        let request_id = submit_resp
            .get("request_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                JobError::Provider(format!(
                    "fal submit missing request_id (got: {})",
                    truncate(&submit_resp.to_string(), 200)
                ))
            })?
            .to_string();
        let status_url = submit_resp
            .get("status_url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| JobError::Provider("fal submit missing status_url".into()))?
            .to_string();
        let response_url = submit_resp
            .get("response_url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| JobError::Provider("fal submit missing response_url".into()))?
            .to_string();

        // Persist the URLs **before** we transition to AwaitingProvider so a
        // crash mid-submit cannot lose the request_id (and therefore the API
        // billing). See playa-jobs persist contract.
        ctx.persist_param("request_id", json!(request_id));
        ctx.persist_param("status_url", json!(status_url));
        ctx.persist_param("response_url", json!(response_url));

        ctx.set_state(JobState::AwaitingProvider);
        self.poll_until_complete_then_download(ctx, &status_url, &response_url)
    }

    fn resume(&self, ctx: &JobContext, params: Value) -> Result<Value, JobError> {
        let status_url = params
            .get("status_url")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let response_url = params
            .get("response_url")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        match (status_url, response_url) {
            (Some(status_url), Some(response_url)) => {
                log::info!(
                    "Seedance: resuming job, status_url={status_url} response_url={response_url}"
                );
                ctx.set_state(JobState::AwaitingProvider);
                self.poll_until_complete_then_download(ctx, &status_url, &response_url)
            }
            _ => {
                log::warn!("Seedance: resume called without persisted URLs — re-submitting");
                self.run(ctx, params)
            }
        }
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}…", &s[..n])
    }
}

// -----------------------------------------------------------------------------
// Tests — mock FalHttp, full provider state machine.
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use playa_jobs::{JobEvent, JobQueue, JobQueueConfig};
    use std::path::Path;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Scripted [`FalHttp`] mock. Each call advances a per-method counter and
    /// returns the corresponding scripted response.
    struct MockHttp {
        post_responses: Mutex<Vec<Result<Value, String>>>,
        post_calls: AtomicUsize,
        status_responses: Mutex<Vec<Result<Value, String>>>,
        status_calls: AtomicUsize,
        result_responses: Mutex<Vec<Result<Value, String>>>,
        result_calls: AtomicUsize,
        download_calls: AtomicUsize,
        last_post_body: Mutex<Option<Value>>,
    }

    impl MockHttp {
        fn new(
            post: Vec<Result<Value, String>>,
            statuses: Vec<Result<Value, String>>,
            results: Vec<Result<Value, String>>,
        ) -> Self {
            Self {
                post_responses: Mutex::new(post),
                post_calls: AtomicUsize::new(0),
                status_responses: Mutex::new(statuses),
                status_calls: AtomicUsize::new(0),
                result_responses: Mutex::new(results),
                result_calls: AtomicUsize::new(0),
                download_calls: AtomicUsize::new(0),
                last_post_body: Mutex::new(None),
            }
        }
    }

    impl FalHttp for MockHttp {
        fn post_json(&self, _url: &str, _key: &str, body: &Value) -> Result<Value, String> {
            *self.last_post_body.lock().unwrap() = Some(body.clone());
            let idx = self.post_calls.fetch_add(1, Ordering::SeqCst);
            self.post_responses
                .lock()
                .unwrap()
                .get(idx)
                .cloned()
                .unwrap_or_else(|| Err("mock: ran out of post responses".into()))
        }

        fn get_json(&self, url: &str, _key: &str) -> Result<Value, String> {
            // Distinguish status vs result by URL suffix `/status`.
            if url.ends_with("/status") {
                let idx = self.status_calls.fetch_add(1, Ordering::SeqCst);
                self.status_responses
                    .lock()
                    .unwrap()
                    .get(idx)
                    .cloned()
                    .unwrap_or_else(|| Err("mock: ran out of status responses".into()))
            } else {
                let idx = self.result_calls.fetch_add(1, Ordering::SeqCst);
                self.result_responses
                    .lock()
                    .unwrap()
                    .get(idx)
                    .cloned()
                    .unwrap_or_else(|| Err("mock: ran out of result responses".into()))
            }
        }

        fn download(&self, _url: &str, dest: &Path) -> Result<u64, String> {
            self.download_calls.fetch_add(1, Ordering::SeqCst);
            // Touch the file so callers checking .exists() succeed.
            std::fs::write(dest, b"fake-mp4-payload").map_err(|e| e.to_string())?;
            Ok(16)
        }
    }

    fn submit_resp() -> Value {
        json!({
            "request_id": "req-abc-123",
            "status_url": "https://queue.fal.run/bytedance/seedance-2.0/image-to-video/requests/req-abc-123/status",
            "response_url": "https://queue.fal.run/bytedance/seedance-2.0/image-to-video/requests/req-abc-123",
            "cancel_url": "https://queue.fal.run/bytedance/seedance-2.0/image-to-video/requests/req-abc-123/cancel",
            "queue_position": 3,
        })
    }

    fn final_resp() -> Value {
        json!({
            "video": {
                "url": "https://files.fal.run/abc/output.mp4",
                "content_type": "video/mp4",
                "file_name": "output.mp4",
                "file_size": 4823041_u64,
            },
            "seed": 42,
        })
    }

    fn fast_config() -> SeedanceProviderConfig {
        SeedanceProviderConfig {
            poll_interval: Duration::from_millis(2),
            max_poll_ticks: 1000,
        }
    }

    fn jobs_config_no_persist() -> JobQueueConfig {
        // playa-jobs has the `persist` feature on by default, so
        // `JobQueueConfig::persist_path` is unconditionally present from this
        // consumer crate's POV. Force `None` so tests do not append to a real
        // log file under ~/.config.
        let mut cfg = JobQueueConfig::default();
        cfg.thread_count = 2;
        cfg.files_dir = std::env::temp_dir().join(format!(
            "playa-job-seedance-test-{}",
            uuid::Uuid::new_v4()
        ));
        cfg.persist_path = None;
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

    fn submit_uses_endpoint_url(endpoint: SeedanceEndpoint) -> Arc<MockHttp> {
        let mock = Arc::new(MockHttp::new(
            vec![Ok(submit_resp())],
            vec![Ok(json!({"status": "COMPLETED"}))],
            vec![Ok(final_resp())],
        ));
        let provider = SeedanceProvider::with_http(
            endpoint,
            "k",
            mock.clone() as Arc<dyn FalHttp>,
            fast_config(),
        );
        let queue = JobQueue::new(jobs_config_no_persist()).unwrap();
        queue.register_provider(provider);
        let kind = endpoint.kind();
        let id = queue
            .submit(kind, json!({"prompt": "x"}))
            .expect("submit");
        assert!(poll_until(Duration::from_secs(2), || queue
            .get(id)
            .map(|j| j.state == JobState::Complete)
            .unwrap_or(false)));
        queue.shutdown();
        mock
    }

    #[test]
    fn endpoint_image_to_video_kind_string() {
        let p = SeedanceProvider::image_to_video("k");
        assert_eq!(p.kind(), kinds::IMAGE_TO_VIDEO);
    }

    #[test]
    fn endpoint_text_to_video_kind_string() {
        let p = SeedanceProvider::text_to_video("k");
        assert_eq!(p.kind(), kinds::TEXT_TO_VIDEO);
    }

    #[test]
    fn endpoint_submit_urls_distinct() {
        assert_eq!(
            SeedanceEndpoint::ImageToVideo.submit_url(),
            "https://queue.fal.run/bytedance/seedance-2.0/image-to-video"
        );
        assert_eq!(
            SeedanceEndpoint::TextToVideo.submit_url(),
            "https://queue.fal.run/bytedance/seedance-2.0/text-to-video"
        );
    }

    #[test]
    fn each_endpoint_round_trips_via_queue() {
        // Both endpoints produce a Complete job through the same poll/download
        // path; the only differences are submit URL and kind string.
        let mock_i2v = submit_uses_endpoint_url(SeedanceEndpoint::ImageToVideo);
        assert_eq!(mock_i2v.post_calls.load(Ordering::SeqCst), 1);
        let mock_t2v = submit_uses_endpoint_url(SeedanceEndpoint::TextToVideo);
        assert_eq!(mock_t2v.post_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn full_happy_path_submit_poll_complete_download() {
        let mock = Arc::new(MockHttp::new(
            vec![Ok(submit_resp())],
            vec![
                Ok(json!({"status": "IN_QUEUE", "queue_position": 3})),
                Ok(json!({"status": "IN_PROGRESS"})),
                Ok(json!({"status": "COMPLETED"})),
            ],
            vec![Ok(final_resp())],
        ));

        let provider = SeedanceProvider::with_http(
            SeedanceEndpoint::ImageToVideo,
            "test-key",
            mock.clone() as Arc<dyn FalHttp>,
            fast_config(),
        );

        let queue = JobQueue::new(jobs_config_no_persist()).unwrap();
        queue.register_provider(provider);

        let id = queue
            .submit(
                kinds::IMAGE_TO_VIDEO,
                json!({"prompt": "test", "image_url": "https://x/y.png", "duration": 5}),
            )
            .unwrap();
        assert!(poll_until(Duration::from_secs(2), || queue
            .get(id)
            .map(|j| j.state == JobState::Complete)
            .unwrap_or(false)));

        let job = queue.get(id).unwrap();
        let result = job.result.expect("result populated");
        assert_eq!(result["video_url"], "https://files.fal.run/abc/output.mp4");
        assert_eq!(result["bytes"], 16);
        assert!(result["mp4_path"]
            .as_str()
            .unwrap()
            .ends_with("output.mp4"));

        // request_id was persisted.
        let stored_request_id = job
            .params
            .get("request_id")
            .and_then(|v| v.as_str())
            .unwrap();
        assert_eq!(stored_request_id, "req-abc-123");

        assert_eq!(mock.post_calls.load(Ordering::SeqCst), 1);
        assert_eq!(mock.status_calls.load(Ordering::SeqCst), 3);
        assert_eq!(mock.result_calls.load(Ordering::SeqCst), 1);
        assert_eq!(mock.download_calls.load(Ordering::SeqCst), 1);

        queue.shutdown();
    }

    #[test]
    fn cancel_during_poll_resolves_to_cancelled() {
        // Mock that loops on IN_PROGRESS forever — test cancels mid-flight.
        let mut statuses = Vec::new();
        for _ in 0..1000 {
            statuses.push(Ok(json!({"status": "IN_PROGRESS"})));
        }
        let mock = Arc::new(MockHttp::new(
            vec![Ok(submit_resp())],
            statuses,
            vec![Ok(final_resp())],
        ));

        let provider = SeedanceProvider::with_http(
            SeedanceEndpoint::ImageToVideo,
            "test-key",
            mock.clone() as Arc<dyn FalHttp>,
            fast_config(),
        );

        let queue = JobQueue::new(jobs_config_no_persist()).unwrap();
        queue.register_provider(provider);

        let id = queue
            .submit(
                kinds::IMAGE_TO_VIDEO,
                json!({"prompt": "x", "image_url": "https://a/b.png"}),
            )
            .unwrap();

        // Wait until we're in AwaitingProvider, then cancel.
        assert!(poll_until(Duration::from_secs(2), || queue
            .get(id)
            .map(|j| j.state == JobState::AwaitingProvider)
            .unwrap_or(false)));
        queue.cancel(id);
        assert!(poll_until(Duration::from_secs(2), || queue
            .get(id)
            .map(|j| j.state == JobState::Cancelled)
            .unwrap_or(false)));

        // Download must NOT have run.
        assert_eq!(mock.download_calls.load(Ordering::SeqCst), 0);
        queue.shutdown();
    }

    #[test]
    fn submit_failure_marks_job_failed() {
        let mock = Arc::new(MockHttp::new(
            vec![Err("fal: 401 invalid api key".into())],
            vec![],
            vec![],
        ));

        let provider = SeedanceProvider::with_http(
            SeedanceEndpoint::ImageToVideo,
            "bad-key",
            mock.clone() as Arc<dyn FalHttp>,
            fast_config(),
        );

        let queue = JobQueue::new(jobs_config_no_persist()).unwrap();
        queue.register_provider(provider);

        let id = queue
            .submit(kinds::IMAGE_TO_VIDEO, json!({"prompt": "x", "image_url": "https://a/b.png"}))
            .unwrap();
        assert!(poll_until(Duration::from_secs(2), || queue
            .get(id)
            .map(|j| j.state == JobState::Failed)
            .unwrap_or(false)));
        let job = queue.get(id).unwrap();
        assert!(job.error.unwrap_or_default().contains("invalid api key"));
        queue.shutdown();
    }

    #[test]
    fn unexpected_status_aborts_with_provider_error() {
        let mock = Arc::new(MockHttp::new(
            vec![Ok(submit_resp())],
            vec![Ok(json!({"status": "GREMLIN"}))],
            vec![],
        ));

        let provider = SeedanceProvider::with_http(
            SeedanceEndpoint::ImageToVideo,
            "k",
            mock.clone() as Arc<dyn FalHttp>,
            fast_config(),
        );
        let queue = JobQueue::new(jobs_config_no_persist()).unwrap();
        queue.register_provider(provider);
        let id = queue
            .submit(kinds::IMAGE_TO_VIDEO, json!({"prompt": "x", "image_url": "https://a/b.png"}))
            .unwrap();
        assert!(poll_until(Duration::from_secs(2), || queue
            .get(id)
            .map(|j| j.state == JobState::Failed)
            .unwrap_or(false)));
        let job = queue.get(id).unwrap();
        assert!(job.error.unwrap_or_default().contains("GREMLIN"));
        queue.shutdown();
    }

    #[test]
    fn missing_status_url_in_submit_response_fails_fast() {
        let mock = Arc::new(MockHttp::new(
            vec![Ok(json!({"request_id": "x"}))],
            vec![],
            vec![],
        ));
        let provider = SeedanceProvider::with_http(
            SeedanceEndpoint::ImageToVideo,
            "k",
            mock as Arc<dyn FalHttp>,
            fast_config(),
        );
        let queue = JobQueue::new(jobs_config_no_persist()).unwrap();
        queue.register_provider(provider);
        let id = queue
            .submit(kinds::IMAGE_TO_VIDEO, json!({"prompt": "x", "image_url": "https://a/b.png"}))
            .unwrap();
        assert!(poll_until(Duration::from_secs(2), || queue
            .get(id)
            .map(|j| j.state == JobState::Failed)
            .unwrap_or(false)));
        queue.shutdown();
    }

    #[test]
    fn listener_sees_progress_events_during_poll() {
        let mock = Arc::new(MockHttp::new(
            vec![Ok(submit_resp())],
            vec![
                Ok(json!({"status": "IN_QUEUE", "queue_position": 2})),
                Ok(json!({"status": "COMPLETED"})),
            ],
            vec![Ok(final_resp())],
        ));
        let provider = SeedanceProvider::with_http(
            SeedanceEndpoint::ImageToVideo,
            "k",
            mock as Arc<dyn FalHttp>,
            fast_config(),
        );
        let queue = JobQueue::new(jobs_config_no_persist()).unwrap();
        queue.register_provider(provider);

        let progress_count = Arc::new(AtomicUsize::new(0));
        let pc = Arc::clone(&progress_count);
        queue.subscribe(move |ev| {
            if matches!(ev, JobEvent::Progress(_, _)) {
                pc.fetch_add(1, Ordering::Relaxed);
            }
        });

        let id = queue
            .submit(kinds::IMAGE_TO_VIDEO, json!({"prompt": "x", "image_url": "https://a/b.png"}))
            .unwrap();
        assert!(poll_until(Duration::from_secs(2), || queue
            .get(id)
            .map(|j| j.state == JobState::Complete)
            .unwrap_or(false)));
        assert!(progress_count.load(Ordering::Relaxed) >= 1);
        queue.shutdown();
    }
}
