//! Standalone Timeline window for development and testing.
//!
//! Full timeline editor with playhead, transport controls, layers, and tracks.

use playa::shell;

use eframe::egui;
use playa::dialogs::prefs::AppSettings;
use playa::ui::render_timeline_panel;
use playa::widgets::timeline::TimelineState;
use playa::widgets::viewport::{Shaders, ViewportState};

fn main() -> eframe::Result<()> {
    shell::init_logger();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 400.0])
            .with_title("Playa - Timeline"),
        ..Default::default()
    };

    eframe::run_native(
        "playa-timeline",
        options,
        Box::new(|_cc| Ok(Box::new(TimelineApp::new()))),
    )
}

struct TimelineApp {
    shell: shell::Shell,
    timeline_state: TimelineState,
    viewport_state: ViewportState,
    shader_manager: Shaders,
    settings: AppSettings,
}

impl TimelineApp {
    fn new() -> Self {
        let shell = shell::Shell::with_test_sequence(&shell::test_sequence());

        Self {
            shell,
            timeline_state: TimelineState::default(),
            viewport_state: ViewportState::new(),
            shader_manager: Shaders::new(),
            settings: AppSettings::default(),
        }
    }
}

impl eframe::App for TimelineApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Update player
        self.shell.player.update(&mut self.shell.project);

        // Process events with timeline state
        if let Some(result) = self.shell.process_events_with_state(
            &mut self.timeline_state,
            &mut self.viewport_state,
            &mut self.settings,
        ) {
            self.shell.handle_deferred(result);
        }

        // Sync settings
        self.timeline_state.snap_enabled = self.settings.timeline_snap_enabled;
        self.timeline_state.lock_work_area = self.settings.timeline_lock_work_area;

        // Error panel
        if self.shell.error_msg.is_some() {
            egui::TopBottomPanel::top("error_panel").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(egui::Color32::RED, self.shell.error_msg.as_ref().unwrap());
                    if ui.button("X").clicked() {
                        self.shell.error_msg = None;
                    }
                });
            });
        }

        // Status panel
        egui::TopBottomPanel::bottom("status_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let frame_idx = self.shell.player.current_frame(&self.shell.project);
                let is_playing = self.shell.player.is_playing();
                let fps = self.shell.player.fps_play();
                let loop_enabled = self.shell.player.loop_enabled();

                ui.label(format!("Frame: {}", frame_idx));
                ui.separator();
                ui.label(format!("FPS: {:.1}", fps));
                ui.separator();
                ui.label(if is_playing { "Playing" } else { "Paused" });
                ui.separator();
                ui.label(if loop_enabled { "Loop: ON" } else { "Loop: OFF" });
                ui.separator();
                ui.label(format!(
                    "Snap: {} | Lock: {}",
                    if self.timeline_state.snap_enabled { "ON" } else { "OFF" },
                    if self.timeline_state.lock_work_area { "ON" } else { "OFF" }
                ));
            });
        });

        // Main timeline panel
        egui::CentralPanel::default().show(ctx, |ui| {
            let (_shader_changed, timeline_actions) = render_timeline_panel(
                ui,
                &mut self.shell.player,
                &self.shell.project,
                &mut self.shader_manager,
                &mut self.timeline_state,
                &self.shell.event_bus,
                self.settings.show_tooltips,
            );

            // Timeline events are emitted directly to event_bus by render_timeline_panel
            let _ = timeline_actions; // hover state available if needed
        });

        // Persist settings
        self.settings.timeline_snap_enabled = self.timeline_state.snap_enabled;
        self.settings.timeline_lock_work_area = self.timeline_state.lock_work_area;

        // Keyboard shortcuts
        ctx.input(|i| {
            if i.key_pressed(egui::Key::Space) {
                let playing = self.shell.player.is_playing();
                self.shell.player.set_is_playing(!playing);
            }
            if i.key_pressed(egui::Key::Home) {
                self.shell.player.to_start(&mut self.shell.project);
            }
            if i.key_pressed(egui::Key::End) {
                self.shell.player.to_end(&mut self.shell.project);
            }
            if i.key_pressed(egui::Key::ArrowLeft) {
                self.shell.player.step(-1, &mut self.shell.project);
            }
            if i.key_pressed(egui::Key::ArrowRight) {
                self.shell.player.step(1, &mut self.shell.project);
            }
            if i.key_pressed(egui::Key::L) {
                let loop_on = self.shell.player.loop_enabled();
                self.shell.player.set_loop_enabled(!loop_on);
            }
            if i.key_pressed(egui::Key::S) && i.modifiers.ctrl {
                self.timeline_state.snap_enabled = !self.timeline_state.snap_enabled;
            }
        });

        // Request repaint if playing
        if self.shell.player.is_playing() {
            ctx.request_repaint();
        }
    }
}
