//! Standalone Preferences window for development and testing.
//!
//! Shows the full settings/preferences dialog.

use eframe::egui;
use playa::dialogs::prefs::{render_settings_window, AppSettings};

fn main() -> eframe::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([500.0, 600.0])
            .with_title("Playa - Preferences"),
        ..Default::default()
    };

    eframe::run_native(
        "playa-prefs",
        options,
        Box::new(|_cc| Ok(Box::new(PrefsApp::new()))),
    )
}

struct PrefsApp {
    settings: AppSettings,
    show_settings: bool,
    changes_made: bool,
}

impl PrefsApp {
    fn new() -> Self {
        Self {
            settings: AppSettings::default(),
            show_settings: true,
            changes_made: false,
        }
    }
}

impl eframe::App for PrefsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Apply theme
        if self.settings.dark_mode {
            ctx.set_visuals(egui::Visuals::dark());
        } else {
            ctx.set_visuals(egui::Visuals::light());
        }

        // Apply font size
        let mut style = (*ctx.style()).clone();
        for (_, font_id) in style.text_styles.iter_mut() {
            font_id.size = self.settings.font_size;
        }
        ctx.set_style(style);

        // Status panel
        egui::TopBottomPanel::bottom("status_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Settings Preview Mode");
                ui.separator();
                if self.changes_made {
                    ui.colored_label(egui::Color32::YELLOW, "Changes not saved (standalone mode)");
                } else {
                    ui.label("No changes");
                }
                ui.separator();
                ui.label(format!("Font: {:.0}px", self.settings.font_size));
                ui.separator();
                ui.label(if self.settings.dark_mode { "Dark" } else { "Light" });
            });
        });

        // Main settings panel - render inline
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Application Settings");
            ui.separator();
            ui.label("Settings window rendered as overlay above.");
        });

        // Snapshot values before render for change detection
        let prev_dark_mode = self.settings.dark_mode;
        let prev_font_size = self.settings.font_size;
        let prev_fps_base = self.settings.fps_base;
        let prev_loop_enabled = self.settings.loop_enabled;
        let prev_cache_percent = self.settings.cache_memory_percent;

        // Render the actual settings window (it appears as overlay)
        render_settings_window(ctx, &mut self.show_settings, &mut self.settings);

        // Detect changes by comparing individual fields
        if self.settings.dark_mode != prev_dark_mode
            || self.settings.font_size != prev_font_size
            || self.settings.fps_base != prev_fps_base
            || self.settings.loop_enabled != prev_loop_enabled
            || self.settings.cache_memory_percent != prev_cache_percent
        {
            self.changes_made = true;
        }

        // If window was closed, reopen it (standalone mode)
        if !self.show_settings {
            self.show_settings = true;
        }
    }
}
