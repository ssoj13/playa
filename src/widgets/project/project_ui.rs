use eframe::egui;
use uuid::Uuid;

use crate::entities::Project;
use crate::core::project_events::*;
use crate::core::player::Player;
use crate::widgets::project::project::ProjectActions;

/// Create configured file dialog for image/video selection
fn create_image_dialog(title: &str) -> rfd::FileDialog {
    rfd::FileDialog::new()
        .add_filter("All Supported Files", crate::utils::media::ALL_EXTS)
        .set_title(title)
}

/// Render project window (dock tab): Unified list of Clips & Compositions
pub fn render(ui: &mut egui::Ui, _player: &mut Player, project: &Project) -> ProjectActions {
    let mut actions = ProjectActions::new();

    // Full-rect hover and click tracking
    let panel_rect = ui.available_rect_before_wrap();
    let panel_response = ui.interact(
        panel_rect,
        ui.id().with("project_panel"),
        egui::Sense::click(),
    );

    // Action buttons single row
    ui.horizontal(|ui| {
        if ui.button("Save").clicked()
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("Playa Project", &["json"])
                .set_title("Save Project")
                .save_file()
        {
            actions.send(SaveProjectEvent(path));
        }
        if ui.button("Load").clicked()
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("Playa Project", &["json"])
                .set_title("Load Project")
                .pick_file()
        {
            actions.send(LoadProjectEvent(path));
        }
        if ui.button("Add Clip").clicked()
            && let Some(paths) = create_image_dialog("Add Media Files").pick_files()
            && !paths.is_empty()
        {
            actions.send(AddClipsEvent(paths));
        }
        if ui.button("Add Folder").clicked()
            && let Some(path) = rfd::FileDialog::new()
                .set_title("Select Folder to Scan")
                .pick_folder()
        {
            actions.send(AddFolderEvent(path));
        }
        if ui.button("Add Comp").clicked() {
            actions.send(AddCompEvent {
                name: "New Comp".to_string(),
                fps: 30.0,
            });
        }
        if ui.button("Clear All").clicked() {
            actions.send(ClearAllMediaEvent);
        }
    });

    ui.separator();

    // Media list fills remaining space
    let scroll_height = ui.available_height();
    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            ui.set_min_height(scroll_height);

            // Collect all comps to render (unified order)
            let all_comps: Vec<Uuid> = project.comps_order();

            if all_comps.is_empty() {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    ui.colored_label(ui.visuals().weak_text_color(), "No media loaded");
                    ui.colored_label(
                        ui.visuals().weak_text_color(),
                        "Click 'Add Clip' to load files",
                    );
                });
                return;
            }

            let media = project.media.read().unwrap_or_else(|e| e.into_inner());
            for comp_uuid in &all_comps {
                let comp = match media.get(comp_uuid) {
                    Some(c) => c,
                    None => continue,
                };
                let comps_order = project.comps_order();
                let clicked_idx = match comps_order
                    .iter()
                    .position(|u| u == comp_uuid)
                {
                    Some(i) => i,
                    None => continue,
                };

                let is_active = project.active().as_ref() == Some(comp_uuid);
                let is_selected = project.selection().iter().any(|u| u == comp_uuid);
                let bg_color = if is_selected {
                    ui.style().visuals.selection.bg_fill
                } else {
                    ui.style().visuals.faint_bg_color
                };

                let is_file = comp.is_file_mode();
                let fps = comp.fps() as u32;
                let frame_count = comp.frame_count();
                let display_text = if is_file {
                    if let Some(mask) = comp.file_mask() {
                        let filename = std::path::Path::new(&mask)
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or(&mask);
                        format!("{} • {}", comp.name(), filename)
                    } else {
                        comp.name().to_string()
                    }
                } else {
                    format!("{} (Layer)", comp.name())
                };

                let available_width = ui.available_width();
                let row_height = ui.spacing().interact_size.y * 1.2;

                let (row_rect, response) = ui.allocate_exact_size(
                    egui::vec2(available_width, row_height),
                    egui::Sense::click_and_drag(),
                );

                // Background and stroke
                ui.painter().rect_filled(row_rect, 2.0, bg_color);
                ui.painter().rect_stroke(
                    row_rect,
                    2.0,
                    egui::Stroke::new(1.0, ui.style().visuals.window_stroke.color),
                    egui::StrokeKind::Inside,
                );

                // Active stripe
                if is_active {
                    let stripe_rect =
                        egui::Rect::from_min_size(row_rect.min, egui::vec2(4.0, row_height));
                    ui.painter()
                        .rect_filled(stripe_rect, 0.0, egui::Color32::from_rgb(0, 200, 0));
                }

                let mut cursor_x = row_rect.min.x + 8.0;
                let center_y = row_rect.center().y;

                // Icon
                let (icon, icon_color) = if is_file {
                    ("[F]", egui::Color32::from_rgb(100, 150, 255))
                } else {
                    ("[C]", egui::Color32::from_rgb(255, 150, 100))
                };
                let icon_galley = ui.painter().layout_no_wrap(
                    icon.to_string(),
                    egui::FontId::proportional(12.0),
                    icon_color,
                );
                let icon_pos = egui::pos2(cursor_x, center_y - icon_galley.size().y * 0.5);
                ui.painter().galley(icon_pos, icon_galley, icon_color);
                cursor_x += 22.0;

                // Right text (frame/fps) and delete button positions
                let right_text = format!("{}f  {}fps", frame_count, fps);
                let right_galley = ui.painter().layout_no_wrap(
                    right_text,
                    egui::FontId::proportional(12.0),
                    ui.visuals().weak_text_color(),
                );
                let delete_size = egui::vec2(16.0, 16.0);
                let delete_pos = egui::pos2(
                    row_rect.max.x - delete_size.x - 6.0,
                    center_y - delete_size.y * 0.5,
                );
                let right_pos = egui::pos2(
                    delete_pos.x - 8.0 - right_galley.size().x,
                    center_y - right_galley.size().y * 0.5,
                );

                // Text area width (clip)
                let text_max_width = (right_pos.x - 8.0) - cursor_x;
                if text_max_width > 0.0 {
                    let text_galley = ui.painter().layout_no_wrap(
                        display_text,
                        egui::FontId::proportional(12.0),
                        ui.visuals().text_color(),
                    );
                    let text_pos = egui::pos2(cursor_x, center_y - text_galley.size().y * 0.5);
                    let clip_rect =
                        egui::Rect::from_min_size(text_pos, egui::vec2(text_max_width, row_height));
                    ui.painter().with_clip_rect(clip_rect).galley(
                        text_pos,
                        text_galley,
                        ui.visuals().text_color(),
                    );
                }

                // Right info
                ui.painter()
                    .galley(right_pos, right_galley, ui.visuals().weak_text_color());

                // Delete button
                let delete_rect = egui::Rect::from_min_size(delete_pos, delete_size);
                let delete_resp = ui.interact(
                    delete_rect,
                    ui.id().with(format!("del_{comp_uuid}")),
                    egui::Sense::click(),
                );
                if ui.is_rect_visible(delete_rect) {
                    ui.painter()
                        .rect_filled(delete_rect, 2.0, ui.visuals().extreme_bg_color);
                    ui.painter().rect_stroke(
                        delete_rect,
                        2.0,
                        egui::Stroke::new(1.0, ui.visuals().weak_text_color()),
                        egui::StrokeKind::Inside,
                    );
                    ui.painter().text(
                        delete_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "X",
                        egui::FontId::proportional(10.0),
                        ui.visuals().weak_text_color(),
                    );
                }
                if delete_resp.clicked() {
                    actions.send(RemoveMediaEvent(*comp_uuid));
                }

                // Selection logic (click) and activation (double click) via events
                let modifiers = ui.input(|i| i.modifiers);
                let current_selection = project.selection();
                if response.clicked() {
                    let (sel, anchor) = compute_selection(
                        &comps_order,
                        &current_selection,
                        project.selection_anchor,
                        clicked_idx,
                        modifiers,
                    );
                    actions.events.push(Box::new(ProjectSelectionChangedEvent {
                        selection: sel,
                        anchor,
                    }));
                }
                if response.double_clicked() {
                    let (sel, anchor) = compute_selection(
                        &comps_order,
                        &current_selection,
                        project.selection_anchor,
                        clicked_idx,
                        modifiers,
                    );
                    actions.events.push(Box::new(ProjectSelectionChangedEvent {
                        selection: sel,
                        anchor,
                    }));
                    actions
                        .events
                        .push(Box::new(ProjectActiveChangedEvent(*comp_uuid)));
                }

                // Drag handling
                if response.drag_started() {
                    if let Some(_pos) = response.interact_pointer_pos() {
                        ui.ctx().data_mut(|data| {
                            data.insert_temp(
                                egui::Id::new("global_drag_state"),
                                crate::widgets::timeline::GlobalDragState::ProjectItem {
                                    source_uuid: *comp_uuid,
                                    duration: Some(frame_count),
                                },
                            );
                        });
                    }
                }

                if response.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                } else if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                }

                ui.add_space(1.0);
            }
        });

    // Double-click on empty area opens file dialog (same as Add Clip button)
    if panel_response.double_clicked() {
        if let Some(paths) = create_image_dialog("Add Media Files").pick_files() {
            if !paths.is_empty() {
                actions.send(AddClipsEvent(paths));
            }
        }
    }

    // Set hover state for input routing
    actions.hovered = panel_response.hovered();

    actions
}

fn compute_selection(
    order: &[Uuid],
    current_selection: &[Uuid],
    anchor: Option<usize>,
    clicked_idx: usize,
    modifiers: egui::Modifiers,
) -> (Vec<Uuid>, Option<usize>) {
    let mut selection: Vec<Uuid> = current_selection.to_vec();
    let mut new_anchor = anchor;

    if modifiers.shift {
        let anchor_idx = new_anchor
            .or_else(|| {
                selection
                    .last()
                    .and_then(|u| order.iter().position(|x| x == u))
            })
            .unwrap_or(clicked_idx);
        let (start, end) = if anchor_idx <= clicked_idx {
            (anchor_idx, clicked_idx)
        } else {
            (clicked_idx, anchor_idx)
        };
        for u in order.iter().skip(start).take(end - start + 1) {
            if !selection.contains(u) {
                selection.push(*u);
            }
        }
        new_anchor = Some(clicked_idx);
    } else if modifiers.command || modifiers.ctrl {
        if let Some(pos) = selection.iter().position(|u| *u == order[clicked_idx]) {
            selection.remove(pos);
        } else {
            selection.push(order[clicked_idx]);
        }
        new_anchor = Some(clicked_idx);
    } else {
        selection.clear();
        selection.push(order[clicked_idx]);
        new_anchor = Some(clicked_idx);
    }

    (selection, new_anchor)
}
