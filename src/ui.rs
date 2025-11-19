use eframe::egui;
use log::info;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::entities::frame::{Frame, FrameStatus};
use crate::player::Player;
use crate::shaders::Shaders;
use crate::timeline::{render_timeline, TimelineConfig, TimelineAction, TimelineState};
use crate::utils::media;
use crate::viewport::{ViewportRenderer, ViewportState};

/// Create configured file dialog for image/video selection
fn create_image_dialog(title: &str) -> rfd::FileDialog {
    rfd::FileDialog::new()
        .add_filter("All Supported Files", media::ALL_EXTS)
        .set_title(title)
}

/// Help text displayed in overlay
pub fn help_text() -> &'static str {
    "Drag'n'drop a file here or double-click to open\n\n\
    Hotkeys:\n\
    F1 - Toggle this help\n\
    F2 - Toggle playlist\n\
    F3 - Preferences\n\
    F7 - Video Encoding\n\
    ESC - Exit Fullscreen / Quit\n\n\
    Z - Toggle Fullscreen\n\
    Ctrl+R - Reset Settings\n\
    Backspace - Toggle Frame Numbers\n\n\
    ' / ` - Toggle Loop\n\
    B - Set Play Range Start\n\
    N - Set Play Range End\n\
    Ctrl+B - Reset Play Range\n\n\
    Playback:\n\
    Space - Play/Pause Toggle\n\
    K / . - Stop\n\
    J / , - Jog Backward\n\
    L / / - Jog Forward\n\n\
    Frame Navigation:\n\
    Arrow Left/Right - Step 1 frame\n\
    PgUp/PgDn - Step 1 frame\n\
    Shift+Arrows/PgUp/PgDn - Step 25 frames\n\
    Ctrl+Arrows/PgUp/PgDn - Jump to Start/End\n\
    1 / Home - Jump to Start\n\
    2 / End - Jump to End\n\
    [ - Previous Clip\n\
    ] - Next Clip\n\n\
    FPS Control:\n\
    - - Decrease Base FPS\n\
    = / + - Increase Base FPS\n\n\
    View:\n\
    A / H - 100% Zoom\n\
    F - Fit to View\n\n\
    Mouse:\n\
    Mouse Wheel - Zoom\n\
    Middle Drag - Pan\n\
    Left Click - Scrub"
}

/// Project window actions result
pub struct ProjectActions {
    pub load_sequence: Option<PathBuf>,
    pub save_project: Option<PathBuf>,
    pub load_project: Option<PathBuf>,
    pub remove_clip: Option<String>,     // clip UUID to remove
    pub set_active_comp: Option<String>, // comp UUID to activate (from double-click)
    pub new_comp: bool,
    pub remove_comp: Option<String>,     // comp UUID to remove
    pub clear_all_comps: bool,           // clear all compositions
}

// Deprecated - use ProjectActions
pub type PlaylistActions = ProjectActions;

/// Render project window (right panel): Unified list of Clips & Compositions
pub fn render_project_window(ctx: &egui::Context, player: &mut Player) -> ProjectActions {
    let mut actions = ProjectActions {
        load_sequence: None,
        save_project: None,
        load_project: None,
        remove_clip: None,
        set_active_comp: None,
        new_comp: false,
        remove_comp: None,
        clear_all_comps: false,
    };

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

                    // === UNIFIED ITEM LIST ===
                    ui.heading("Items");

                    egui::ScrollArea::vertical()
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            // First, list all clips
                            for clip_uuid in &player.project.clips_order {
                                let clip = match player.project.media.get(clip_uuid).and_then(|s| s.as_clip()) {
                                    Some(c) => c,
                                    None => continue,
                                };

                                ui.horizontal(|ui| {
                                    // Clip icon
                                    ui.label("📹");

                                    // Clip name (truncated pattern) - make it draggable
                                    let pattern = clip.pattern();
                                    let display_name = if pattern.len() > 25 {
                                        format!("{}...", &pattern[..25])
                                    } else {
                                        pattern.to_string()
                                    };

                                    // Use allocate_rect instead of Label to prevent text selection
                                    let text_color = ui.visuals().text_color();
                                    let text_galley = ui.painter().layout_no_wrap(
                                        display_name.clone(),
                                        egui::FontId::default(),
                                        text_color,
                                    );
                                    let desired_size = text_galley.size();
                                    let (rect, label_response) = ui.allocate_exact_size(desired_size, egui::Sense::click_and_drag());

                                    if ui.is_rect_visible(rect) {
                                        ui.painter().galley(rect.min, text_galley, text_color);
                                    }

                                      // Handle drag for clips - store drag state
                                      if label_response.drag_started() {
                                          if let Some(pos) = label_response.interact_pointer_pos() {
                                              let duration = clip.len();
                                              let display_name = display_name.clone();
                                              ui.ctx().data_mut(|data| {
                                                  data.insert_temp(egui::Id::new("global_drag_state"),
                                                      crate::timeline::GlobalDragState::ProjectItem {
                                                          source_uuid: clip_uuid.clone(),
                                                          display_name,
                                                          duration: Some(duration),
                                                          drag_start_pos: pos,
                                                      });
                                              });
                                          }
                                      }

                                    // Update cursor based on drag state
                                    if label_response.dragged() {
                                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                                    } else if label_response.hovered() {
                                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                                    }

                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.small_button("✖").clicked() {
                                            actions.remove_clip = Some(clip_uuid.clone());
                                        }
                                        ui.label(format!("{}f", clip.len()));
                                        let (w, h) = clip.resolution();
                                        ui.label(format!("{}x{}", w, h));
                                    });
                                });

                                ui.add_space(2.0);
                            }

                            // Then, list all compositions
                            for comp_uuid in &player.project.comps_order {
                                let comp = match player.project.media.get(comp_uuid).and_then(|s| s.as_comp()) {
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
                                        // Comp icon
                                        ui.label("🎬");

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
                                                          crate::timeline::GlobalDragState::ProjectItem {
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

/// Render timeline panel with transport controls (bottom, resizable)
///
/// egui automatically persists panel size through its internal state.
/// Returns true if shader was changed.
pub fn render_timeline_panel(
    ctx: &egui::Context,
    player: &mut Player,
    shader_manager: &mut Shaders,
    show_frame_numbers: bool,
    frame: Option<&Frame>,
    viewport_state: &crate::viewport::ViewportState,
    render_time_ms: f32,
    timeline_state: &mut TimelineState,
) -> bool {
    let old_shader = shader_manager.current_shader.clone();

    egui::TopBottomPanel::bottom("timeline")
        .resizable(true)
        .default_height(350.0)
        .height_range(150.0..=700.0)
        .show(ctx, |ui| {
            // Loop and FPS info at top of panel
            ui.horizontal(|ui| {
                ui.checkbox(&mut player.loop_enabled, "Loop");
                ui.add_space(16.0);
                ui.label("FPS:");
                let fps = if player.is_playing { player.fps_play } else { player.fps_base };
                ui.label(format!("{:.2}", fps));
            });

            ui.add_space(4.0);
            ui.separator();

            // Timeline section (with integrated transport controls)
            if let Some(comp_uuid) = &player.active_comp.clone() {
                if let Some(comp) = player.project.media.get(comp_uuid).and_then(|s| s.as_comp()) {
                    let mut config = TimelineConfig::default();
                    config.show_frame_numbers = show_frame_numbers;

                    match render_timeline(ui, comp, &config, timeline_state) {
                        TimelineAction::SetFrame(new_frame) => {
                            player.set_frame(new_frame);
                        }
                        TimelineAction::SelectLayer(idx) => {
                            if let Some(comp_uuid) = &player.active_comp.clone() {
                                if let Some(comp) = player
                                    .project
                                    .media
                                    .get_mut(comp_uuid)
                                    .and_then(|s| s)
                                {
                                    comp.set_selected_layer(Some(idx));
                                }
                            }
                        }
                        TimelineAction::ClearSelection => {
                            if let Some(comp_uuid) = &player.active_comp.clone() {
                                if let Some(comp) = player
                                    .project
                                    .media
                                    .get_mut(comp_uuid)
                                    .and_then(|s| s)
                                {
                                    comp.set_selected_layer(None);
                                }
                            }
                        }
                        TimelineAction::ToStart => {
                            player.to_start();
                        }
                        TimelineAction::ToEnd => {
                            player.to_end();
                        }
                        TimelineAction::TogglePlay => {
                            player.toggle_play_pause();
                        }
                        TimelineAction::Stop => {
                            player.stop();
                        }
                        TimelineAction::JumpToPrevEdge => {
                            // Get child edges sorted by distance from current frame
                            let edges = comp.get_child_edges_near(comp.current_frame);

                            // Find first edge that is before current frame
                            if let Some(&(frame, _)) = edges.iter().find(|(f, _)| *f < comp.current_frame) {
                                player.set_frame(frame);
                            }
                        }
                        TimelineAction::JumpToNextEdge => {
                            // Get child edges sorted by distance from current frame
                            let edges = comp.get_child_edges_near(comp.current_frame);

                            // Find first edge that is after current frame
                            if let Some(&(frame, _)) = edges.iter().find(|(f, _)| *f > comp.current_frame) {
                                player.set_frame(frame);
                            }
                        }
                        TimelineAction::AddLayer { source_uuid, start_frame } => {
                            if let Some(comp_uuid) = &player.active_comp.clone() {
                                // Use add_child method
                                if let Some(comp) = player.project.media.get_mut(comp_uuid).and_then(|s| s) {
                                    if let Err(e) = comp.add_child(source_uuid, start_frame, &player.project) {
                                        eprintln!("Failed to add child: {}", e);
                                    }
                                }
                            }
                        }
                        TimelineAction::MoveLayer { layer_idx, new_start } => {
                            if let Some(comp_uuid) = &player.active_comp.clone() {
                                if let Some(comp) = player.project.media.get_mut(comp_uuid).and_then(|s| s) {
                                    if let Err(e) = comp.move_child(layer_idx, new_start) {
                                        eprintln!("Failed to move child: {}", e);
                                    }
                                }
                            }
                        }
                        TimelineAction::ReorderLayer { from_idx, to_idx } => {
                            if let Some(comp_uuid) = &player.active_comp.clone() {
                                if let Some(comp) = player.project.media.get_mut(comp_uuid).and_then(|s| s) {
                                    if from_idx != to_idx && from_idx < comp.children.len() && to_idx < comp.children.len() {
                                        let child_uuid = comp.children.remove(from_idx);
                                        comp.children.insert(to_idx, child_uuid);
                                        comp.clear_cache();
                                    }
                                }
                            }
                        }
                        TimelineAction::SetLayerPlayStart { layer_idx, new_play_start } => {
                            if let Some(comp_uuid) = &player.active_comp.clone() {
                                if let Some(comp) = player.project.media.get_mut(comp_uuid).and_then(|s| s) {
                                    if let Err(e) = comp.set_child_play_start(layer_idx, new_play_start) {
                                        eprintln!("Failed to set child play start: {}", e);
                                    }
                                }
                            }
                        }
                        TimelineAction::SetLayerPlayEnd { layer_idx, new_play_end } => {
                            if let Some(comp_uuid) = &player.active_comp.clone() {
                                if let Some(comp) = player.project.media.get_mut(comp_uuid).and_then(|s| s) {
                                    if let Err(e) = comp.set_child_play_end(layer_idx, new_play_end) {
                                        eprintln!("Failed to set child play end: {}", e);
                                    }
                                }
                            }
                        }
                        TimelineAction::SetCompPlayStart { frame } => {
                            if let Some(comp_uuid) = &player.active_comp.clone() {
                                if let Some(comp) = player.project.media.get_mut(comp_uuid).and_then(|s| s) {
                                    let play_start = (frame as i32 - comp.start() as i32).max(0);
                                    comp.set_comp_play_start(play_start);
                                }
                            }
                        }
                        TimelineAction::SetCompPlayEnd { frame } => {
                            if let Some(comp_uuid) = &player.active_comp.clone() {
                                if let Some(comp) = player.project.media.get_mut(comp_uuid).and_then(|s| s) {
                                    let play_end = (comp.end() as i32 - frame as i32).max(0);
                                    comp.set_comp_play_end(play_end);
                                }
                            }
                        }
                        TimelineAction::ResetCompPlayArea => {
                            if let Some(comp_uuid) = &player.active_comp.clone() {
                                if let Some(comp) = player.project.media.get_mut(comp_uuid).and_then(|s| s) {
                                    comp.set_comp_play_start(0);
                                    comp.set_comp_play_end(0);
                                }
                            }
                        }
                        TimelineAction::None => {}
                    }
                }
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label("No active composition");
                });
            }

            ui.add_space(4.0);
            ui.separator();

            // Status bar section (bottom of panel)
            ui.horizontal(|ui| {
                // Filename
                if let Some(frame) = frame {
                    if let Some(path) = frame.file() {
                        if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                            ui.monospace(filename);
                        } else {
                            ui.monospace("---");
                        }
                    } else {
                        ui.monospace("No file");
                    }
                } else {
                    ui.monospace("No file");
                }

                ui.separator();

                // Resolution
                if let Some(img) = frame {
                    ui.monospace(format!("{:>4}x{:<4}", img.width(), img.height()));
                } else {
                    ui.monospace("   0x0   ");
                }

                ui.separator();

                // Pixel format
                if let Some(img) = frame {
                    let format_str = match img.pixel_format() {
                        crate::frame::PixelFormat::Rgba8 => "RGBA u8",
                        crate::frame::PixelFormat::RgbaF16 => "RGBA f16",
                        crate::frame::PixelFormat::RgbaF32 => "RGBA f32",
                    };
                    ui.monospace(format_str);
                } else {
                    ui.monospace("---");
                }

                ui.separator();

                // Zoom
                ui.monospace(format!("{:>6.1}%", viewport_state.zoom * 100.0));

                ui.separator();

                // Render time
                ui.monospace(format!("{:.1}ms", render_time_ms));
            });

            ui.add_space(4.0);
        });

    old_shader != shader_manager.current_shader
}

/// Viewport actions result
pub struct ViewportActions {
    pub load_sequence: Option<PathBuf>,
}

/// Render viewport (central panel)
pub fn render_viewport(
    ctx: &egui::Context,
    frame: Option<&Frame>,
    error_msg: Option<&String>,
    player: &mut Player,
    viewport_state: &mut ViewportState,
    viewport_renderer: &Arc<Mutex<ViewportRenderer>>,
    shader_manager: &mut Shaders,
    show_help: bool,
    is_fullscreen: bool,
    texture_needs_upload: bool,
) -> (ViewportActions, f32) {
    let mut actions = ViewportActions {
        load_sequence: None,
    };
    let mut render_time_ms = 0.0;

    let central = if is_fullscreen {
        egui::CentralPanel::default().frame(egui::Frame::new().fill(egui::Color32::BLACK))
    } else {
        egui::CentralPanel::default()
    };

    central.show(ctx, |ui| {
        let panel_rect = ui.max_rect();

        let response = ui.interact(
            panel_rect,
            ui.id().with("viewport_interaction"),
            egui::Sense::click_and_drag(),
        );

        let double_clicked = response.double_clicked()
            || (ctx.input(|i| {
                i.pointer.button_double_clicked(egui::PointerButton::Primary)
            }) && response.hovered());

        if double_clicked {
            info!("Double-click detected, opening file dialog");
            if let Some(path) = create_image_dialog("Select Image File").pick_file() {
                info!("File selected: {}", path.display());
                actions.load_sequence = Some(path);
            }
        }

        if let Some(error) = error_msg {
            ui.centered_and_justified(|ui| {
                ui.colored_label(egui::Color32::RED, error);
            });
        } else if let Some(img) = frame {
            let w = img.width();
            let h = img.height();
            let frame_state = img.status();
            let available_size = panel_rect.size();

            if viewport_state.viewport_size != available_size {
                viewport_state.set_viewport_size(available_size);
            }
            let image_size = egui::vec2(w as f32, h as f32);
            if viewport_state.image_size != image_size {
                viewport_state.set_image_size(image_size);
            }

            handle_viewport_input(ctx, ui, panel_rect, viewport_state);

            if let Some(frame_idx) =
                viewport_state.handle_scrubbing(&response, double_clicked, player.total_frames())
            {
                player.set_frame(frame_idx);
            }

            let render_start = std::time::Instant::now();

            let renderer = viewport_renderer.clone();
            let state = viewport_state.clone();
            let mut needs_upload = texture_needs_upload;
            {
                let r = renderer.lock().unwrap();
                if r.needs_texture_update(w, h) {
                    needs_upload = true;
                }
            }

            let maybe_pixels = if needs_upload {
                Some((img.buffer(), img.pixel_format()))
            } else {
                None
            };

            ui.painter().add(egui::PaintCallback {
                rect: panel_rect,
                callback: Arc::new(egui_glow::CallbackFn::new(
                    move |_info, painter| {
                        let gl = painter.gl();
                        let mut renderer = renderer.lock().unwrap();
                        if let Some((pixels, pixel_format)) = maybe_pixels.as_ref() {
                            renderer.upload_texture(gl, w, h, &*pixels, *pixel_format);
                        }
                        renderer.render(gl, &state);
                    },
                )),
            });

            render_time_ms = render_start.elapsed().as_secs_f32() * 1000.0;

            match frame_state {
                FrameStatus::Loading => {
                    ui.painter().text(
                        panel_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        format!("Loading frame {}...", player.current_frame()),
                        egui::FontId::proportional(24.0),
                        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 200),
                    );
                }
                FrameStatus::Error => {
                    ui.painter().text(
                        panel_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        format!("Failed to load frame {}", player.current_frame()),
                        egui::FontId::proportional(24.0),
                        egui::Color32::from_rgb(255, 100, 100),
                    );
                }
                FrameStatus::Loaded | FrameStatus::Header | FrameStatus::Placeholder => {}
            }

            // Draw viewport overlays (scrubber, guides, etc.)
            viewport_state.draw(ui, panel_rect);
        } else {
            ui.centered_and_justified(|ui| {
                ui.label("No frame loaded. Drag'n'drop a file or use the playlist.");
            });
        }

        if show_help {
            render_help_overlay(ui, panel_rect);
        }

        // Shader selector overlay (top-right corner)
        egui::Area::new(ui.id().with("shader_overlay"))
            .fixed_pos(egui::pos2(panel_rect.max.x - 200.0, panel_rect.min.y + 10.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Shader:");
                    egui::ComboBox::from_id_salt("shader_selector_viewport")
                        .selected_text(&shader_manager.current_shader)
                        .show_ui(ui, |ui| {
                            for shader_name in shader_manager.get_shader_names() {
                                ui.selectable_value(
                                    &mut shader_manager.current_shader,
                                    shader_name.to_string(),
                                    shader_name,
                                );
                            }
                        });
                });
            });
    });

    (actions, render_time_ms)
}

fn handle_viewport_input(
    ctx: &egui::Context,
    _ui: &egui::Ui,
    rect: egui::Rect,
    viewport_state: &mut ViewportState,
) {
    let scroll_delta = ctx.input(|i| i.raw_scroll_delta);
    if scroll_delta.y.abs() > 0.1 {
        let cursor_pos = ctx.input(|i| i.pointer.hover_pos());
        if let Some(cursor_pos) = cursor_pos
            && rect.contains(cursor_pos)
        {
            let relative_pos = cursor_pos - rect.left_top();
            viewport_state.handle_zoom(scroll_delta.y, relative_pos);
            ctx.request_repaint();
        }
    }

    let pointer = ctx.input(|i| i.pointer.clone());
    if pointer.button_down(egui::PointerButton::Middle) {
        let delta = pointer.delta();
        if delta.length() > 0.1 {
            viewport_state.handle_pan(delta);
            ctx.request_repaint();
        }
    }
}

fn render_help_overlay(ui: &egui::Ui, panel_rect: egui::Rect) {
    ui.painter().text(
        panel_rect.left_top() + egui::vec2(10.0, 10.0),
        egui::Align2::LEFT_TOP,
        help_text(),
        egui::FontId::proportional(13.0),
        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 128),
    );
}

