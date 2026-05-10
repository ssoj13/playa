//! Events broadcast by [`crate::JobQueue`] to its subscribers. The queue stays
//! agnostic to any specific event-bus implementation — it accepts plain
//! `Box<dyn Fn(JobEvent) + Send + Sync>` listeners. App glue forwards into
//! `playa-events::EventBus` without coupling this crate to it.

use crate::job::{JobId, JobProgress, JobState};

#[derive(Debug, Clone)]
pub enum JobEvent {
    Created(JobId),
    StateChanged(JobId, JobState),
    Progress(JobId, JobProgress),
    Completed(JobId, serde_json::Value),
    Failed(JobId, String),
    Cancelled(JobId),
}

impl JobEvent {
    #[inline]
    pub fn job_id(&self) -> JobId {
        match self {
            Self::Created(id)
            | Self::StateChanged(id, _)
            | Self::Progress(id, _)
            | Self::Completed(id, _)
            | Self::Failed(id, _)
            | Self::Cancelled(id) => *id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_id_extraction_covers_every_variant() {
        let id = JobId::new();
        for ev in [
            JobEvent::Created(id),
            JobEvent::StateChanged(id, JobState::Pending),
            JobEvent::Progress(
                id,
                JobProgress {
                    stage: "x".into(),
                    fraction: None,
                    message: None,
                },
            ),
            JobEvent::Completed(id, serde_json::Value::Null),
            JobEvent::Failed(id, "boom".into()),
            JobEvent::Cancelled(id),
        ] {
            assert_eq!(ev.job_id(), id);
        }
    }
}
