//! HTTP client trait + the `ureq` production implementation.
//!
//! Splits the trait so tests can script the full provider state machine
//! against an in-memory mock — no network, no API key. Same shape as
//! `playa-job-seedance::http`. Duplicated rather than shared: provider
//! crates are intentionally self-contained; a future refactor can lift
//! the trait into a shared `playa-fal-http` crate if more providers
//! land.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

/// Minimal HTTP surface the provider needs. Errors are stringified — the
/// provider wraps them into [`playa_jobs_core::JobError::Provider`] /
/// [`playa_jobs_core::JobError::Io`] depending on which call site failed.
pub trait FalHttp: Send + Sync + 'static {
    fn post_json(&self, url: &str, fal_key: &str, body: &Value) -> Result<Value, String>;
    fn get_json(&self, url: &str, fal_key: &str) -> Result<Value, String>;
    /// Stream the response body to `dest`, returning bytes copied. Caller
    /// guarantees `dest`'s parent directory exists.
    fn download(&self, url: &str, dest: &Path) -> Result<u64, String>;
}

/// Production `ureq` + `rustls` client. Sync, blocking, lives happily on a
/// `playa-jobs` worker thread without dragging tokio into the engine.
pub struct UreqFalHttp {
    agent: ureq::Agent,
}

impl UreqFalHttp {
    pub fn new() -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(30))
            .timeout_read(Duration::from_secs(300))
            .timeout_write(Duration::from_secs(300))
            .build();
        Self { agent }
    }
}

impl Default for UreqFalHttp {
    fn default() -> Self {
        Self::new()
    }
}

impl FalHttp for UreqFalHttp {
    fn post_json(&self, url: &str, fal_key: &str, body: &Value) -> Result<Value, String> {
        let resp = self
            .agent
            .post(url)
            .set("Authorization", &format!("Key {fal_key}"))
            .set("Content-Type", "application/json")
            .send_json(body.clone())
            .map_err(|e| format!("POST {url}: {e}"))?;
        resp.into_json::<Value>()
            .map_err(|e| format!("POST {url} body decode: {e}"))
    }

    fn get_json(&self, url: &str, fal_key: &str) -> Result<Value, String> {
        let resp = self
            .agent
            .get(url)
            .set("Authorization", &format!("Key {fal_key}"))
            .call()
            .map_err(|e| format!("GET {url}: {e}"))?;
        resp.into_json::<Value>()
            .map_err(|e| format!("GET {url} body decode: {e}"))
    }

    fn download(&self, url: &str, dest: &Path) -> Result<u64, String> {
        let resp = self
            .agent
            .get(url)
            .call()
            .map_err(|e| format!("GET {url}: {e}"))?;
        let mut reader = resp.into_reader();
        let mut file = std::fs::File::create(dest)
            .map_err(|e| format!("create {}: {e}", dest.display()))?;
        std::io::copy(&mut reader, &mut file).map_err(|e| format!("copy to file: {e}"))
    }
}

/// Convenience wrapper so the provider can take `Arc<dyn FalHttp>` without the
/// caller having to box manually.
pub fn arc_ureq() -> Arc<dyn FalHttp> {
    Arc::new(UreqFalHttp::new())
}
