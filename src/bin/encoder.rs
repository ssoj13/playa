//! Standalone Encoder dialog window for development and testing.
//!
//! Shows the full encode dialog with codec settings and export options.

use playa::shell;

use eframe::egui;
use playa::dialogs::encode::{EncodeDialog, EncodeDialogSettings};

fn main() -> eframe::Result<()> {
    shell::init_logger();

    // Initialize FFmpeg
    if let Err(e) = playa_ffmpeg::init() {
        log::error!("Failed to initialize FFmpeg: {}", e);
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([600.0, 700.0])
            .with_title("Playa - Encoder"),
        ..Default::default()
    };

    eframe::run_native(
        "playa-encoder",
        options,
        Box::new(|_cc| Ok(Box::new(EncoderApp::new()))),
    )
}

struct EncoderApp {
    shell: shell::Shell,
    encode_dialog: Option<EncodeDialog>,
}

impl EncoderApp {
    fn new() -> Self {
        let shell = shell::Shell::with_test_sequence(shell::TEST_SEQUENCE);

        // Create encode dialog if we have an active comp
        let encode_dialog = shell.player.active_comp().map(|_| {
            let settings = EncodeDialogSettings::default();
            EncodeDialog::load_from_settings(&settings)
        });

        Self {
            shell,
            encode_dialog,
        }
    }
}

impl eframe::App for EncoderApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Process events
        if let Some(result) = self.shell.process_events() {
            self.shell.handle_deferred(result);
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
                if let Some(ref dialog) = self.encode_dialog {
                    if dialog.is_encoding() {
                        ui.label("Status: Encoding...");
                        ui.separator();
                        if let Some(ref progress) = dialog.progress {
                            let pct = if progress.total_frames > 0 {
                                progress.current_frame as f32 / progress.total_frames as f32 * 100.0
                            } else {
                                0.0
                            };
                            ui.label(format!("Progress: {:.1}%", pct));
                        } else {
                            ui.label("Progress: 0%");
                        }
                    } else {
                        ui.label("Status: Ready");
                    }
                } else {
                    ui.label("No active comp for encoding");
                }
            });
        });

        // Main encode dialog
        egui::CentralPanel::default().show(ctx, |ui| {
            // Recreate dialog if needed
            if self.encode_dialog.is_none() && self.shell.player.active_comp().is_some() {
                let settings = EncodeDialogSettings::default();
                self.encode_dialog = Some(EncodeDialog::load_from_settings(&settings));
            }

            if let Some(ref mut dialog) = self.encode_dialog {
                // Render dialog inline (not as window)
                ui.heading("Encode Settings");
                ui.separator();

                let active_comp = self.shell.player.active_comp();
                let _should_stay_open = dialog.render(ctx, &self.shell.project, active_comp);
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label("No comp loaded. Check test sequence path.");
                });
            }
        });

        // Request repaint if encoding
        if let Some(ref dialog) = self.encode_dialog {
            if dialog.is_encoding() {
                ctx.request_repaint();
            }
        }
    }
}
