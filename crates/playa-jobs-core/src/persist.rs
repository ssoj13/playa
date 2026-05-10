//! Append-only JSONL persistence log.
//!
//! Writes one entry per state-touching operation. On boot,
//! [`Log::replay_to_jobs`] folds the entries into the latest in-memory state
//! per job. Tombstones (after terminal completion) garbage-collect entries.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::job::{Job, JobId, JobProgress, JobState, now_secs};

/// Adjacently tagged because (a) `Job.kind` exists and would collide with an
/// internally-tagged discriminator, (b) `Tombstone(JobId)` is a newtype around
/// a non-struct type which serde's internal tagging rejects.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum LogEntry {
    /// Full job snapshot at submit time.
    Created(Job),
    /// Incremental state change.
    Updated {
        id: JobId,
        state: JobState,
        progress: Option<JobProgress>,
        result: Option<serde_json::Value>,
        error: Option<String>,
        updated_at: u64,
    },
    /// Single-key params patch (used to write a Seedance `task_id` before the
    /// `Submitting → AwaitingProvider` transition so a crash never loses it).
    ParamPatch {
        id: JobId,
        key: String,
        value: serde_json::Value,
    },
    /// Tombstone: forget this job on next replay.
    Tombstone(JobId),
    /// Compact "state transition" event written by the updater thread on
    /// every state change. Replay folds these into `Job.state_history`. Much
    /// smaller than a full `Updated` snapshot for the common case of "job
    /// just stepped from Submitting to AwaitingProvider".
    StageEntered {
        id: JobId,
        state: JobState,
        at: u64,
    },
    /// Cost reported by the provider (e.g. `0.3024 * duration_secs` for
    /// Seedance). Replay folds into `Job.cost_usd`.
    Cost {
        id: JobId,
        usd: f64,
        at: u64,
    },
}

pub struct Log {
    path: PathBuf,
    file: Mutex<BufWriter<File>>,
}

impl Log {
    /// Open (creating if missing) the log at `path`. Parent directory is
    /// created as well.
    pub fn open(path: PathBuf) -> std::io::Result<Self> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        Ok(Self {
            path,
            file: Mutex::new(BufWriter::new(file)),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append one entry and `flush()` so a process crash immediately afterwards
    /// keeps the entry on disk.
    pub fn append(&self, entry: &LogEntry) -> std::io::Result<()> {
        let line = serde_json::to_string(entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let mut guard = self.file.lock().unwrap_or_else(|e| e.into_inner());
        writeln!(guard, "{line}")?;
        guard.flush()?;
        Ok(())
    }

    /// Read every entry verbatim. Used by tests; production callers use
    /// [`Self::replay_to_jobs`].
    pub fn entries(path: &Path) -> std::io::Result<Vec<LogEntry>> {
        let file = match File::open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let reader = BufReader::new(file);
        let mut out = Vec::new();
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let entry: LogEntry = serde_json::from_str(&line)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            out.push(entry);
        }
        Ok(out)
    }

    /// Fold the log into the latest snapshot per job.
    pub fn replay_to_jobs(path: &Path) -> std::io::Result<HashMap<JobId, Job>> {
        let mut jobs: HashMap<JobId, Job> = HashMap::new();
        for entry in Self::entries(path)? {
            match entry {
                LogEntry::Created(j) => {
                    jobs.insert(j.id, j);
                }
                LogEntry::Updated {
                    id,
                    state,
                    progress,
                    result,
                    error,
                    updated_at,
                } => {
                    if let Some(j) = jobs.get_mut(&id) {
                        j.state = state;
                        j.progress = progress;
                        j.result = result;
                        j.error = error;
                        j.updated_at = updated_at;
                    }
                }
                LogEntry::ParamPatch { id, key, value } => {
                    if let Some(j) = jobs.get_mut(&id)
                        && let Some(obj) = j.params.as_object_mut()
                    {
                        obj.insert(key, value);
                        j.updated_at = now_secs();
                    }
                }
                LogEntry::Tombstone(id) => {
                    jobs.remove(&id);
                }
                LogEntry::StageEntered { id, state, at } => {
                    if let Some(j) = jobs.get_mut(&id) {
                        j.state = state;
                        j.updated_at = at;
                        if j.state_history.last().map(|(s, _)| *s) != Some(state) {
                            j.state_history.push((state, at));
                        }
                    }
                }
                LogEntry::Cost { id, usd, at } => {
                    if let Some(j) = jobs.get_mut(&id) {
                        j.cost_usd = Some(usd);
                        j.updated_at = at;
                    }
                }
            }
        }
        Ok(jobs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn tmp_log_path(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("playa-jobs-test-{tag}-{nanos}.jsonl"))
    }

    #[test]
    fn append_and_entries_round_trip() {
        let path = tmp_log_path("rt");
        let _ = std::fs::remove_file(&path);
        let log = Log::open(path.clone()).unwrap();
        let job = Job::new("dummy", serde_json::json!({}));
        log.append(&LogEntry::Created(job.clone())).unwrap();
        log.append(&LogEntry::Updated {
            id: job.id,
            state: JobState::Complete,
            progress: None,
            result: Some(serde_json::json!({"ok": true})),
            error: None,
            updated_at: now_secs(),
        })
        .unwrap();
        drop(log);

        let entries = Log::entries(&path).unwrap();
        assert_eq!(entries.len(), 2);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn replay_collapses_to_latest_state() {
        let path = tmp_log_path("rep");
        let _ = std::fs::remove_file(&path);
        let log = Log::open(path.clone()).unwrap();
        let mut job = Job::new("seedance.video", serde_json::json!({"prompt": "p"}));
        let id = job.id;
        log.append(&LogEntry::Created(job.clone())).unwrap();
        job.state = JobState::Submitting;
        job.touch();
        log.append(&LogEntry::Updated {
            id,
            state: job.state,
            progress: None,
            result: None,
            error: None,
            updated_at: job.updated_at,
        })
        .unwrap();
        log.append(&LogEntry::ParamPatch {
            id,
            key: "task_id".into(),
            value: serde_json::Value::String("abc-123".into()),
        })
        .unwrap();
        drop(log);

        let jobs = Log::replay_to_jobs(&path).unwrap();
        let restored = jobs.get(&id).unwrap();
        assert_eq!(restored.state, JobState::Submitting);
        assert_eq!(
            restored.params.get("task_id").and_then(|v| v.as_str()),
            Some("abc-123")
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn tombstone_removes_job() {
        let path = tmp_log_path("tomb");
        let _ = std::fs::remove_file(&path);
        let log = Log::open(path.clone()).unwrap();
        let job = Job::new("dummy", serde_json::json!({}));
        let id = job.id;
        log.append(&LogEntry::Created(job)).unwrap();
        log.append(&LogEntry::Tombstone(id)).unwrap();
        drop(log);

        let jobs = Log::replay_to_jobs(&path).unwrap();
        assert!(jobs.is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn replay_reconstructs_state_history_from_stage_entries() {
        let path = tmp_log_path("hist");
        let _ = std::fs::remove_file(&path);
        let log = Log::open(path.clone()).unwrap();
        let mut job = Job::new("seedance.text_to_video", serde_json::json!({}));
        let id = job.id;
        let created_at = job.created_at;
        log.append(&LogEntry::Created(job.clone())).unwrap();

        // Append three stage transitions.
        for (state, offset) in [
            (JobState::Submitting, 1),
            (JobState::AwaitingProvider, 2),
            (JobState::Complete, 3),
        ] {
            log.append(&LogEntry::StageEntered {
                id,
                state,
                at: created_at + offset,
            })
            .unwrap();
            job.state = state;
        }
        // Cost entry too.
        log.append(&LogEntry::Cost {
            id,
            usd: 9.99,
            at: created_at + 4,
        })
        .unwrap();
        drop(log);

        let jobs = Log::replay_to_jobs(&path).unwrap();
        let restored = jobs.get(&id).unwrap();
        assert_eq!(restored.state, JobState::Complete);
        let states: Vec<JobState> = restored.state_history.iter().map(|(s, _)| *s).collect();
        // Pending was already in the Created snapshot's state_history; replay
        // appended Submitting / AwaitingProvider / Complete on top.
        assert!(states.contains(&JobState::Pending));
        assert!(states.contains(&JobState::Submitting));
        assert!(states.contains(&JobState::AwaitingProvider));
        assert!(states.contains(&JobState::Complete));
        assert_eq!(restored.cost_usd, Some(9.99));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn missing_file_replays_empty() {
        let path = tmp_log_path("missing");
        let _ = std::fs::remove_file(&path);
        let jobs = Log::replay_to_jobs(&path).unwrap();
        assert!(jobs.is_empty());
    }
}
