//! Top-level egui wiring for the Playa UI.
//!
//! - Drives timeline/viewport panels using shared Player/TimelineState/Shader state.
//! - Bridges widget events into the central EventBus (set frame, layer ops, playback).
//!
//! # Panel Layout
//!
//! Split mode uses `egui::Frame::NONE` on both SidePanel and CentralPanel
//! to ensure consistent alignment between outline and canvas areas.
//! This removes default panel margins that caused visual offsets.
//!
//! # Coordinate Systems
//!
//! egui uses absolute screen coordinates for mouse events. When working with
//! panels (Viewport, Timeline, Project, AE), we need to convert between:
//! - **Screen coords**: Absolute position on the application window
//! - **Local coords**: Position relative to a panel's top-left corner
//!
//! Use `screen_to_local()` and `local_to_screen()` for conversions.
//!
//! Data flow: UI interactions → EventBus → Player/Project/Comps → next UI frame/render.
use eframe::egui;
use egui::{Pos2, Rect, Vec2};

// ============================================================================
// Coordinate Conversion Utilities
// ============================================================================

/// Convert absolute screen coordinates to local coordinates relative to panel.
///
/// Use this when handling mouse events inside a panel - `response.interact_pointer_pos()`
/// returns screen coords, but panel-internal logic often needs local coords.
///
/// # Example
/// ```ignore
/// let mouse_screen = response.interact_pointer_pos().unwrap();
/// let mouse_local = screen_to_local(mouse_screen, panel_rect);
/// // mouse_local.x is now 0..panel_width, not absolute screen X
/// ```
pub fn screen_to_local(screen_pos: Pos2, panel_rect: Rect) -> Vec2 {
    Vec2::new(
        screen_pos.x - panel_rect.min.x,
        screen_pos.y - panel_rect.min.y,
    )
}

/// Convert local panel coordinates to absolute screen coordinates.
///
/// Use this when you need to draw something at a position calculated in local
/// coords, or when passing coords to egui drawing functions.
///
/// # Example
/// ```ignore
/// let local_pos = Vec2::new(100.0, 50.0); // 100px from panel left, 50px from top
/// let screen_pos = local_to_screen(local_pos, panel_rect);
/// painter.circle_filled(screen_pos, 5.0, Color32::RED);
/// ```
pub fn local_to_screen(local_pos: Vec2, panel_rect: Rect) -> Pos2 {
    Pos2::new(
        local_pos.x + panel_rect.min.x,
        local_pos.y + panel_rect.min.y,
    )
}

/// Check if a screen position is inside a panel's bounds.
///
/// Useful for determining which panel has focus when handling global keyboard
/// events, or for hit-testing before processing mouse events.
///
/// # Example
/// ```ignore
/// if is_in_panel(cursor_pos, viewport_rect) {
///     // Handle viewport-specific shortcuts
/// } else if is_in_panel(cursor_pos, timeline_rect) {
///     // Handle timeline-specific shortcuts  
/// }
/// ```
pub fn is_in_panel(screen_pos: Pos2, panel_rect: Rect) -> bool {
    panel_rect.contains(screen_pos)
}

/// Get normalized position within panel (0.0..1.0 for both axes).
///
/// Useful for proportional calculations like scrubber position mapping.
/// Returns None if position is outside panel bounds.
///
/// # Example
/// ```ignore
/// if let Some(norm) = screen_to_normalized(mouse_pos, panel_rect) {
///     let frame = lerp(play_start, play_end, norm.x);
/// }
/// ```
pub fn screen_to_normalized(screen_pos: Pos2, panel_rect: Rect) -> Option<Vec2> {
    if !panel_rect.contains(screen_pos) {
        return None;
    }
    let local = screen_to_local(screen_pos, panel_rect);
    Some(Vec2::new(
        local.x / panel_rect.width(),
        local.y / panel_rect.height(),
    ))
}

use crate::entities::Project;
use crate::core::event_bus::EventBus;
use crate::core::player::Player;
use crate::widgets::timeline::{
    TimelineConfig, TimelineState, render_canvas, render_outline, render_toolbar,
};
use crate::widgets::viewport::shaders::Shaders;

/// Render timeline panel inside a dock tab. Returns true if shader changed.
pub fn render_timeline_panel(
    ui: &mut egui::Ui,
    player: &mut Player,
    project: &Project,
    shader_manager: &mut Shaders,
    timeline_state: &mut TimelineState,
    event_bus: &EventBus,
    show_tooltips: bool,
    layer_height: f32,
    name_column_width: f32,
    outline_top_offset: f32,
    layout_names: &[String],
    current_layout: &str,
    timeline_hover_highlight: bool,
) -> (bool, crate::widgets::timeline::TimelineActions) {
    let old_shader = shader_manager.current_shader.clone();
    let mut timeline_actions = crate::widgets::timeline::TimelineActions::default();

    // Block vertical scroll - timeline panel should not scroll vertically
    let available_height = ui.available_height();
    ui.vertical(|ui| {
        ui.set_min_height(available_height);
        ui.set_max_height(available_height);

        // Timeline section (split: outline + canvas)
        if let Some(comp_uuid) = player.active_comp() {
            // Reset pan to frame 0 when switching comps (ruler shows absolute frame numbers)
            if timeline_state
                .last_comp_uuid
                .map(|u| u != comp_uuid)
                .unwrap_or(true)
            {
                timeline_state.pan_offset = 0.0;
                timeline_state.last_comp_uuid = Some(comp_uuid);
            }

            if let Some(comp) = project.clone_comp(comp_uuid) {
                let comp = &comp; // reborrow for compatibility
                let config = TimelineConfig::new(layer_height, name_column_width);

                // CRITICAL ORDER: Toolbar MUST be rendered BEFORE calculating splitter_height.
                // If we calculate height first, then render toolbar, the panels will receive
                // incorrect height and egui will add unwanted vertical scrollbar.

                // Toolbar with transport, zoom, snap, lock, loop, and view mode selector
                render_toolbar(ui, timeline_state, player.loop_enabled(), show_tooltips, layout_names, current_layout, |evt| event_bus.emit_boxed(evt));
                ui.add_space(4.0);

                // Now calculate remaining height for panels
                let splitter_height = ui.available_height();

                match timeline_state.view_mode {
                    crate::widgets::timeline::TimelineViewMode::Split => {
                        // Ensure outline_width is at least 400px (default) if it's too small
                        // This prevents the splitter from being too narrow after loading saved state
                        let saved_width = timeline_state.outline_width.max(400.0);
                        let outline_response = egui::SidePanel::left("timeline_outline")
                            .resizable(true)
                            .min_width(100.0)
                            .default_width(saved_width)
                            .frame(egui::Frame::NONE)  // Remove default frame to align with canvas
                            .show_inside(ui, |ui| {
                                // Lock panel to exact height to prevent vertical scrollbar.
                                // set_height() alone is not enough - egui can still add scrollbar
                                // if content exceeds height. set_max_height() enforces hard limit.
                                ui.set_height(splitter_height);
                                ui.set_max_height(splitter_height);
                                render_outline(
                                    ui,
                                    comp_uuid,
                                    comp,
                                    &config,
                                    timeline_state,
                                    timeline_state.view_mode,
                                    outline_top_offset,
                                    |evt| event_bus.emit_boxed(evt),
                                );
                            });

                        // Update persistent outline width only if significantly changed (>1px) AND
                        // the new width is reasonable (not the minimum width, which egui may set
                        // during initialization). This prevents overwriting saved width with temporary
                        // values during UI initialization.
                        let new_width = outline_response.response.rect.width();
                        // Only update if:
                        // 1. The difference is significant (>1px)
                        // 2. The new width is not the minimum width (100px) - this prevents reset on first frame
                        // 3. The new width is reasonable (>= 150px) - this ensures we don't save invalid values
                        // 4. The new width is not significantly smaller than the saved width (user didn't collapse it)
                        if (new_width - timeline_state.outline_width).abs() > 1.0
                            && new_width >= 150.0
                            && new_width != 100.0
                            && new_width >= timeline_state.outline_width * 0.5 // Don't save if collapsed to <50% of saved width
                        {
                            timeline_state.outline_width = new_width.max(400.0); // Ensure minimum 400px
                        }

                        egui::CentralPanel::default()
                            .frame(egui::Frame::NONE)  // Remove default frame to align with outline
                            .show_inside(ui, |ui| {
                            // Same as outline: lock to exact height to prevent unwanted vertical scroll
                            ui.set_height(splitter_height);
                            ui.set_max_height(splitter_height);
                            timeline_actions = render_canvas(
                                ui,
                                comp_uuid,
                                comp,
                                project,
                                &config,
                                timeline_state,
                                timeline_state.view_mode,
                                timeline_hover_highlight,
                                |evt| event_bus.emit_boxed(evt),
                            );
                        });
                    }
                    crate::widgets::timeline::TimelineViewMode::CanvasOnly => {
                        egui::CentralPanel::default()
                            .frame(egui::Frame::NONE)
                            .show_inside(ui, |ui| {
                            ui.set_height(splitter_height);
                            ui.set_max_height(splitter_height);
                            timeline_actions = render_canvas(
                                ui,
                                comp_uuid,
                                comp,
                                project,
                                &config,
                                timeline_state,
                                timeline_state.view_mode,
                                timeline_hover_highlight,
                                |evt| event_bus.emit_boxed(evt),
                            );
                        });
                    }
                    crate::widgets::timeline::TimelineViewMode::OutlineOnly => {
                        egui::CentralPanel::default()
                            .frame(egui::Frame::NONE)
                            .show_inside(ui, |ui| {
                            ui.set_height(splitter_height);
                            ui.set_max_height(splitter_height);
                            render_outline(
                                ui,
                                comp_uuid,
                                comp,
                                &config,
                                timeline_state,
                                timeline_state.view_mode,
                                outline_top_offset,
                                |evt| event_bus.emit_boxed(evt),
                            );
                        });
                    }
                }
            }
        } else {
            ui.centered_and_justified(|ui| {
                ui.label("No active composition");
            });
        }
    });

    (
        old_shader != shader_manager.current_shader,
        timeline_actions,
    )
}
