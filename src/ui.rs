use eframe::egui;
use log::info;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::frame::{Frame, FrameStatus};
use crate::player::Player;
use crate::scrub::Scrubber;
use crate::shaders::Shaders;
use crate::timeslider::{time_slider, SequenceRange, TimeSliderConfig};
use crate::viewport::{ViewportRenderer, ViewportState};

/// Image file format filters for file dialogs
pub const FILE_FILTERS: &[&str] = &["exr", "png", "jpg", "jpeg", "tif", "tiff", "tga"];

/// Help text displayed in overlay
pub fn help_text() -> &'static str {
    "Drag'n'drop a file here or double-click to open\n\n\
    Hotkeys:\n\
    F1 - Toggle this help\n\
    F2 - Toggle playlist\n\
    F3 - Preferences\n\
    ESC - Exit Fullscreen / Quit\n\n\
    Z - Toggle Fullscreen\n\
    Ctrl+R - Reset Settings\n\n\
    ' / ` - Toggle Loop\n\
    Playback:\n\
    Space - Play/Pause\n\
    J / , / ← - Backward\n\
    K / ↓ - Stop/Dec FPS\n\
    L / . / → - Forward\n\
    Ctrl+← / ↑ - Go Start\n\
    Ctrl+→ - Go End\n\n\
    View:\n\
    A / 1 - 100% Zoom\n\
    Home / H - Fit to View\n\
    F - Fit to View\n\
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

/// Render playlist panel (right side)
pub fn render_playlist(
    ctx: &egui::Context,
    player: &mut Player,
) -> PlaylistActions {
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
            ui.heading("Sequences");

            // Add/Clear buttons on left, Up/Down on right
            ui.horizontal(|ui| {
                if ui.button("Add").clicked() {
                    if let Some(paths) = rfd::FileDialog::new()
                        .add_filter("Image Files", FILE_FILTERS)
                        .set_title("Add Files")
                        .pick_files()
                    {
                        if !paths.is_empty() {
                            info!("Add button: loading {}", paths[0].display());
                            actions.load_sequence = Some(paths[0].clone());
                        }
                    }
                }
                if ui.button("Clear").clicked() {
                    actions.clear_all = true;
                }

                // Push Up/Down to the right
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let has_selection = player.selected_seq_idx.is_some();
                    ui.add_enabled_ui(has_selection, |ui| {
                        if ui.button("↓ Down").clicked() {
                            if let Some(idx) = player.selected_seq_idx {
                                let new_idx = (idx + 1).min(player.cache.sequences().len().saturating_sub(1));
                                player.cache.move_seq(idx, 1);
                                player.selected_seq_idx = Some(new_idx);
                            }
                        }
                        if ui.button("↑ Up").clicked() {
                            if let Some(idx) = player.selected_seq_idx {
                                let new_idx = idx.saturating_sub(1);
                                player.cache.move_seq(idx, -1);
                                player.selected_seq_idx = Some(new_idx);
                            }
                        }
                    });
                });
            });

            // Save/Load playlist on separate line
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("JSON Playlist", &["json"])
                        .set_title("Save Playlist")
                        .save_file()
                    {
                        actions.save_playlist = Some(path);
                    }
                }
                if ui.button("Load").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("JSON Playlist", &["json"])
                        .set_title("Load Playlist")
                        .pick_file()
                    {
                        actions.load_playlist = Some(path);
                    }
                }
            });

            ui.separator();

            // List of sequences
            egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
                let mut to_remove: Option<usize> = None;
                let mut to_select: Option<usize> = None;

                for (idx, seq) in player.cache.sequences().iter().enumerate() {
                    ui.horizontal(|ui| {
                        // Sequence name (clickable) - highlight if selected
                        let is_selected = player.selected_seq_idx == Some(idx);
                        if ui.selectable_label(is_selected, seq.pattern()).clicked() {
                            to_select = Some(idx);
                        }

                        // Frame count
                        ui.label(format!("{}f", seq.len()));

                        // Remove button
                        if ui.small_button("X").clicked() {
                            to_remove = Some(idx);
                        }
                    });
                }

                // Execute deferred actions
                if let Some(idx) = to_select {
                    player.cache.jump_to_seq(idx);
                    player.selected_seq_idx = Some(idx);
                }
                if let Some(idx) = to_remove {
                    player.cache.remove_seq(idx);
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

            ui.label("FPS:");
            // FPS Combobox with predefined values
            egui::ComboBox::from_id_salt("fps_combo")
                .selected_text(format!("{:.0}", player.fps))
                .show_ui(ui, |ui| {
                    for &fps_value in &[1.0, 2.0, 4.0, 8.0, 12.0, 24.0, 30.0, 60.0, 120.0, 240.0] {
                        ui.selectable_value(&mut player.fps, fps_value, format!("{:.0}", fps_value));
                    }
                });

            // FPS DragValue for custom input
            ui.add(
                egui::DragValue::new(&mut player.fps)
                    .speed(0.1)
                    .range(0.00000001..=1000.0)
            );

            ui.separator();

            ui.label("Shader:");
            // Shader combobox
            egui::ComboBox::from_id_salt("shader_combo")
                .selected_text(format!("{}", shader_manager.current_shader))
                .show_ui(ui, |ui| {
                    for shader_name in shader_manager.get_shader_names() {
                        ui.selectable_value(
                            &mut shader_manager.current_shader,
                            shader_name.clone(),
                            shader_name
                        );
                    }
                });

            ui.separator();

            // Frame number display
            ui.label("Frame:");
            ui.label(format!("{}", player.current_frame()));
        });

        ui.add_space(4.0);

        // Row 3: Custom time slider with sequence visualization
        // Update cache only when sequences change
        if *last_seq_version != player.cache.sequences_version() {
            *cached_seq_ranges = build_sequence_ranges(player);
            *last_seq_version = player.cache.sequences_version();
        }

        let config = TimeSliderConfig::default();
        if let Some(new_frame) = time_slider(
            ui,
            player.current_frame(),
            player.total_frames(),
            cached_seq_ranges,
            &config,
            &player.cache,
        ) {
            player.set_frame(new_frame);
        }

        ui.add_space(8.0);
    });

    // Return true if shader changed
    old_shader != shader_manager.current_shader
}

/// Build sequence ranges for timeline visualization
fn build_sequence_ranges(player: &Player) -> Vec<SequenceRange> {
    let sequences = player.cache.sequences();
    let mut ranges = Vec::new();
    let mut global_offset = 0;

    for seq in sequences {
        let frame_count = seq.len();
        ranges.push(SequenceRange {
            start_frame: global_offset,
            end_frame: global_offset + frame_count.saturating_sub(1),
            pattern: seq.pattern().to_string(),
        });
        global_offset += frame_count;
    }

    ranges
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
    scrubber: &mut Option<Scrubber>,
    show_help: bool,
    is_fullscreen: bool,
    texture_needs_upload: bool,
) -> (ViewportActions, f32) {
    let mut actions = ViewportActions {
        load_sequence: None,
    };
    let mut render_time_ms = 0.0;

    // Use a black background in cinema mode for letterbox effect
    let central = if is_fullscreen {
        egui::CentralPanel::default().frame(
            egui::Frame::new().fill(egui::Color32::BLACK)
        )
    } else {
        egui::CentralPanel::default()
    };

    central.show(ctx, |ui| {
        // Get panel rect always (works even with empty viewport)
        let panel_rect = ui.max_rect();

        // Universal interaction zone for double-click and scrubbing (works always)
        let response = ui.interact(panel_rect, ui.id().with("viewport_interaction"), egui::Sense::click_and_drag());

        // Check for double-click FIRST (works always - opens file dialog)
        let double_clicked = response.double_clicked() ||
            (ctx.input(|i| i.pointer.button_double_clicked(egui::PointerButton::Primary))
             && response.hovered());

        if double_clicked {
            info!("Double-click detected, opening file dialog");
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Image Files", FILE_FILTERS)
                .add_filter("EXR Images", &["exr"])
                .add_filter("PNG Images", &["png"])
                .add_filter("JPEG Images", &["jpg", "jpeg"])
                .add_filter("TIFF Images", &["tif", "tiff"])
                .add_filter("TGA Images", &["tga"])
                .set_title("Select Image File")
                .pick_file()
            {
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

            // Update viewport state only when size changes
            if viewport_state.viewport_size != available_size {
                viewport_state.set_viewport_size(available_size);
            }
            let image_size = egui::vec2(w as f32, h as f32);
            if viewport_state.image_size != image_size {
                viewport_state.set_image_size(image_size);
            }

            // Handle viewport zoom/pan input (before scrubber)
            handle_viewport_input(ctx, ui, panel_rect, viewport_state);

            // Handle scrubbing input (only if NOT double-clicking and frame exists)
            if !double_clicked {
                if let Some(scrubber) = scrubber {
                    use crate::scrub::Scrubber;

                    if response.clicked_by(egui::PointerButton::Primary) || response.dragged_by(egui::PointerButton::Primary) {
                        if let Some(original_mouse_pos) = response.interact_pointer_pos() {
                            // Start scrubbing - freeze bounds
                            if !scrubber.is_active() {
                                let current_bounds = viewport_state.get_image_screen_bounds();
                                let current_size = viewport_state.image_size;

                                // Calculate initial normalized position from mouse
                                let normalized = Scrubber::mouse_to_normalized(original_mouse_pos.x, current_bounds);
                                scrubber.start_scrubbing(current_bounds, current_size, normalized);
                                scrubber.set_last_mouse_x(original_mouse_pos.x);

                                // Pause playback
                                if player.is_playing {
                                    player.toggle_play_pause();
                                }
                            }

                            // Use frozen bounds for entire scrubbing session
                            let image_bounds = scrubber.frozen_bounds()
                                .unwrap_or_else(|| viewport_state.get_image_screen_bounds());

                            if scrubber.mouse_moved(original_mouse_pos.x) {
                                // Mouse moved - recalculate normalized from mouse
                                let normalized = Scrubber::mouse_to_normalized(original_mouse_pos.x, image_bounds);
                                scrubber.set_normalized_position(normalized);
                                scrubber.set_last_mouse_x(original_mouse_pos.x);

                                // Check if normalized is outside valid range (clamped)
                                let is_clamped = normalized < 0.0 || normalized > 1.0;
                                scrubber.set_clamped(is_clamped);

                                // Calculate frame from new normalized position (clamps to valid range)
                                let frame_idx = Scrubber::normalized_to_frame(normalized, player.total_frames());
                                player.set_frame(frame_idx);
                                scrubber.set_current_frame(frame_idx);

                                // Visual line follows mouse everywhere (can be outside image bounds)
                                scrubber.set_visual_x(original_mouse_pos.x);
                            } else {
                                // Mouse didn't move - keep saved normalized position
                                let saved_normalized = scrubber.normalized_position().unwrap_or(0.5);

                                // Check if normalized is outside valid range (clamped)
                                let is_clamped = saved_normalized < 0.0 || saved_normalized > 1.0;
                                scrubber.set_clamped(is_clamped);

                                // Update frame from saved normalized (clamps to valid range)
                                let frame_idx = Scrubber::normalized_to_frame(saved_normalized, player.total_frames());
                                player.set_frame(frame_idx);
                                scrubber.set_current_frame(frame_idx);

                                // Update visual from saved normalized (converts back to pixel position)
                                let visual_x = Scrubber::normalized_to_pixel(saved_normalized, image_bounds);
                                scrubber.set_visual_x(visual_x);
                            }
                        }
                    }

                    // Reset scrubbing when mouse released
                    if scrubber.is_active()
                        && !response.dragged()
                        && !ctx.input(|i| i.pointer.button_down(egui::PointerButton::Primary))
                    {
                        scrubber.stop_scrubbing();
                    }

                    if scrubber.is_active() {
                        ctx.set_cursor_icon(egui::CursorIcon::None);
                    }
                }
            }

            // Measure render time
            let render_start = std::time::Instant::now();

            // Decide if we need to upload a texture this frame
            let renderer = viewport_renderer.clone();
            let state = viewport_state.clone();
            let mut needs_upload = texture_needs_upload;
            {
                let r = renderer.lock().unwrap();
                if r.needs_texture_update(w, h) { needs_upload = true; }
            }

            // Only fetch pixel data when we actually need to upload
            let upload_payload: Option<(crate::frame::PixelBuffer, crate::frame::PixelFormat)> = if needs_upload {
                Some((img.pixel_buffer(), img.pixel_format()))
            } else {
                None
            };

            ui.painter().add(egui::PaintCallback {
                rect: panel_rect,
                callback: std::sync::Arc::new(egui_glow::CallbackFn::new(move |_info, painter: &egui_glow::Painter| {
                    let gl = painter.gl();
                    let mut r = renderer.lock().unwrap();

                    if let Some((pixel_buffer, pixel_format)) = &upload_payload {
                        r.upload_texture(gl, w, h, pixel_buffer, *pixel_format);
                    }
                    r.render(gl, &state);
                })),
            });

            render_time_ms = render_start.elapsed().as_secs_f32() * 1000.0;

            // Draw loading/error indicator
            match frame_state {
                FrameStatus::Loading => {
                    ui.painter().text(
                        panel_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        format!("Loading frame {}...", player.current_frame()),
                        egui::FontId::proportional(24.0),
                        egui::Color32::from_rgba_premultiplied(255, 255, 255, 200),
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
                FrameStatus::Loaded | FrameStatus::Header | FrameStatus::Placeholder => {
                    // No indicator for loaded/header/placeholder frames
                }
            }

            // Draw scrubber visual feedback
            if let Some(scrubber) = scrubber {
                scrubber.draw(ui, panel_rect);
            }

            // Draw help overlay if requested (disabled in cinema mode)
            if show_help && !is_fullscreen {
                render_help_overlay(ui, panel_rect);
            }
        } else if show_help && !is_fullscreen {
            // Also show help if there is no image
            let panel_rect = ui.max_rect();
            render_help_overlay(ui, panel_rect);
        }
    });

    (actions, render_time_ms)
}

/// Handle viewport input (zoom/pan)
fn handle_viewport_input(
    ctx: &egui::Context,
    _ui: &egui::Ui,
    rect: egui::Rect,
    viewport_state: &mut ViewportState,
) {
    // Handle mouse wheel zoom
    let scroll_delta = ctx.input(|i| i.raw_scroll_delta);
    if scroll_delta.y.abs() > 0.1 {
        let cursor_pos = ctx.input(|i| i.pointer.hover_pos());
        if let Some(cursor_pos) = cursor_pos {
            if rect.contains(cursor_pos) {
                // Convert cursor to viewport-relative coords
                let relative_pos = cursor_pos - rect.left_top();
                viewport_state.handle_zoom(scroll_delta.y, relative_pos);
                ctx.request_repaint();
            }
        }
    }

    // Handle middle mouse pan
    let pointer = ctx.input(|i| i.pointer.clone());
    if pointer.button_down(egui::PointerButton::Middle) {
        let delta = pointer.delta();
        if delta.length() > 0.1 {
            viewport_state.handle_pan(delta);
            ctx.request_repaint();
        }
    }
}

/// Render help overlay
fn render_help_overlay(ui: &egui::Ui, panel_rect: egui::Rect) {
    ui.painter().text(
        panel_rect.left_top() + egui::vec2(10.0, 10.0),
        egui::Align2::LEFT_TOP,
        help_text(),
        egui::FontId::proportional(13.0),
        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 128),
    );
}
