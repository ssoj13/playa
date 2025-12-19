//! Viewport widget - UI rendering

use eframe::egui;
use log::info;
use std::sync::{Arc, Mutex};

use super::shaders::Shaders;
use super::{ViewportRenderer, ViewportState};
use super::gizmo::GizmoState;
use super::tool::ToolMode;
use crate::entities::node::Node;
use crate::entities::Project;
use crate::entities::frame::{Frame, FrameStatus};
use crate::core::event_bus::BoxedEvent;
use crate::core::player::Player;
use crate::widgets::actions::ActionQueue;
use crate::widgets::file_dialogs::create_media_dialog;

pub type ViewportActions = ActionQueue;

/// Render viewport inside provided UI (dock tab or fullscreen panel)
pub fn render(
    ui: &mut egui::Ui,
    frame: Option<&Frame>,
    error_msg: Option<&String>,
    player: &mut Player,
    project: &mut Project,
    viewport_state: &mut ViewportState,
    viewport_renderer: &Arc<Mutex<ViewportRenderer>>,
    shader_manager: &mut Shaders,
    gizmo_state: &mut GizmoState,
    show_help: bool,
    is_fullscreen: bool,
    texture_needs_upload: bool,
) -> (ViewportActions, f32) {
    let mut actions = ViewportActions::default();
    let mut render_time_ms = 0.0;
    let old_shader = shader_manager.current_shader.clone();

    let ctx = ui.ctx().clone();
    let panel_rect = ui.max_rect();
    if is_fullscreen {
        ui.painter()
            .rect_filled(panel_rect, 0.0, egui::Color32::BLACK);
    }

    let response = ui.interact(
        panel_rect,
        ui.id().with("viewport_interaction"),
        egui::Sense::click_and_drag(),
    );

    let double_clicked = response.double_clicked()
        || (ctx.input(|i| {
            i.pointer
                .button_double_clicked(egui::PointerButton::Primary)
        }) && response.hovered());

    if double_clicked {
        info!("Double-click detected, opening file dialog");
        if let Some(paths) = create_media_dialog("Select Media Files").pick_files()
            && !paths.is_empty() {
                info!("Files selected: {:?}", paths);
                actions.send(crate::widgets::project::project_events::AddClipsEvent(paths));
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

        handle_viewport_input(&ctx, ui, panel_rect, viewport_state, response.hovered());

        // Render the frame first (OpenGL callback). Any egui overlays drawn before this
        // would be overdrawn by the callback, so keep overlays after it.
        let render_start = std::time::Instant::now();

        let renderer = viewport_renderer.clone();
        let render_state = viewport_state.render_state();
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
            callback: Arc::new(egui_glow::CallbackFn::new(move |_info, painter| {
                let gl = painter.gl();
                let mut renderer = renderer.lock().unwrap();
                if let Some((pixels, pixel_format)) = maybe_pixels.as_ref() {
                    renderer.upload_texture(gl, w, h, pixels, *pixel_format);
                }
                renderer.render(gl, &render_state);
            })),
        });

        render_time_ms = render_start.elapsed().as_secs_f32() * 1000.0;

        // Render gizmo for transform manipulation (Move/Rotate/Scale tools)
        // (must be after GL callback so it stays visible).
        let (_gizmo_consumed, gizmo_events) =
            gizmo_state.render(ui, viewport_state, project, player);
        actions.events.extend(gizmo_events);

        // Right mouse drag: "no-aim" shortcut for the active tool, without having to
        // aim at gizmo handles. This moves the gizmo center implicitly by updating
        // layer attrs via the same event bus pipeline.
        //
        // - Move: translate in screen plane (like dragging TranslateView)
        // - Rotate: rotate Z (like dragging RotateZ)
        // - Scale: uniform scale (like dragging ScaleUniform)
        if let Some(evt) = right_drag_tool_event(&ctx, panel_rect, viewport_state, player, project)
        {
            actions.events.push(evt);
            ctx.request_repaint();
        }

        // Note: Scrubbing moved to RMB in Select tool (see right_drag_tool_event)

        match frame_state {
            // Header = file comp created frame but not loaded yet
            // Loading = worker claimed frame, loading in progress
            // Composing = composition in progress (waiting for source frames)
            FrameStatus::Header | FrameStatus::Loading | FrameStatus::Composing => {
                let msg = match frame_state {
                    FrameStatus::Composing => format!("Composing frame {}...", player.current_frame(project)),
                    _ => format!("Loading frame {}...", player.current_frame(project)),
                };
                ui.painter().text(
                    panel_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    msg,
                    egui::FontId::proportional(24.0),
                    egui::Color32::from_rgba_unmultiplied(255, 255, 255, 200),
                );
                // Request repaint to check if frame finished loading
                ui.ctx().request_repaint();
            }
            FrameStatus::Error => {
                ui.painter().text(
                    panel_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    format!("Failed to load frame {}", player.current_frame(project)),
                    egui::FontId::proportional(24.0),
                    egui::Color32::from_rgb(255, 100, 100),
                );
            }
            FrameStatus::Loaded | FrameStatus::Placeholder | FrameStatus::Expired => {}
        }

        // Draw viewport overlays (scrubber, guides, etc.)
        viewport_state.draw(ui, panel_rect);
    }

    if show_help {
        render_help_overlay(ui, panel_rect);
    }

    // Shader selector overlay (top-right corner)
    egui::Area::new(ui.id().with("shader_overlay"))
        .fixed_pos(egui::pos2(
            panel_rect.max.x - 200.0,
            panel_rect.min.y + 10.0,
        ))
        .show(&ctx, |ui| {
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

    // If shader changed, recompile in renderer immediately
    if shader_manager.current_shader != old_shader
        && let Ok(mut renderer) = viewport_renderer.lock() {
            renderer.update_shader(shader_manager);
        }

    // Track hover state for input routing
    actions.hovered = response.hovered();

    (actions, render_time_ms)
}

fn handle_viewport_input(
    ctx: &egui::Context,
    _ui: &egui::Ui,
    rect: egui::Rect,
    viewport_state: &mut ViewportState,
    is_hovered: bool,
) {
    // Honor existing hover/focus routing; ignore input when cursor is outside viewport
    if !is_hovered {
        return;
    }

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

/// RMB drag handler for all tools:
/// - Select (Q): timeline scrubbing
/// - Move (W): translate layer
/// - Rotate (E): rotate layer Z
/// - Scale (R): uniform scale layer
fn right_drag_tool_event(
    ctx: &egui::Context,
    panel_rect: egui::Rect,
    viewport_state: &mut ViewportState,
    player: &Player,
    project: &Project,
) -> Option<BoxedEvent> {
    let tool = ToolMode::from_str(&project.tool());

    // Latch RMB drag only when press starts inside the viewport rect.
    let (pressed, released, down, delta, latest_pos) = ctx.input(|i| {
        (
            i.pointer.button_pressed(egui::PointerButton::Secondary),
            i.pointer.button_released(egui::PointerButton::Secondary),
            i.pointer.button_down(egui::PointerButton::Secondary),
            i.pointer.delta(),
            i.pointer.latest_pos(),
        )
    });

    if pressed {
        viewport_state.rmb_tool_drag_active =
            latest_pos.is_some_and(|p| panel_rect.contains(p));
        // Initialize scrubber on press for Select tool
        if matches!(tool, ToolMode::Select) && viewport_state.rmb_tool_drag_active {
            let bounds = viewport_state.get_image_screen_bounds();
            viewport_state.scrubber.start_scrubbing(bounds, viewport_state.image_size, 0.5);
        }
    }
    if released || !down {
        viewport_state.rmb_tool_drag_active = false;
        if matches!(tool, ToolMode::Select) {
            viewport_state.scrubber.stop_scrubbing();
        }
    }
    if !viewport_state.rmb_tool_drag_active {
        return None;
    }

    // Select tool: timeline scrubbing
    if matches!(tool, ToolMode::Select) {
        let local_x = latest_pos.map(|p| p.x - panel_rect.min.x)?;
        let comp_uuid = player.active_comp()?;
        let (play_start, play_end) = project
            .with_node(comp_uuid, |n| n.play_range(true))
            .unwrap_or((0, 100));
        
        let image_bounds = viewport_state.scrubber.frozen_bounds()
            .unwrap_or_else(|| viewport_state.get_image_screen_bounds());
        
        // Map mouse X to frame
        let frame = crate::widgets::viewport::viewport::fit(
            local_x,
            image_bounds.min.x, image_bounds.max.x,
            play_start as f32, play_end as f32,
        ).round() as i32;
        let frame_clamped = frame.clamp(play_start, play_end);
        
        viewport_state.scrubber.set_clamped(frame != frame_clamped);
        viewport_state.scrubber.set_current_frame(frame_clamped);
        viewport_state.scrubber.set_visual_x(local_x);
        
        return Some(Box::new(crate::core::player_events::SetFrameEvent(frame_clamped)));
    }

    // Transform tools: need delta movement
    if delta.length() <= 0.1 {
        return None;
    }

    let comp_uuid = player.active_comp()?;
    let selected = project
        .with_comp(comp_uuid, |comp| comp.layer_selection.clone())
        .unwrap_or_default();
    if selected.is_empty() {
        return None;
    }

    // Convert screen-space delta to comp-space pixels (Y-up).
    let zoom = viewport_state.zoom.max(0.0001);
    let delta_viewport = super::coords::screen_delta_to_viewport(delta);
    let dx_px = delta_viewport.x / zoom;
    let dy_px = delta_viewport.y / zoom;

    // Rotate/scale sensitivity: normalized by viewport size so it feels stable across resolutions.
    let min_dim = panel_rect.width().min(panel_rect.height()).max(1.0);

    let mut updates = Vec::new();
    project.with_comp(comp_uuid, |comp| {
        for layer_uuid in &selected {
            let Some(layer) = comp.get_layer(*layer_uuid) else { continue };
            let mut pos = layer.attrs.get_vec3("position").unwrap_or([0.0, 0.0, 0.0]);
            let mut rot = layer.attrs.get_vec3("rotation").unwrap_or([0.0, 0.0, 0.0]);
            let mut scale = layer.attrs.get_vec3("scale").unwrap_or([1.0, 1.0, 1.0]);

            match tool {
                ToolMode::Move => {
                    // Translate in view plane.
                    pos[0] += dx_px;
                    pos[1] += dy_px;
                }
                ToolMode::Rotate => {
                    // RotateZ: horizontal drag. Positive delta.x rotates clockwise (user space).
                    let deg_delta = (delta_viewport.x / min_dim) * 180.0;
                    rot[2] += deg_delta;
                }
                ToolMode::Scale => {
                    // Uniform scale: right/up increases, left/down decreases.
                    // Exponential mapping avoids negative scales and feels natural.
                    let norm = (delta_viewport.x + delta_viewport.y) / min_dim;
                    let factor = 2.0_f32.powf(norm);
                    scale[0] = (scale[0] * factor).clamp(0.001, 1000.0);
                    scale[1] = (scale[1] * factor).clamp(0.001, 1000.0);
                }
                ToolMode::Select => {} // handled above
            }

            updates.push((*layer_uuid, pos, rot, scale));
        }
    });

    if updates.is_empty() {
        return None;
    }

    Some(Box::new(crate::entities::comp_events::SetLayerTransformsEvent {
        comp_uuid,
        updates,
    }))
}

fn render_help_overlay(ui: &egui::Ui, panel_rect: egui::Rect) {
    ui.painter().text(
        panel_rect.left_top() + egui::vec2(10.0, 10.0),
        egui::Align2::LEFT_TOP,
        crate::ui::help_text(),
        egui::FontId::proportional(13.0),
        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 128),
    );
}
