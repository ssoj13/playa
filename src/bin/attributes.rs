//! Standalone Attributes Editor window for development and testing.
//!
//! Shows attribute editor for test Comp loaded from kz sequence.

use playa::shell;

use eframe::egui;
use playa::widgets::ae::{render, AttributesState};

fn main() -> eframe::Result<()> {
    shell::init_logger();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([400.0, 600.0])
            .with_title("Playa - Attributes Editor"),
        ..Default::default()
    };

    eframe::run_native(
        "playa-attributes",
        options,
        Box::new(|_cc| Ok(Box::new(AttributesApp::new()))),
    )
}

struct AttributesApp {
    shell: shell::Shell,
    attributes_state: AttributesState,
}

impl AttributesApp {
    fn new() -> Self {
        let shell = shell::Shell::with_test_sequence(shell::TEST_SEQUENCE);

        Self {
            shell,
            attributes_state: AttributesState::default(),
        }
    }
}

impl eframe::App for AttributesApp {
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
                if let Some(active) = self.shell.player.active_comp() {
                    if let Some(comp) = self.shell.project.get_comp(active) {
                        ui.label(format!("Comp: {}", comp.name()));
                        ui.separator();
                        ui.label(format!("UUID: {:.8}", active));
                        ui.separator();
                        ui.label(format!("Attrs: {}", comp.attrs.len()));
                    }
                } else {
                    ui.label("No active comp");
                }
            });
        });

        // Comp selector panel
        egui::TopBottomPanel::top("comp_selector").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Select Comp:");
                let comps_order = self.shell.project.comps_order();
                for uuid in comps_order {
                    if let Some(comp) = self.shell.project.get_comp(uuid) {
                        let is_active = self.shell.player.active_comp() == Some(uuid);
                        if ui.selectable_label(is_active, comp.name()).clicked() {
                            self.shell.player.set_active_comp(Some(uuid), &mut self.shell.project);
                        }
                    }
                }
            });
        });

        // Main attributes editor
        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(active) = self.shell.player.active_comp() {
                self.shell.project.modify_comp(active, |comp| {
                    let comp_name = comp.name().to_string();
                    render(ui, &mut comp.attrs, &mut self.attributes_state, &comp_name);
                });
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label("No comp loaded. Check test sequence path.");
                });
            }
        });
    }
}
