use eframe::egui;
use crate::player::Player;
use super::ProjectActions;

/// Create configured file dialog for image/video selection
fn create_image_dialog(title: &str) -> rfd::FileDialog {
    rfd::FileDialog::new()
        .add_filter("All Supported Files", crate::utils::media::ALL_EXTS)
        .set_title(title)
}

/// Render project window (right panel): Unified list of Clips & Compositions
pub fn render_project_window(ctx: &egui::Context, player: &mut Player) -> ProjectActions {
    let mut actions = ProjectActions::new();

    egui::SidePanel::right("project_window")
        .default_width(280.0)
        .min_width(20.0)
        .resizable(true)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    ui.heading("Project");

                    // Action buttons (2 rows)
                    ui.horizontal(|ui| {
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

                    ui.horizontal(|ui| {
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

                    // === COMPOSITIONS LIST ===
                    ui.heading("Compositions");

                    egui::ScrollArea::vertical()
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            // List all compositions (both File and Layer modes)
                            for comp_uuid in &player.project.comps_order {
                                let comp = match player.project.media.get(comp_uuid) {
                                    Some(c) => c,
                                    None => continue,
                                };

                                let is_active = player.active_comp.as_ref() == Some(comp_uuid);

                                let frame = if is_active {
                                    egui::Frame::new()
                                        .fill(ui.style().visuals.selection.bg_fill)
                                        .inner_margin(4.0)
                                        .corner_radius(2.0)
                                } else {
                                    egui::Frame::new().inner_margin(4.0)
                                };

                                frame.show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        // Comp icon (different for File vs Layer mode)
                                        let icon = match comp.mode {
                                            crate::entities::comp::CompMode::File => "📹", // File mode (image sequence)
                                            crate::entities::comp::CompMode::Layer => "🎬", // Layer mode (composition)
                                        };
                                        ui.label(icon);

                                        // Comp name - clickable for activation and draggable
                                        // Use allocate_rect instead of Label to prevent text selection
                                        let text_color = ui.visuals().text_color();
                                        let text_galley = ui.painter().layout_no_wrap(
                                            comp.name().to_string(),
                                            egui::FontId::default(),
                                            text_color,
                                        );
                                        let desired_size = text_galley.size();
                                        let (rect, name_response) = ui.allocate_exact_size(desired_size, egui::Sense::click_and_drag());

                                        if ui.is_rect_visible(rect) {
                                            ui.painter().galley(rect.min, text_galley, text_color);
                                        }

                                        // Double-click to activate
                                        if name_response.double_clicked() {
                                            actions.set_active_comp = Some(comp_uuid.clone());
                                        }

                                          // Handle drag for comps - store drag state
                                          if name_response.drag_started() {
                                              if let Some(pos) = name_response.interact_pointer_pos() {
                                                  let duration = comp.frame_count();
                                                  let display_name = comp.name().to_string();
                                                  ui.ctx().data_mut(|data| {
                                                      data.insert_temp(egui::Id::new("global_drag_state"),
                                                          crate::widgets::timeline::GlobalDragState::ProjectItem {
                                                              source_uuid: comp_uuid.clone(),
                                                              display_name,
                                                              duration: Some(duration),
                                                              drag_start_pos: pos,
                                                          });
                                                  });
                                              }
                                          }

                                        // Update cursor based on drag state
                                        if name_response.dragged() {
                                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                                        } else if name_response.hovered() {
                                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                                        }

                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            if ui.small_button("✖").clicked() {
                                                actions.remove_comp = Some(comp_uuid.clone());
                                            }
                                            ui.label(format!("{}fps", comp.fps() as u32));
                                            ui.label(format!("{}f", comp.frame_count()));
                                        });
                                    })
                                });

                                ui.add_space(2.0);
                            }
                        });
                });
        });

    actions
}
