//! [`InpaintProvider`] implementation. Plug into
//! [`playa_jobs_core::JobQueue`] via
//! [`playa_jobs_core::JobQueue::register_provider`].

use std::sync::Arc;
use std::time::Duration;

use playa_jobs_core::{JobContext, JobError, JobProgress, JobProvider, JobState};
use serde_json::{Value, json};

use crate::http::{FalHttp, UreqFalHttp};

/// Stable kind strings used by callers when submitting via
/// [`playa_jobs_core::JobQueue::submit`].
pub mod kinds {
    pub const FLUX_PRO_V1_1_INPAINTING: &str = "inpaint.flux_pro_v1_1";
}

/// Which inpaint endpoint a [`InpaintProvider`] instance targets. v1
/// ships Flux Pro v1.1 only; the enum lives as a single variant so we
/// can grow it (Runway gen-fill, SD inpaint variants, seedream inpaint)
/// without breaking the public API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InpaintEndpoint {
    /// `fal-ai/flux-pro/v1.1/inpainting` — prompt-driven mask
    /// replacement, $0.05/MP at time of writing.
    FluxProV1_1,
}

impl InpaintEndpoint {
    pub const fn submit_url(self) -> &'static str {
        match self {
            Self::FluxProV1_1 => "https://queue.fal.run/fal-ai/flux-pro/v1.1/inpainting",
        }
    }

    pub const fn kind(self) -> &'static str {
        match self {
            Self::FluxProV1_1 => kinds::FLUX_PRO_V1_1_INPAINTING,
        }
    }

    /// Per-megapixel rate in USD. Used by
    /// [`InpaintProvider::estimate_cost_usd`] when params expose
    /// `image_size` / `width` × `height`.
    pub const fn cost_per_megapixel_usd(self) -> f64 {
        match self {
            Self::FluxProV1_1 => 0.05_f64,
        }
    }
}

/// Configuration knobs. Tests tighten `poll_interval` for sub-second
/// runs; production sticks with defaults.
#[derive(Debug, Clone)]
pub struct InpaintProviderConfig {
    pub poll_interval: Duration,
    /// Stop polling after this many ticks (safety net against a stuck
    /// queue position). 5 s × 1440 = 2 hours — well past fal's
    /// published max wait.
    pub max_poll_ticks: usize,
}

impl Default for InpaintProviderConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(5),
            max_poll_ticks: 1440,
        }
    }
}

pub struct InpaintProvider {
    api_key: String,
    http: Arc<dyn FalHttp>,
    config: InpaintProviderConfig,
    endpoint: InpaintEndpoint,
}

impl InpaintProvider {
    /// Production constructor.
    pub fn new(endpoint: InpaintEndpoint, api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            http: Arc::new(UreqFalHttp::new()),
            config: InpaintProviderConfig::default(),
            endpoint,
        }
    }

    /// Shortcut: the canonical v1 endpoint.
    pub fn flux_pro_v1_1(api_key: impl Into<String>) -> Self {
        Self::new(InpaintEndpoint::FluxProV1_1, api_key)
    }

    /// Test / advanced constructor. Inject a [`FalHttp`] mock and a
    /// tight poll interval so unit tests run in milliseconds.
    pub fn with_http(
        endpoint: InpaintEndpoint,
        api_key: impl Into<String>,
        http: Arc<dyn FalHttp>,
        config: InpaintProviderConfig,
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

        ctx.cancel.check_err()?;
        let result = self
            .http
            .get_json(response_url, &self.api_key)
            .map_err(JobError::Provider)?;

        // Flux inpaint response: { "images": [{ "url": ..., "width":,
        // "height": }], "seed": ..., "prompt": ..., ... }
        let image_url = result
            .pointer("/images/0/url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                JobError::Provider(format!(
                    "fal response missing images[0].url (got: {})",
                    truncate(&result.to_string(), 200)
                ))
            })?
            .to_string();

        ctx.set_state(JobState::Downloading);
        ctx.cancel.check_err()?;

        let png_path = ctx.files_dir.join("output.png");
        let bytes = self
            .http
            .download(&image_url, &png_path)
            .map_err(JobError::Io)?;
        log::info!(
            "Inpaint: downloaded {bytes} bytes to {}",
            png_path.display()
        );

        ctx.set_state(JobState::Staging);

        Ok(json!({
            "png_path": png_path.to_string_lossy(),
            "image_url": image_url,
            "bytes": bytes,
            "fal_response": result,
        }))
    }

    /// Best-effort post-completion cost reporter. Reads `width` and
    /// `height` from the original params, computes megapixels, multiplies
    /// by the endpoint's per-MP rate. Skips silently when dims are absent
    /// (the queue's accounting is best-effort and the user sees the
    /// final billed cost via the active Generation record in playa-app).
    fn report_cost_from_params(&self, ctx: &JobContext, params: &Value) {
        if let Some(mp) = megapixels_from_params(params) {
            let usd = mp * self.endpoint.cost_per_megapixel_usd();
            ctx.report_cost(usd);
        }
    }
}

impl JobProvider for InpaintProvider {
    fn kind(&self) -> &'static str {
        self.endpoint.kind()
    }

    fn estimate_cost_usd(&self, params: &Value) -> Option<f64> {
        let mp = megapixels_from_params(params)?;
        Some(mp * self.endpoint.cost_per_megapixel_usd())
    }

    fn run(&self, ctx: &JobContext, params: Value) -> Result<Value, JobError> {
        ctx.set_state(JobState::Submitting);
        ctx.cancel.check_err()?;

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

        // Persist URLs BEFORE state transition so crash mid-submit
        // doesn't leak API billing.
        ctx.persist_param("request_id", json!(request_id));
        ctx.persist_param("status_url", json!(status_url));
        ctx.persist_param("response_url", json!(response_url));

        ctx.set_state(JobState::AwaitingProvider);
        let result =
            self.poll_until_complete_then_download(ctx, &status_url, &response_url)?;
        self.report_cost_from_params(ctx, &params);
        Ok(result)
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
                    "Inpaint: resuming job, status_url={status_url} response_url={response_url}"
                );
                ctx.set_state(JobState::AwaitingProvider);
                let result =
                    self.poll_until_complete_then_download(ctx, &status_url, &response_url)?;
                self.report_cost_from_params(ctx, &params);
                Ok(result)
            }
            _ => {
                log::warn!("Inpaint: resume called without persisted URLs — re-submitting");
                self.run(ctx, params)
            }
        }
    }
}

/// Pull megapixels from params. Recognises two shapes:
/// - `{"width": <u64>, "height": <u64>}` — explicit pixels
/// - `{"image_size": "1024x1024"}` — fal-style string
/// Returns `None` if neither is present.
fn megapixels_from_params(params: &Value) -> Option<f64> {
    if let (Some(w), Some(h)) = (
        params.get("width").and_then(|v| v.as_u64()),
        params.get("height").and_then(|v| v.as_u64()),
    ) {
        return Some((w * h) as f64 / 1_000_000.0);
    }
    if let Some(s) = params.get("image_size").and_then(|v| v.as_str())
        && let Some((w, h)) = s.split_once('x')
        && let (Ok(w), Ok(h)) = (w.parse::<u64>(), h.parse::<u64>())
    {
        return Some((w * h) as f64 / 1_000_000.0);
    }
    None
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}…", &s[..n])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Scripted mock — sibling-pattern to seedance's MockHttp.
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
            std::fs::write(dest, b"fake-png-payload").map_err(|e| e.to_string())?;
            Ok(16)
        }
    }

    #[test]
    fn kind_string_stable() {
        let p = InpaintProvider::flux_pro_v1_1("k");
        assert_eq!(p.kind(), kinds::FLUX_PRO_V1_1_INPAINTING);
        assert_eq!(p.kind(), "inpaint.flux_pro_v1_1");
    }

    #[test]
    fn submit_url_is_canonical_flux_pro_inpainting_endpoint() {
        assert_eq!(
            InpaintEndpoint::FluxProV1_1.submit_url(),
            "https://queue.fal.run/fal-ai/flux-pro/v1.1/inpainting"
        );
    }

    #[test]
    fn megapixels_from_width_height() {
        let v = json!({"width": 1024_u64, "height": 1024_u64});
        assert!((megapixels_from_params(&v).unwrap() - 1.048576).abs() < 1e-9);
    }

    #[test]
    fn megapixels_from_image_size_string() {
        let v = json!({"image_size": "1024x1024"});
        assert!((megapixels_from_params(&v).unwrap() - 1.048576).abs() < 1e-9);
        let v2 = json!({"image_size": "1920x1080"});
        assert!((megapixels_from_params(&v2).unwrap() - 2.0736).abs() < 1e-9);
    }

    #[test]
    fn megapixels_absent_returns_none() {
        let v = json!({"prompt": "hi"});
        assert!(megapixels_from_params(&v).is_none());
    }

    #[test]
    fn estimate_cost_usd_uses_per_megapixel_rate() {
        let p = InpaintProvider::flux_pro_v1_1("k");
        let cost = p
            .estimate_cost_usd(&json!({"width": 1024_u64, "height": 1024_u64}))
            .unwrap();
        // 1.048576 MP × $0.05/MP ≈ $0.0524
        assert!((cost - 0.052_428_8).abs() < 1e-5, "got {cost}");
    }

    #[test]
    fn estimate_cost_usd_returns_none_when_dims_absent() {
        let p = InpaintProvider::flux_pro_v1_1("k");
        assert!(p.estimate_cost_usd(&json!({"prompt": "hi"})).is_none());
    }

    /// Smoke test: full submit→poll→download path against MockHttp.
    /// The provider state machine must observe one POST, two status
    /// polls (IN_PROGRESS → COMPLETED), one result GET, and one
    /// download.
    #[test]
    fn happy_path_mock_submit_poll_complete_download() {
        use playa_jobs_core::{Job, JobQueue, JobQueueConfig};
        use std::sync::Arc as A;

        let post = vec![Ok(json!({
            "request_id": "req_123",
            "status_url": "https://queue.fal.run/req_123/status",
            "response_url": "https://queue.fal.run/req_123",
            "cancel_url": "https://queue.fal.run/req_123/cancel",
        }))];
        let statuses = vec![
            Ok(json!({"status": "IN_PROGRESS"})),
            Ok(json!({"status": "COMPLETED"})),
        ];
        let results = vec![Ok(json!({
            "images": [{"url": "https://fal.media/files/abc.png", "width": 1024, "height": 1024}],
            "seed": 42_u64,
            "prompt": "a wolf",
        }))];
        let mock = A::new(MockHttp::new(post, statuses, results));
        let provider = InpaintProvider::with_http(
            InpaintEndpoint::FluxProV1_1,
            "k",
            mock.clone(),
            InpaintProviderConfig {
                poll_interval: Duration::from_millis(5),
                max_poll_ticks: 10,
            },
        );

        // Real-ish JobContext requires a JobQueue. Spin a queue, register
        // the provider, submit a dummy job, await completion. Mirrors
        // seedance's integration tests.
        let bus = A::new(playa_jobs_core::EventBus::new());
        let cfg = JobQueueConfig {
            thread_count: 2,
            files_dir: std::env::temp_dir()
                .join(format!("playa-inpaint-test-{}", uuid::Uuid::new_v4())),
            persist_path: None,
        };
        let queue = JobQueue::new(cfg, A::clone(&bus)).unwrap();
        queue.register_provider(provider);

        let id = queue
            .submit(
                kinds::FLUX_PRO_V1_1_INPAINTING,
                json!({
                    "image_url": "data:image/png;base64,iVBORw0K...",
                    "mask_url":  "data:image/png;base64,iVBORw0K...",
                    "prompt":    "a wolf",
                    "seed":      42_u64,
                    "width":     1024_u64,
                    "height":    1024_u64,
                }),
            )
            .unwrap();

        // Poll for completion.
        use std::time::Instant;
        let start = Instant::now();
        let done = loop {
            if let Some(Job { state, .. }) = queue.get(id) {
                if state.is_terminal() {
                    break state == playa_jobs_core::JobState::Complete;
                }
            }
            if start.elapsed() > Duration::from_secs(3) {
                break false;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(done, "job did not reach Complete within 3s");

        assert_eq!(mock.post_calls.load(Ordering::SeqCst), 1);
        assert_eq!(mock.status_calls.load(Ordering::SeqCst), 2);
        assert_eq!(mock.result_calls.load(Ordering::SeqCst), 1);
        assert_eq!(mock.download_calls.load(Ordering::SeqCst), 1);

        // Verify result schema.
        let job = queue.get(id).unwrap();
        let result = job.result.expect("result envelope");
        assert!(result.get("png_path").is_some());
        assert_eq!(result.get("bytes").and_then(|v| v.as_u64()), Some(16));
        queue.shutdown();
    }
}
