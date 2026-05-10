//! Core job data: id, state, progress, error, persisted record.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stable identifier minted on [`crate::JobQueue::submit`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JobId(pub Uuid);

impl JobId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for JobId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for JobId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Lifecycle state machine. Terminal states (`Complete`, `Failed`, `Cancelled`)
/// never transition further — workers leave the job alone after writing one of
/// those.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum JobState {
    /// Submitted, awaiting a worker thread.
    Pending,
    /// Worker is sending the request to the provider (e.g. POST to API).
    Submitting,
    /// Provider accepted the request; we are polling for completion (e.g.
    /// Seedance has a `task_id`, we poll status every 15 s).
    AwaitingProvider,
    /// Provider returned a result URL; we are streaming bytes to the local
    /// files directory.
    Downloading,
    /// Final placement step (move, attach to layer, …).
    Staging,
    Complete,
    Failed,
    Cancelled,
}

impl JobState {
    #[inline]
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Complete | Self::Failed | Self::Cancelled)
    }

    /// True for states that, on a clean restart with persistence enabled,
    /// should re-enter the work queue (provider gets a chance to resume).
    #[inline]
    pub fn is_resumable(self) -> bool {
        matches!(
            self,
            Self::Pending | Self::Submitting | Self::AwaitingProvider | Self::Downloading
        )
    }
}

/// Free-form progress record. `fraction` is `Some(0.0..=1.0)` when the
/// provider can express a meaningful proportion, else `None` (busy spinner).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobProgress {
    pub stage: String,
    pub fraction: Option<f32>,
    pub message: Option<String>,
}

/// Boxed error used through the [`crate::JobProvider::run`] return type.
#[derive(Debug, Clone)]
pub enum JobError {
    Cancelled,
    Provider(String),
    Io(String),
    Serde(String),
    UnknownProvider(String),
}

impl std::fmt::Display for JobError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => write!(f, "job cancelled"),
            Self::Provider(s) => write!(f, "provider error: {s}"),
            Self::Io(s) => write!(f, "io error: {s}"),
            Self::Serde(s) => write!(f, "serialization error: {s}"),
            Self::UnknownProvider(k) => write!(f, "unknown provider kind: {k}"),
        }
    }
}

impl std::error::Error for JobError {}

/// Snapshot of a job. Cloned out of the queue's internal map by the public
/// reads — callers never get a `&Job` because they would block writers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: JobId,
    pub kind: String,
    pub state: JobState,
    pub progress: Option<JobProgress>,
    pub error: Option<String>,
    pub params: serde_json::Value,
    /// Final value returned by [`crate::JobProvider::run`] on
    /// [`JobState::Complete`]. `None` until then.
    pub result: Option<serde_json::Value>,
    /// Seconds since the Unix epoch (kept as `u64` so the persisted form does
    /// not embed a non-portable `SystemTime`).
    pub created_at: u64,
    pub updated_at: u64,
}

impl Job {
    pub(crate) fn new(kind: impl Into<String>, params: serde_json::Value) -> Self {
        let now = now_secs();
        Self {
            id: JobId::new(),
            kind: kind.into(),
            state: JobState::Pending,
            progress: None,
            error: None,
            params,
            result: None,
            created_at: now,
            updated_at: now,
        }
    }

    pub(crate) fn touch(&mut self) {
        self.updated_at = now_secs();
    }
}

#[inline]
pub(crate) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_states_classification() {
        assert!(JobState::Complete.is_terminal());
        assert!(JobState::Failed.is_terminal());
        assert!(JobState::Cancelled.is_terminal());
        assert!(!JobState::Pending.is_terminal());
        assert!(!JobState::AwaitingProvider.is_terminal());
    }

    #[test]
    fn resumable_excludes_terminal_and_staging() {
        assert!(JobState::Pending.is_resumable());
        assert!(JobState::Submitting.is_resumable());
        assert!(JobState::AwaitingProvider.is_resumable());
        assert!(JobState::Downloading.is_resumable());
        // Staging is the local placement step — re-running it on restart could
        // duplicate side effects (e.g. layer attached twice). Excluded.
        assert!(!JobState::Staging.is_resumable());
        assert!(!JobState::Complete.is_resumable());
    }

    #[test]
    fn job_id_unique() {
        let a = JobId::new();
        let b = JobId::new();
        assert_ne!(a, b);
    }

    #[test]
    fn job_serializes_round_trip() {
        let j = Job::new("seedance.video", serde_json::json!({"prompt": "test"}));
        let s = serde_json::to_string(&j).unwrap();
        let back: Job = serde_json::from_str(&s).unwrap();
        assert_eq!(back.id, j.id);
        assert_eq!(back.kind, j.kind);
        assert_eq!(back.state, j.state);
    }

    #[test]
    fn job_error_display() {
        assert_eq!(JobError::Cancelled.to_string(), "job cancelled");
        assert_eq!(
            JobError::UnknownProvider("foo".into()).to_string(),
            "unknown provider kind: foo"
        );
    }
}
