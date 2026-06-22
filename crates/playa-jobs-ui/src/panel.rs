//! [`JobsPanel`] — a [`JobQueue`] view built on the reusable `egui-jobs-table`
//! widget.
//!
//! The generic widget renders the sortable/filterable rows (status badges,
//! progress bars, per-row action buttons). This module is the thin playa shell
//! around it: a "+ Generate" header that opens the submit dialog, a bulk action
//! bar over the table's multi-selection, and a footer with active/total/cost
//! stats. Jobs are queried via [`JobQueue::list`] each frame; for real-time
//! updates wire `JobQueue::subscribe(|_| ctx.request_repaint())` at boot.

use std::collections::HashMap;

use egui::Ui;
use egui_jobs_table::{JobAction, JobRow, JobStatus, JobsTable, JobsTableState};

use playa_jobs_core::{Job, JobId, JobQueue, JobState};

/// Action the panel emits per frame for the host to dispatch to queue methods.
#[derive(Debug, Clone, PartialEq)]
pub enum JobsAction {
    /// No action this frame.
    None,
    /// Host should call `JobQueue::cancel` for each id.
    Cancel(Vec<JobId>),
    /// Host should call `JobQueue::retry` for each id.
    Retry(Vec<JobId>),
    /// Host should call `JobQueue::remove` for each id.
    Delete(Vec<JobId>),
    /// Host should reveal the `mp4_path` recorded in the job's result.
    RevealMp4(JobId),
    /// Host should open [`crate::SubmitDialog`].
    OpenSubmit,
}

/// Table view of a [`JobQueue`]. Sort / filter / selection live in the
/// embedded [`JobsTableState`]; the panel owns no job state itself.
#[derive(Debug, Default)]
pub struct JobsPanel {
    /// Sort, filter and multi-selection owned by the egui-jobs-table widget.
    pub table: JobsTableState,
}

impl JobsPanel {
    pub fn new() -> Self {
        // Column 0 (Submitted) descending → newest-first, matching the old panel.
        Self {
            table: JobsTableState::new(),
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, queue: &JobQueue) -> JobsAction {
        let mut action = JobsAction::None;

        // Header — launch the submit dialog.
        ui.horizontal(|ui| {
            if ui.button("+ Generate").clicked() {
                action = JobsAction::OpenSubmit;
            }
        });
        ui.separator();

        // Snapshot the queue into the widget's flat row model. A stable u64 (the
        // low 64 bits of the JobId Uuid) keys selection across frames; `id_map`
        // recovers the full JobId for emitted actions. Collisions are negligible
        // for a job queue and only ever scope a single user click.
        let jobs = queue.list();
        let now = playa_jobs_core::job::now_secs();
        let mut id_map: HashMap<u64, JobId> = HashMap::with_capacity(jobs.len());
        let mut rows: Vec<JobRow> = Vec::with_capacity(jobs.len());
        for job in &jobs {
            let rid = job_row_id(job.id);
            id_map.insert(rid, job.id);
            rows.push(build_row(rid, job, now));
        }

        // Footer + bulk action bar over the table's multi-selection. The bulk
        // buttons resolve the selected row ids back to live jobs to gate which
        // actions are valid (only terminal jobs delete, only failed/cancelled
        // retry, only running cancel).
        let stats = compute_footer_stats(&jobs);
        egui::Panel::bottom("jobs_actions")
            .resizable(false)
            .min_size(28.0)
            .show_inside(ui, |ui| {
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.weak(format!(
                        "{} active · {} total · {:.2} USD est.",
                        stats.active_count, stats.total_count, stats.total_cost_usd,
                    ));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let selected: Vec<&Job> = jobs
                            .iter()
                            .filter(|j| self.table.is_selected(job_row_id(j.id)))
                            .collect();
                        let any_terminal = selected.iter().any(|j| j.state.is_terminal());
                        let any_non_terminal = selected.iter().any(|j| !j.state.is_terminal());
                        let any_failed_cancelled = selected
                            .iter()
                            .any(|j| matches!(j.state, JobState::Failed | JobState::Cancelled));

                        if ui
                            .add_enabled(any_terminal, egui::Button::new("Delete"))
                            .clicked()
                        {
                            action = JobsAction::Delete(
                                selected
                                    .iter()
                                    .filter(|j| j.state.is_terminal())
                                    .map(|j| j.id)
                                    .collect(),
                            );
                        }
                        if ui
                            .add_enabled(any_failed_cancelled, egui::Button::new("Retry"))
                            .clicked()
                        {
                            action = JobsAction::Retry(
                                selected
                                    .iter()
                                    .filter(|j| {
                                        matches!(j.state, JobState::Failed | JobState::Cancelled)
                                    })
                                    .map(|j| j.id)
                                    .collect(),
                            );
                        }
                        if ui
                            .add_enabled(any_non_terminal, egui::Button::new("Cancel"))
                            .clicked()
                        {
                            action = JobsAction::Cancel(
                                selected
                                    .iter()
                                    .filter(|j| !j.state.is_terminal())
                                    .map(|j| j.id)
                                    .collect(),
                            );
                        }
                        if !selected.is_empty() {
                            ui.label(format!("{} selected", selected.len()));
                        }
                    });
                });
            });

        // The generic table: filter toolbar + sortable rows + per-row actions.
        let headers = ["Submitted", "Elapsed", "Kind", "Size", "Detail"]
            .map(String::from)
            .to_vec();
        let table_actions = JobsTable::new(headers).show(ui, &rows, &mut self.table);

        // Header / bulk-bar action wins; otherwise the first per-row action.
        if matches!(action, JobsAction::None) {
            for act in table_actions {
                let mapped = act_to_jobs_action(act, &id_map);
                if !matches!(mapped, JobsAction::None) {
                    action = mapped;
                    break;
                }
            }
        }

        action
    }
}

/// Stable per-job table id: the low 64 bits of the JobId Uuid.
fn job_row_id(id: JobId) -> u64 {
    id.0.as_u128() as u64
}

/// Collapse playa's eight-state lifecycle onto the table's five status buckets.
fn map_state(state: JobState) -> JobStatus {
    match state {
        JobState::Pending => JobStatus::Queued,
        JobState::Submitting
        | JobState::AwaitingProvider
        | JobState::Downloading
        | JobState::Staging => JobStatus::Running,
        JobState::Complete => JobStatus::Done,
        JobState::Failed => JobStatus::Failed,
        JobState::Cancelled => JobStatus::Cancelled,
    }
}

/// Build one table row from a job. Columns line up with the headers
/// (Submitted / Elapsed / Kind / Size / Detail); status drives the badge and
/// the default per-row action set; a known progress fraction renders a bar.
fn build_row(rid: u64, job: &Job, now: u64) -> JobRow {
    let submitted = format_clock(job.created_at);
    let elapsed = format_elapsed(now.saturating_sub(job.created_at));
    let kind = job.kind.clone();
    let size = job
        .result
        .as_ref()
        .and_then(|v| v.get("bytes"))
        .and_then(|n| n.as_u64())
        .map(|b| format!("{:.1} MB", b as f64 / 1_048_576.0))
        .unwrap_or_default();
    // Detail = error message (if failed) else the latest progress stage/message.
    let detail = if let Some(err) = &job.error {
        err.clone()
    } else if let Some(p) = &job.progress {
        p.message.clone().unwrap_or_else(|| p.stage.clone())
    } else {
        String::new()
    };

    let status = map_state(job.state);
    let mut row = JobRow::new(rid, vec![submitted, elapsed, kind, size, detail], status);
    if status == JobStatus::Running
        && let Some(frac) = job.progress.as_ref().and_then(|p| p.fraction)
    {
        row = row.with_progress(frac);
    }
    row
}

/// Map a per-row [`JobAction`] back to the host-facing [`JobsAction`]
/// (single-id vecs). `Open` is playa's "reveal mp4"; `Select` is handled inside
/// the widget's state, so it produces no host action.
fn act_to_jobs_action(act: JobAction, id_map: &HashMap<u64, JobId>) -> JobsAction {
    match act {
        JobAction::Cancel { id } => id_map
            .get(&id)
            .map_or(JobsAction::None, |j| JobsAction::Cancel(vec![*j])),
        JobAction::Retry { id } => id_map
            .get(&id)
            .map_or(JobsAction::None, |j| JobsAction::Retry(vec![*j])),
        JobAction::Remove { id } => id_map
            .get(&id)
            .map_or(JobsAction::None, |j| JobsAction::Delete(vec![*j])),
        JobAction::Open { id } => id_map
            .get(&id)
            .map_or(JobsAction::None, |j| JobsAction::RevealMp4(*j)),
        JobAction::Select { .. } => JobsAction::None,
    }
}

// =============================================================================
// Pure helpers (testable without egui)
// =============================================================================

fn job_cost_or_zero(job: &Job) -> f64 {
    // Cost proxy until a real Job.cost_usd field lands: the result byte count.
    job.result
        .as_ref()
        .and_then(|v| v.get("bytes"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0)
}

#[derive(Debug, Clone, Default)]
pub(crate) struct FooterStats {
    pub total_count: usize,
    pub active_count: usize,
    pub total_cost_usd: f64,
}

pub(crate) fn compute_footer_stats(jobs: &[Job]) -> FooterStats {
    let mut s = FooterStats {
        total_count: jobs.len(),
        ..FooterStats::default()
    };
    for j in jobs {
        if !j.state.is_terminal() {
            s.active_count += 1;
        }
        s.total_cost_usd += job_cost_or_zero(j);
    }
    s
}

fn format_clock(secs_since_epoch: u64) -> String {
    // Local-friendly hh:mm:ss without pulling chrono.
    let s = secs_since_epoch;
    let hh = (s / 3600) % 24;
    let mm = (s / 60) % 60;
    let ss = s % 60;
    format!("{:02}:{:02}:{:02}", hh, mm, ss)
}

fn format_elapsed(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs / 60) % 60;
    let s = secs % 60;
    if h > 0 {
        format!("{}:{:02}:{:02}", h, m, s)
    } else {
        format!("{}:{:02}", m, s)
    }
}

// =============================================================================
// Tests — pure helpers; UI rendering is not exercised here.
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use playa_jobs_core::job::{Job, JobState};
    use serde_json::json;

    fn fake_job(kind: &str, state: JobState) -> Job {
        let mut j = Job::new(kind.to_string(), json!({"prompt": "p"}));
        j.state = state;
        j
    }

    #[test]
    fn map_state_collapses_to_five_buckets() {
        assert_eq!(map_state(JobState::Pending), JobStatus::Queued);
        assert_eq!(map_state(JobState::Submitting), JobStatus::Running);
        assert_eq!(map_state(JobState::Downloading), JobStatus::Running);
        assert_eq!(map_state(JobState::Complete), JobStatus::Done);
        assert_eq!(map_state(JobState::Failed), JobStatus::Failed);
        assert_eq!(map_state(JobState::Cancelled), JobStatus::Cancelled);
    }

    #[test]
    fn footer_stats_count_active_and_total() {
        let jobs = vec![
            fake_job("k", JobState::Pending),
            fake_job("k", JobState::AwaitingProvider),
            fake_job("k", JobState::Complete),
        ];
        let s = compute_footer_stats(&jobs);
        assert_eq!(s.total_count, 3);
        assert_eq!(s.active_count, 2);
    }

    #[test]
    fn row_id_is_stable_per_job() {
        let j = fake_job("k", JobState::Pending);
        assert_eq!(job_row_id(j.id), job_row_id(j.id));
    }

    #[test]
    fn format_elapsed_under_hour_drops_hour_segment() {
        assert_eq!(format_elapsed(45), "0:45");
        assert_eq!(format_elapsed(125), "2:05");
        assert_eq!(format_elapsed(3725), "1:02:05");
    }

    #[test]
    fn jobs_panel_default_sorts_first_column_descending() {
        let p = JobsPanel::new();
        assert!(p.table.sort_descending);
        assert!(p.table.selected.is_empty());
    }
}
