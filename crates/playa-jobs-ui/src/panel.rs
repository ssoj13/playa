//! [`JobsPanel`] — sortable, filterable table view of a [`JobQueue`].
//!
//! State (sort, filter, selection) lives on the panel struct; jobs are
//! queried via [`JobQueue::list`] each frame. For real-time updates the
//! host should wire `JobQueue::subscribe(|_| ctx.request_repaint())` at
//! boot so events trigger a redraw.

use std::collections::HashSet;

use egui::{Color32, Sense, Ui};
use egui_extras::{Column, TableBuilder};

use playa_jobs_core::{Job, JobId, JobQueue, JobState};

/// Column the user can sort the table by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobsSortColumn {
    Submitted,
    Elapsed,
    Kind,
    State,
    Cost,
}

/// Action the panel emits per frame for the host to dispatch.
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
    /// Host should open the OS file manager / explorer focused on the
    /// `mp4_path` recorded in the job's result.
    RevealMp4(JobId),
    /// Host should open [`crate::SubmitDialog`].
    OpenSubmit,
}

#[derive(Debug, Default)]
pub struct JobsPanel {
    pub sort_column: Option<JobsSortColumn>,
    pub sort_descending: bool,
    pub filter_search: String,
    pub filter_active_only: bool,
    pub selected: HashSet<JobId>,
}

impl JobsPanel {
    pub fn new() -> Self {
        Self {
            sort_column: Some(JobsSortColumn::Submitted),
            sort_descending: true,
            ..Default::default()
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, queue: &JobQueue) -> JobsAction {
        let mut action = JobsAction::None;

        // Header bar.
        ui.horizontal(|ui| {
            if ui.button("+ Generate").clicked() {
                action = JobsAction::OpenSubmit;
            }
            ui.separator();
            ui.label("🔍");
            ui.text_edit_singleline(&mut self.filter_search);
            ui.checkbox(&mut self.filter_active_only, "Active only");
        });
        ui.separator();

        // Pull + filter + sort. Cheap unless thousands of jobs (HashMap walk).
        let mut jobs = queue.list();
        filter_jobs(&mut jobs, &self.filter_search, self.filter_active_only);
        if let Some(col) = self.sort_column {
            sort_jobs(&mut jobs, col, self.sort_descending);
        }

        if jobs.is_empty() {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                ui.weak("No jobs.");
                ui.weak("Click `+ Generate` to start one, or set FAL_KEY in `.env`.");
            });
            return action;
        }

        // Build the action set we'll commit on bulk-action button click.
        // (Computed before the bottom panel renders so button-enabled state
        // reflects the current selection.)
        let any_selected = !self.selected.is_empty();
        let any_terminal_selected = jobs
            .iter()
            .filter(|j| self.selected.contains(&j.id))
            .any(|j| j.state.is_terminal());
        let any_non_terminal_selected = jobs
            .iter()
            .filter(|j| self.selected.contains(&j.id))
            .any(|j| !j.state.is_terminal());
        let any_failed_or_cancelled_selected = jobs
            .iter()
            .filter(|j| self.selected.contains(&j.id))
            .any(|j| matches!(j.state, JobState::Failed | JobState::Cancelled));

        // Bottom action bar (shown only when something is selected).
        egui::TopBottomPanel::bottom("jobs_actions")
            .resizable(false)
            .min_height(28.0)
            .show_inside(ui, |ui| {
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    let stats = compute_footer_stats(&jobs);
                    ui.weak(format!(
                        "{} active · {} total · {:.2} USD est.",
                        stats.active_count, stats.total_count, stats.total_cost_usd,
                    ));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled(any_terminal_selected, egui::Button::new("Delete"))
                            .clicked()
                        {
                            let ids: Vec<JobId> = jobs
                                .iter()
                                .filter(|j| {
                                    self.selected.contains(&j.id) && j.state.is_terminal()
                                })
                                .map(|j| j.id)
                                .collect();
                            action = JobsAction::Delete(ids);
                        }
                        if ui
                            .add_enabled(
                                any_failed_or_cancelled_selected,
                                egui::Button::new("Retry"),
                            )
                            .clicked()
                        {
                            let ids: Vec<JobId> = jobs
                                .iter()
                                .filter(|j| {
                                    self.selected.contains(&j.id)
                                        && matches!(
                                            j.state,
                                            JobState::Failed | JobState::Cancelled
                                        )
                                })
                                .map(|j| j.id)
                                .collect();
                            action = JobsAction::Retry(ids);
                        }
                        if ui
                            .add_enabled(any_non_terminal_selected, egui::Button::new("Cancel"))
                            .clicked()
                        {
                            let ids: Vec<JobId> = jobs
                                .iter()
                                .filter(|j| {
                                    self.selected.contains(&j.id) && !j.state.is_terminal()
                                })
                                .map(|j| j.id)
                                .collect();
                            action = JobsAction::Cancel(ids);
                        }
                        ui.label(if any_selected {
                            format!("{} selected", self.selected.len())
                        } else {
                            String::new()
                        });
                    });
                });
            });

        // Table view.
        let table = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .column(Column::exact(20.0)) // checkbox
            .column(Column::auto().at_least(80.0)) // submitted
            .column(Column::auto().at_least(60.0)) // elapsed
            .column(Column::auto().at_least(120.0)) // kind
            .column(Column::auto().at_least(80.0)) // state
            .column(Column::auto().at_least(60.0)) // cost
            .column(Column::remainder()); // error / progress

        table
            .header(20.0, |mut header| {
                header.col(|ui| {
                    ui.label("");
                });
                header.col(|ui| {
                    if ui.button("Submitted").clicked() {
                        toggle_sort(self, JobsSortColumn::Submitted);
                    }
                });
                header.col(|ui| {
                    if ui.button("Elapsed").clicked() {
                        toggle_sort(self, JobsSortColumn::Elapsed);
                    }
                });
                header.col(|ui| {
                    if ui.button("Kind").clicked() {
                        toggle_sort(self, JobsSortColumn::Kind);
                    }
                });
                header.col(|ui| {
                    if ui.button("State").clicked() {
                        toggle_sort(self, JobsSortColumn::State);
                    }
                });
                header.col(|ui| {
                    if ui.button("Cost").clicked() {
                        toggle_sort(self, JobsSortColumn::Cost);
                    }
                });
                header.col(|ui| {
                    ui.label("Detail");
                });
            })
            .body(|mut body| {
                let now = playa_jobs_core::job::now_secs();
                for job in &jobs {
                    body.row(18.0, |mut row| {
                        row.col(|ui| {
                            let mut sel = self.selected.contains(&job.id);
                            if ui.checkbox(&mut sel, "").changed() {
                                if sel {
                                    self.selected.insert(job.id);
                                } else {
                                    self.selected.remove(&job.id);
                                }
                            }
                        });
                        row.col(|ui| {
                            ui.label(format_clock(job.created_at));
                        });
                        row.col(|ui| {
                            ui.label(format_elapsed(now.saturating_sub(job.created_at)));
                        });
                        row.col(|ui| {
                            ui.label(&job.kind);
                        });
                        row.col(|ui| {
                            let (label, color) = state_pill(job.state);
                            ui.colored_label(color, label);
                        });
                        row.col(|ui| match job.state {
                            JobState::Complete => {
                                if let Some(serde_json::Value::Number(n)) = job
                                    .result
                                    .as_ref()
                                    .and_then(|v| v.get("bytes"))
                                {
                                    let bytes = n.as_u64().unwrap_or(0);
                                    ui.label(format!("{:.1} MB", bytes as f64 / 1_048_576.0));
                                } else {
                                    ui.label("");
                                }
                            }
                            _ => {
                                ui.label("");
                            }
                        });
                        row.col(|ui| {
                            if let Some(err) = &job.error {
                                ui.colored_label(Color32::from_rgb(220, 60, 60), err);
                            } else if let Some(progress) = &job.progress {
                                ui.label(progress.message.as_deref().unwrap_or(&progress.stage));
                            } else if matches!(job.state, JobState::Complete) {
                                if ui
                                    .add(egui::Button::new("Reveal mp4").sense(Sense::click()))
                                    .clicked()
                                {
                                    action = JobsAction::RevealMp4(job.id);
                                }
                            }
                        });
                    });
                }
            });

        action
    }
}

fn toggle_sort(panel: &mut JobsPanel, col: JobsSortColumn) {
    if panel.sort_column == Some(col) {
        panel.sort_descending = !panel.sort_descending;
    } else {
        panel.sort_column = Some(col);
        panel.sort_descending = true;
    }
}

// =============================================================================
// Pure helpers (testable without egui)
// =============================================================================

pub(crate) fn filter_jobs(jobs: &mut Vec<Job>, search: &str, active_only: bool) {
    let q = search.trim().to_ascii_lowercase();
    jobs.retain(|j| {
        if active_only && j.state.is_terminal() {
            return false;
        }
        if q.is_empty() {
            return true;
        }
        let id = j.id.to_string().to_ascii_lowercase();
        let kind = j.kind.to_ascii_lowercase();
        let prompt = j
            .params
            .get("prompt")
            .and_then(|v| v.as_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        let err = j
            .error
            .as_ref()
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();
        id.contains(&q) || kind.contains(&q) || prompt.contains(&q) || err.contains(&q)
    });
}

pub(crate) fn sort_jobs(jobs: &mut [Job], column: JobsSortColumn, descending: bool) {
    jobs.sort_by(|a, b| {
        let order = match column {
            JobsSortColumn::Submitted => a.created_at.cmp(&b.created_at),
            JobsSortColumn::Elapsed => {
                let ea = a.updated_at.saturating_sub(a.created_at);
                let eb = b.updated_at.saturating_sub(b.created_at);
                ea.cmp(&eb)
            }
            JobsSortColumn::Kind => a.kind.cmp(&b.kind),
            JobsSortColumn::State => format!("{:?}", a.state).cmp(&format!("{:?}", b.state)),
            JobsSortColumn::Cost => {
                // Cost field will be populated in US-08; until then default 0.0.
                let ac = job_cost_or_zero(a);
                let bc = job_cost_or_zero(b);
                ac.partial_cmp(&bc).unwrap_or(std::cmp::Ordering::Equal)
            }
        };
        if descending { order.reverse() } else { order }
    });
}

fn job_cost_or_zero(job: &Job) -> f64 {
    // Cost is added as Job.cost_usd in US-08; until then we look in
    // result["bytes"] as a stand-in proxy (just to make sort deterministic
    // pre-US-08; the real field replaces this).
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
    let mut s = FooterStats::default();
    s.total_count = jobs.len();
    for j in jobs {
        if !j.state.is_terminal() {
            s.active_count += 1;
        }
        s.total_cost_usd += job_cost_or_zero(j);
    }
    s
}

fn state_pill(state: JobState) -> (&'static str, Color32) {
    match state {
        JobState::Pending => ("Pending", Color32::from_rgb(180, 180, 180)),
        JobState::Submitting => ("Submitting", Color32::from_rgb(100, 150, 220)),
        JobState::AwaitingProvider => ("Awaiting", Color32::from_rgb(220, 180, 60)),
        JobState::Downloading => ("Downloading", Color32::from_rgb(120, 180, 240)),
        JobState::Staging => ("Staging", Color32::from_rgb(180, 120, 220)),
        JobState::Complete => ("Complete", Color32::from_rgb(80, 200, 120)),
        JobState::Failed => ("Failed", Color32::from_rgb(220, 60, 60)),
        JobState::Cancelled => ("Cancelled", Color32::from_rgb(160, 140, 80)),
    }
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
    use playa_jobs_core::job::{Job, JobId, JobState};
    use serde_json::json;

    fn fake_job(kind: &str, state: JobState, prompt: &str, err: Option<&str>) -> Job {
        let mut j = Job::new(kind.to_string(), json!({"prompt": prompt}));
        j.id = JobId::new();
        j.state = state;
        if let Some(e) = err {
            j.error = Some(e.to_string());
        }
        j
    }

    #[test]
    fn filter_by_prompt_substring_case_insensitive() {
        let mut jobs = vec![
            fake_job("seedance.text_to_video", JobState::Complete, "cyberpunk wolf", None),
            fake_job("seedance.text_to_video", JobState::Complete, "fluffy kitten", None),
        ];
        filter_jobs(&mut jobs, "WOLF", false);
        assert_eq!(jobs.len(), 1);
        assert_eq!(
            jobs[0].params.get("prompt").unwrap().as_str().unwrap(),
            "cyberpunk wolf"
        );
    }

    #[test]
    fn filter_by_kind() {
        let mut jobs = vec![
            fake_job("seedance.text_to_video", JobState::Complete, "x", None),
            fake_job("ffmpeg.encode", JobState::Complete, "x", None),
        ];
        filter_jobs(&mut jobs, "ffmpeg", false);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].kind, "ffmpeg.encode");
    }

    #[test]
    fn filter_active_only_drops_terminal() {
        let mut jobs = vec![
            fake_job("k", JobState::Pending, "p", None),
            fake_job("k", JobState::Complete, "p", None),
            fake_job("k", JobState::Failed, "p", Some("boom")),
        ];
        filter_jobs(&mut jobs, "", true);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].state, JobState::Pending);
    }

    #[test]
    fn filter_by_error_message() {
        let mut jobs = vec![
            fake_job("k", JobState::Failed, "p", Some("401 unauthorized")),
            fake_job("k", JobState::Failed, "p", Some("network reset")),
        ];
        filter_jobs(&mut jobs, "401", false);
        assert_eq!(jobs.len(), 1);
    }

    #[test]
    fn sort_by_submitted_descending() {
        let mut a = fake_job("k", JobState::Pending, "p", None);
        let mut b = fake_job("k", JobState::Pending, "p", None);
        a.created_at = 100;
        b.created_at = 200;
        let mut jobs = vec![a.clone(), b.clone()];
        sort_jobs(&mut jobs, JobsSortColumn::Submitted, true);
        assert_eq!(jobs[0].created_at, 200);
        assert_eq!(jobs[1].created_at, 100);
    }

    #[test]
    fn sort_by_kind_alphabetical() {
        let jobs_a = fake_job("a.alpha", JobState::Pending, "p", None);
        let jobs_b = fake_job("b.beta", JobState::Pending, "p", None);
        let mut jobs = vec![jobs_b.clone(), jobs_a.clone()];
        sort_jobs(&mut jobs, JobsSortColumn::Kind, false);
        assert_eq!(jobs[0].kind, "a.alpha");
        assert_eq!(jobs[1].kind, "b.beta");
    }

    #[test]
    fn footer_stats_count_active_and_total() {
        let jobs = vec![
            fake_job("k", JobState::Pending, "p", None),
            fake_job("k", JobState::AwaitingProvider, "p", None),
            fake_job("k", JobState::Complete, "p", None),
        ];
        let s = compute_footer_stats(&jobs);
        assert_eq!(s.total_count, 3);
        assert_eq!(s.active_count, 2);
    }

    #[test]
    fn format_elapsed_under_hour_drops_hour_segment() {
        assert_eq!(format_elapsed(45), "0:45");
        assert_eq!(format_elapsed(125), "2:05");
        assert_eq!(format_elapsed(3725), "1:02:05");
    }

    #[test]
    fn jobs_panel_default_starts_with_submitted_descending() {
        let p = JobsPanel::new();
        assert_eq!(p.sort_column, Some(JobsSortColumn::Submitted));
        assert!(p.sort_descending);
        assert!(p.selected.is_empty());
        assert!(!p.filter_active_only);
        assert!(p.filter_search.is_empty());
    }
}
