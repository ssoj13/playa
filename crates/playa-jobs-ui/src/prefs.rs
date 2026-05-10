//! Preferences-panel renderer for [`playa_jobs_core::JobsSettings`].
//!
//! Plug into a [`playa_prefs::PrefsRegistry`] like:
//! ```ignore
//! registry.add(playa_prefs::PrefsEntry {
//!     id: "jobs",
//!     label: "Jobs & Rendering",
//!     category: "Integrations",
//!     search_keywords: vec!["seedance", "fal.ai", "budget", "queue", "cost"],
//!     render: Box::new(|ui, app_settings| {
//!         playa_jobs_ui::prefs::render(ui, &mut app_settings.jobs);
//!     }),
//! });
//! ```

use egui::Ui;
use playa_jobs_core::JobsSettings;

pub fn render(ui: &mut Ui, settings: &mut JobsSettings) {
    ui.heading("Budget");
    ui.checkbox(&mut settings.daily_budget_enabled, "Cap daily spend");
    ui.add_enabled_ui(settings.daily_budget_enabled, |ui| {
        ui.add(
            egui::Slider::new(&mut settings.daily_budget_usd, 1.0..=500.0)
                .suffix(" USD")
                .integer()
                .text("daily cap"),
        );
        ui.weak("Submits beyond the cap are rejected with a clear error.");
    });
    ui.add_space(12.0);

    ui.heading("Behaviour");
    ui.checkbox(
        &mut settings.auto_attach_mp4,
        "Auto-attach completed mp4 to active comp",
    );
    ui.add_space(12.0);

    ui.heading("Retention");
    let mut retain = settings.retention_days.is_some();
    let toggled = ui
        .checkbox(&mut retain, "Auto-prune terminal jobs after N days")
        .changed();
    if toggled {
        settings.retention_days = if retain { Some(30) } else { None };
    }
    if retain {
        let days = settings.retention_days.get_or_insert(30);
        ui.add(
            egui::DragValue::new(days)
                .range(1..=365)
                .suffix(" days"),
        );
    }
}

#[cfg(test)]
mod tests {
    // Tests for the underlying JobsSettings struct live in playa-jobs-core;
    // egui rendering is a pass-through over those values. Smoke is covered
    // when playa-app wires this panel via the prefs registry.
}
