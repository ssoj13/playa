use eframe::egui;
use log::info;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::frame::{Frame, FrameStatus};
use crate::player::Player;
use crate::shaders::Shaders;
use crate::timeslider::{time_slider, SequenceRange, TimeSliderConfig};
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

/// Playlist actions result
pub struct PlaylistActions {
    pub load_sequence: Option<PathBuf>,
    pub clear_all: bool,
    pub save_playlist: Option<PathBuf>,
    pub load_playlist: Option<PathBuf>,
}

/// Render playlist panel (right side) based on Project/order_clips
pub fn render_playlist(ctx: &egui::Context, player: &mut Player) -> PlaylistActions {
    let mut actions = PlaylistActions {
        load_sequence: None,
        clear_all: false,
        save_playlist: None,
        load_playlist: None,
    };

    egui::SidePanel::right("playlist")
        .default_width(250.0)
        .min_width(20.0)
        .resizable(true)
        .show(ctx, |ui| {
            egui::ScrollArea::both()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    ui.heading("Playlist");

                    // Add/Clear buttons on left, Up/Down on right
                    ui.horizontal(|ui| {
                        if ui.button("Add").clicked()
                            && let Some(paths) = create_image_dialog("Add Files").pick_files()
                            && !paths.is_empty()
                        {
                            info!("Add button: loading {}", paths[0].display());
                            actions.load_sequence = Some(paths[0].clone());
                        }
                        if ui.button("Clear").clicked() {
                            actions.clear_all = true;
                        }

                        // Push Up/Down to the right (reorder Project.order_clips)
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let has_selection = player.selected_seq_idx.is_some();
                            ui.add_enabled_ui(has_selection, |ui| {
                                if ui.button("↓ Down").clicked()
                                    && let Some(idx) = player.selected_seq_idx
                                {
                                    let len = player.project.order_clips.len();
                                    if idx + 1 < len {
                                        player.project.order_clips.swap(idx, idx + 1);
                                        player.selected_seq_idx = Some(idx + 1);
                                    }
                                }
                                if ui.button("↑ Up").clicked()
                                    && let Some(idx) = player.selected_seq_idx
                                {
                                    if idx > 0 {
                                        player.project.order_clips.swap(idx, idx - 1);
                                        player.selected_seq_idx = Some(idx - 1);
                                    }
                                }
                            });
                        });
                    });

                    // Save/Load playlist on separate line
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked()
                            && let Some(path) = rfd::FileDialog::new()
                                .add_filter("JSON Playlist", &["json"])
                                .set_title("Save Playlist")
                                .save_file()
                        {
                            actions.save_playlist = Some(path);
                        }
                        if ui.button("Load").clicked()
                            && let Some(path) = rfd::FileDialog::new()
                                .add_filter("JSON Playlist", &["json"])
                                .set_title("Load Playlist")
                                .pick_file()
                        {
                            actions.load_playlist = Some(path);
                        }
                    });

                    ui.separator();

                    // List of clips from Project.order_clips
                    egui::ScrollArea::vertical()
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            let mut to_remove: Option<usize> = None;
                            let mut to_select: Option<usize> = None;

                            for (idx, clip_uuid) in player.project.order_clips.iter().enumerate() {
                                let clip = match player.project.clips.get(clip_uuid) {
                                    Some(c) => c,
                                    None => continue,
                                };
                                let is_selected = player.selected_seq_idx == Some(idx);

                                let frame = if is_selected {
                                    egui::Frame::new()
                                        .fill(ui.style().visuals.selection.bg_fill)
                                        .inner_margin(4.0)
                                } else {
                                    egui::Frame::new().inner_margin(4.0)
                                };

                                frame.show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        let name_label = ui.selectable_label(
                                            false,
                                            clip.pattern().to_string(),
                                        );

                                        if name_label.clicked() {
                                            to_select = Some(idx);
                                        }

                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                if ui.small_button("X").clicked() {
                                                    to_remove = Some(idx);
                                                }

                                                ui.label(format!("{}f", clip.len()));

                                                let (w, h) = clip.resolution();
                                                ui.label(format!("{}x{}", w, h));
                                            },
                                        );
                                    });
                                });

                                ui.add_space(2.0);
                            }

                            // Execute deferred actions
                            if let Some(idx) = to_select {
                                if let Some(uuid) = player.project.order_clips.get(idx).cloned() {
                                    player.selected_seq_idx = Some(idx);
                                    player.set_active_clip_by_uuid(&uuid);
                                }
                            }
                            if let Some(idx) = to_remove {
                                if idx < player.project.order_clips.len() {
                                    let removed_uuid = player.project.order_clips.remove(idx);
                                    player.project.clips.remove(&removed_uuid);
                                }
                                if player.selected_seq_idx == Some(idx) {
                                    player.selected_seq_idx = None;
                                } else if let Some(sel) = player.selected_seq_idx {
                                    if sel > idx {
                                        player.selected_seq_idx = Some(sel - 1);
                                    }
                                }
                            }
                        });
                });
        });

    actions
}

/// Render controls panel (bottom)
pub fn render_controls(
    ctx: &egui::Context,
    player: &mut Player,
    shader_manager: &mut Shaders,
    cached_seq_ranges: &mut Vec<SequenceRange>,
    last_seq_version: &mut usize,
    show_frame_numbers: bool,
) -> bool {
    let old_shader = shader_manager.current_shader.clone();

    egui::TopBottomPanel::bottom("controls").show(ctx, |ui| {
        ui.add_space(8.0);

        // Row 1: Transport controls (Start | Play/Pause | End) centered
        ui.vertical_centered(|ui| {
            ui.horizontal(|ui| {
                if ui.button("⏮ Start").clicked() {
                    player.to_start();
                }

                let play_text = if player.is_playing { "⏸ Pause" } else { "▶ Play" };
                if ui.button(play_text).clicked() {
                    player.toggle_play_pause();
                }

                if ui.button("End ⏭").clicked() {
                    player.to_end();
                }
            });
        });

        ui.add_space(4.0);

        // Row 2: Loop, FPS, Shader, and Frame number
        ui.horizontal(|ui| {
            ui.checkbox(&mut player.loop_enabled, "Loop");

            ui.separator();

            ui.label("Base FPS:");

            let old_fps = player.fps_base;

            egui::ComboBox::from_id_salt("fps_combo")
                .selected_text(format!("{:.0}", player.fps_base))
                .show_ui(ui, |ui| {
                    for &fps_value in &[1.0, 2.0, 4.0, 8.0, 12.0, 24.0, 30.0, 60.0, 120.0, 240.0] {
                        ui.selectable_value(
                            &mut player.fps_base,
                            fps_value,
                            format!("{:.0}", fps_value),
                        );
                    }
                });

            ui.add(
                egui::DragValue::new(&mut player.fps_base)
                    .speed(0.1)
                    .range(0.00000001..=1000.0),
            );

            if (player.fps_base - old_fps).abs() > 0.001 && player.is_playing {
                if player.fps_play < player.fps_base {
                    log::debug!(
                        "Base FPS changed from {:.1} to {:.1}, pushing play_fps from {:.1} to {:.1}",
                        old_fps,
                        player.fps_base,
                        player.fps_play,
                        player.fps_base
                    );
                    player.fps_play = player.fps_base;
                }
            }

            if player.is_playing {
                ui.label(format!("Play FPS: {:.0}", player.fps_play));
            }

            ui.separator();

            ui.label("Shader:");
            egui::ComboBox::from_id_salt("shader_combo")
                .selected_text(shader_manager.current_shader.to_string())
                .show_ui(ui, |ui| {
                    for shader_name in shader_manager.get_shader_names() {
                        ui.selectable_value(
                            &mut shader_manager.current_shader,
                            shader_name.clone(),
                            shader_name,
                        );
                    }
                });

            ui.separator();

            ui.label("Frame:");
            ui.label(format!("{}", player.current_frame()));
        });

        ui.add_space(4.0);

        // Row 3: Custom time slider
        // For now, build a single range for the active comp
        *cached_seq_ranges = build_sequence_ranges(player);
        *last_seq_version = cached_seq_ranges.len();

        let mut config = TimeSliderConfig::default();
        config.show_frame_numbers = show_frame_numbers;
        if let Some(new_frame) = time_slider(
            ui,
            player.current_frame(),
            player.total_frames(),
            cached_seq_ranges,
            &config,
        ) {
            player.set_frame(new_frame);
        }

        ui.add_space(8.0);
    });

    old_shader != shader_manager.current_shader
}

/// Build sequence ranges for timeline visualization (comp-based)
fn build_sequence_ranges(player: &Player) -> Vec<SequenceRange> {
    let total_frames = player.total_frames();
    if total_frames == 0 {
        return Vec::new();
    }

    vec![SequenceRange {
        start_frame: 0,
        end_frame: total_frames.saturating_sub(1),
        pattern: "Comp".to_string(),
    }]
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
        } else {
            ui.centered_and_justified(|ui| {
                ui.label("No frame loaded. Drag'n'drop a file or use the playlist.");
            });
        }

        if show_help {
            render_help_overlay(ui, panel_rect);
        }
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
