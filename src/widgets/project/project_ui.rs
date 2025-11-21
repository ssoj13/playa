use eframe::egui;
use egui_taffy::{TuiBuilderLogic, tui};
use taffy::style::Style;

use crate::player::Player;
use crate::widgets::project::project::ProjectActions;

/// Create configured file dialog for image/video selection
fn create_image_dialog(title: &str) -> rfd::FileDialog {
    rfd::FileDialog::new()
        .add_filter("All Supported Files", crate::utils::media::ALL_EXTS)
        .set_title(title)
}

/// Render project window (dock tab): Unified list of Clips & Compositions
pub fn render(ui: &mut egui::Ui, player: &mut Player) -> ProjectActions {
    let mut actions = ProjectActions::new();

    let id = ui.id().with("project_taffy");
    tui(ui, id)
        .reserve_available_space()
        .style(Style {
            flex_direction: taffy::FlexDirection::Column,
            ..Default::default()
        })
        .show(|tui| {
            tui.ui(|ui: &mut egui::Ui| {
                ui.heading("Project");

                // Action buttons (2 rows)
                ui.horizontal(|ui: &mut egui::Ui| {
                    if ui.button("Save").clicked()
                        && let Some(path) = rfd::FileDialog::new()
                            .add_filter("Playa Project", &["json"])
                            .set_title("Save Project")
                            .save_file()
                    {
                        actions.save_project = Some(path);
                    }
                    if ui.button("Load").clicked()
                        && let Some(path) = rfd::FileDialog::new()
                            .add_filter("Playa Project", &["json"])
                            .set_title("Load Project")
                            .pick_file()
                    {
                        actions.load_project = Some(path);
                    }
                });

                ui.horizontal(|ui: &mut egui::Ui| {
                    if ui.button("Add Clip").clicked()
                        && let Some(paths) = create_image_dialog("Add Media Files").pick_files()
                        && !paths.is_empty()
                    {
                        actions.load_sequence = Some(paths[0].clone());
                    }
                    if ui.button("Add Comp").clicked() {
                        actions.new_comp = true;
                    }
                    if ui.button("Clear All").clicked() {
                        actions.clear_all_comps = true;
                    }
                });

                ui.separator();
            });

            // === MEDIA LIST (Unified Clips & Comps) ===
            tui.ui(|ui: &mut egui::Ui| {
                ui.heading("Media");

                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        // Collect all comps to render (unified order)
                        let all_comps: Vec<String> = player.project.comps_order.clone();

                        if all_comps.is_empty() {
                            // Empty state message
                            ui.add_space(20.0);
                            ui.vertical_centered(|ui| {
                                ui.colored_label(
                                    ui.visuals().weak_text_color(),
                                    "No media loaded"
                                );
                                ui.colored_label(
                                    ui.visuals().weak_text_color(),
                                    "Click 'Add Clip' to load files"
                                );
                            });
                            return;
                        }

                        // Render all comps
                        for comp_uuid in &all_comps {
                            let comp = match player.project.media.get(comp_uuid) {
                                Some(c) => c,
                                None => continue,
                            };

                            let is_active = player.active_comp.as_ref() == Some(comp_uuid);

                            // Bar background with selection highlight
                            let bg_color = if is_active {
                                ui.style().visuals.selection.bg_fill
                            } else {
                                ui.style().visuals.faint_bg_color
                            };

                            let available_width = ui.available_width();

                            let frame = egui::Frame::new()
                                .fill(bg_color)
                                .inner_margin(egui::vec2(4.0, 2.0))
                                .corner_radius(2.0)
                                .stroke(egui::Stroke::new(1.0, ui.style().visuals.window_stroke.color));

                            frame.show(ui, |ui| {
                                ui.set_width(available_width);
                                ui.horizontal(|ui| {
                                    // Icon based on mode
                                    let (icon, icon_color) = match comp.mode {
                                        crate::entities::comp::CompMode::File => {
                                            ("[F]", egui::Color32::from_rgb(100, 150, 255))
                                        }
                                        crate::entities::comp::CompMode::Layer => {
                                            ("[C]", egui::Color32::from_rgb(255, 150, 100))
                                        }
                                    };
                                    ui.colored_label(icon_color, icon);

                                    // Compact name + info in one line
                                    let display_text = match comp.mode {
                                        crate::entities::comp::CompMode::File => {
                                            if let Some(mask) = &comp.file_mask {
                                                let filename = std::path::Path::new(mask)
                                                    .file_name()
                                                    .and_then(|s| s.to_str())
                                                    .unwrap_or(mask);
                                                format!("{} • {}", comp.name(), filename)
                                            } else {
                                                comp.name().to_string()
                                            }
                                        }
                                        crate::entities::comp::CompMode::Layer => {
                                            format!("{} (Layer)", comp.name())
                                        }
                                    };

                                    let text_galley = ui.painter().layout_no_wrap(
                                        display_text,
                                        egui::FontId::proportional(12.0),
                                        ui.visuals().text_color(),
                                    );
                                    let (text_rect, response) = ui.allocate_exact_size(
                                        text_galley.size(),
                                        egui::Sense::click_and_drag(),
                                    );

                                    if ui.is_rect_visible(text_rect) {
                                        ui.painter().galley(text_rect.min, text_galley, ui.visuals().text_color());
                                    }

                                    // Click to activate
                                    if response.clicked() {
                                        actions.set_active_comp = Some(comp_uuid.clone());
                                    }

                                    // Drag handling
                                    if response.drag_started() {
                                        if let Some(pos) = response.interact_pointer_pos() {
                                            let duration = comp.frame_count();
                                            let display_name = comp.name().to_string();
                                            ui.ctx().data_mut(|data| {
                                                data.insert_temp(
                                                    egui::Id::new("global_drag_state"),
                                                    crate::widgets::timeline::GlobalDragState::ProjectItem {
                                                        source_uuid: comp_uuid.clone(),
                                                        display_name,
                                                        duration: Some(duration),
                                                        drag_start_pos: pos,
                                                    },
                                                );
                                            });
                                        }
                                    }

                                    // Cursor feedback
                                    if response.dragged() {
                                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                                    } else if response.hovered() {
                                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                                    }

                                    // Right side: frame count, FPS, Delete
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        // Delete button
                                        if ui.small_button("X").clicked() {
                                            actions.remove_comp = Some(comp_uuid.clone());
                                        }
                                        // FPS
                                        ui.colored_label(
                                            ui.visuals().weak_text_color(),
                                            format!("{}fps", comp.fps() as u32)
                                        );
                                        // Frame count
                                        ui.colored_label(
                                            ui.visuals().weak_text_color(),
                                            format!("{}f", comp.frame_count())
                                        );
                                    });
                                });
                            });

                            ui.add_space(1.0);
                        }
                    });
            });
        });

    actions
}
