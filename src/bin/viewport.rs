//! Standalone Viewport window for development and testing.
//!
//! Full viewport with Player, zoom, pan, shaders, and playback controls.

use playa::shell;

use std::sync::{Arc, Mutex};

use eframe::egui;
use playa::dialogs::prefs::AppSettings;
use playa::entities::Frame;
use playa::widgets::viewport::{render, Shaders, ViewportRenderer, ViewportState};

fn main() -> eframe::Result<()> {
    shell::init_logger();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 720.0])
            .with_title("Playa - Viewport"),
        ..Default::default()
    };

    eframe::run_native(
        "playa-viewport",
        options,
        Box::new(|_cc| Ok(Box::new(ViewportApp::new()))),
    )
}

struct ViewportApp {
    shell: shell::Shell,
    viewport_state: ViewportState,
    viewport_renderer: Arc<Mutex<ViewportRenderer>>,
    shader_manager: Shaders,
    frame: Option<Frame>,
    displayed_frame: Option<i32>,
    show_help: bool,
    #[allow(dead_code)] // TODO: use settings in viewport standalone
    settings: AppSettings,
}

impl ViewportApp {
    fn new() -> Self {
        let shell = shell::Shell::with_test_sequence(&shell::test_sequence());

        Self {
            shell,
            viewport_state: ViewportState::new(),
            viewport_renderer: Arc::new(Mutex::new(ViewportRenderer::new())),
            shader_manager: Shaders::new(),
            frame: None,
            displayed_frame: None,
            show_help: true,
            settings: AppSettings::default(),
        }
    }
}

impl eframe::App for ViewportApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Update player
        self.shell.player.update(&mut self.shell.project);

        // Process events
        if let Some(result) = self.shell.process_events() {
            self.shell.handle_deferred(result);
        }

        // Check if frame changed
        let current_frame = self.shell.player.current_frame(&self.shell.project);
        let texture_needs_upload = self.displayed_frame != Some(current_frame);

        if texture_needs_upload {
            self.frame = self.shell.player.get_current_frame(&self.shell.project);
            self.displayed_frame = Some(current_frame);
        }

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

                ui.label(format!("Frame: {}", frame_idx));
                ui.separator();
                ui.label(format!("FPS: {:.1}", fps));
                ui.separator();
                ui.label(if is_playing { "Playing" } else { "Paused" });
                ui.separator();

                // Playback controls
                if ui.button(if is_playing { "||" } else { ">" }).clicked() {
                    self.shell.player.set_is_playing(!is_playing);
                }
                if ui.button("|<").clicked() {
                    self.shell.player.to_start(&mut self.shell.project);
                }
                if ui.button(">|").clicked() {
                    self.shell.player.to_end(&mut self.shell.project);
                }
                if ui.button("<").clicked() {
                    self.shell.player.step(-1, &mut self.shell.project);
                }
                if ui.button(">").clicked() {
                    self.shell.player.step(1, &mut self.shell.project);
                }

                ui.separator();
                ui.label(format!("Zoom: {:.0}%", self.viewport_state.zoom * 100.0));
            });
        });

        // Main viewport
        egui::CentralPanel::default().show(ctx, |ui| {
            let (viewport_actions, _render_time) = render(
                ui,
                self.frame.as_ref(),
                self.shell.error_msg.as_ref(),
                &mut self.shell.player,
                &mut self.shell.project,
                &mut self.viewport_state,
                &self.viewport_renderer,
                &mut self.shader_manager,
                self.show_help,
                false, // is_fullscreen
                texture_needs_upload,
            );

            // Emit viewport events
            for evt in viewport_actions.events {
                self.shell.event_bus.emit_boxed(evt);
            }
        });

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
            if i.key_pressed(egui::Key::F) {
                self.viewport_state.set_mode_fit();
            }
            if i.key_pressed(egui::Key::Num1) {
                self.viewport_state.zoom = 1.0;
            }
            if i.key_pressed(egui::Key::F1) {
                self.show_help = !self.show_help;
            }
        });

        // Request repaint if playing
        if self.shell.player.is_playing() {
            ctx.request_repaint();
        }
    }
}
