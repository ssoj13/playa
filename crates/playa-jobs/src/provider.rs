//! Provider trait + execution context.
//!
//! Provider implementations live in vendor-specific sibling crates
//! (`playa-job-seedance`, `playa-job-ffmpeg`); this crate intentionally has no
//! HTTP-client dep so each provider can pick its own (`ureq`, `reqwest`, …).

use std::path::PathBuf;
use std::sync::mpsc;

use crate::cancel::CancelToken;
use crate::job::{JobError, JobId, JobProgress, JobState};

/// What a provider sees while running. Constructed by the queue per job; the
/// [`JobContext::set_state`] / [`JobContext::update`] methods flow through a
/// channel back to the queue's updater thread, which writes the job map and
/// emits [`crate::JobEvent`]s to subscribers.
pub struct JobContext {
    pub job_id: JobId,
    pub cancel: CancelToken,
    /// Per-job staging directory. The queue created it before invoking the
    /// provider; safe to write any artifacts here. Cleaned up on terminal
    /// state by the queue.
    pub files_dir: PathBuf,
    pub(crate) update_tx: mpsc::Sender<UpdateMsg>,
}

impl JobContext {
    /// Move the job into a new lifecycle state. Non-blocking; the queue's
    /// updater thread applies the change asynchronously.
    pub fn set_state(&self, state: JobState) {
        let _ = self.update_tx.send(UpdateMsg::State(self.job_id, state));
    }

    /// Push a free-form progress record (stage label, optional fraction).
    /// Multiple updates per job are fine.
    pub fn update(&self, progress: JobProgress) {
        let _ = self
            .update_tx
            .send(UpdateMsg::Progress(self.job_id, progress));
    }

    /// Persist a key/value pair into the job's `params` JSON object so a
    /// crash-restart sees enough context to resume (e.g. a Seedance
    /// `task_id` written **before** the `Submitting → AwaitingProvider`
    /// transition — otherwise a crash mid-submit leaks API credits).
    pub fn persist_param(&self, key: impl Into<String>, value: serde_json::Value) {
        let _ = self
            .update_tx
            .send(UpdateMsg::ParamPatch(self.job_id, key.into(), value));
    }
}

/// Trait every long-running task implements. Synchronous: the provider is
/// already running on a dedicated worker thread, so `std::thread::sleep` and
/// blocking HTTP calls are fine.
pub trait JobProvider: Send + Sync + 'static {
    /// Stable identifier used by [`crate::JobQueue::submit`] to look up the
    /// provider. Conventionally `"vendor.kind"`, e.g. `"seedance.video"`.
    fn kind(&self) -> &'static str;

    /// Execute one job to completion. Return `Ok(value)` for success (placed
    /// in [`crate::Job::result`]); return `Err` to mark the job failed (or
    /// cancelled if the cancel token tripped).
    fn run(
        &self,
        ctx: &JobContext,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, JobError>;

    /// Hook invoked by [`crate::JobQueue::replay_persisted`] when a previously
    /// in-flight job is found in the persistence log. Default implementation
    /// re-runs from scratch — override when the provider can salvage partial
    /// state via fields the job persisted into `params` (e.g. Seedance reads
    /// `params["task_id"]` and skips straight to polling).
    fn resume(
        &self,
        ctx: &JobContext,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, JobError> {
        self.run(ctx, params)
    }
}

// -----------------------------------------------------------------------------
// Internal: messages that JobContext sends to the updater thread.
// -----------------------------------------------------------------------------

pub(crate) enum UpdateMsg {
    State(JobId, JobState),
    Progress(JobId, JobProgress),
    /// Set `params[key] = value` so a crash-restart sees the new value.
    ParamPatch(JobId, String, serde_json::Value),
    /// Final outcome from a worker thread. Carries either a success value or
    /// an error, plus the job id. Always followed by a terminal state write.
    Final(JobId, Result<serde_json::Value, JobError>),
    /// Stop the updater thread on queue shutdown.
    Shutdown,
}

#[cfg(test)]
mod tests {
    // Provider trait is tested via the queue integration tests; nothing
    // standalone to assert here without exercising the queue.
}
