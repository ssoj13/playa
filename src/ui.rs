//! Top-level egui wiring for the Playa UI.
//! - Drives timeline/viewport panels using shared Player/TimelineState/Shader state.
//! - Bridges widget events into the central EventBus (set frame, layer ops, playback).
//! Data flow: UI interactions → EventBus → Player/Project/Comps → next UI frame/render.
use eframe::egui;

use crate::entities::Project;
use crate::core::event_bus::EventBus;
use crate::core::player::Player;
use crate::widgets::timeline::{
    TimelineConfig, TimelineState, render_canvas, render_outline, render_toolbar,
};
use crate::widgets::viewport::shaders::Shaders;

/// Help text displayed in overlay
pub fn help_text() -> &'static str {
    "Drag'n'drop a file here or double-click to open\n\n\
    Hotkeys:\n\
    F1 - Toggle this help\n\
    F2 - Toggle Project panel\n\
    F3 - Toggle Attributes panel\n\
    F4 - Toggle Encoder dialog\n\
    F5 - Toggle Preferences\n\
    ESC - Exit Fullscreen / Quit\n\n\
    Z - Toggle Fullscreen\n\
    Ctrl+R - Reset Settings\n\
    Backspace - Toggle Frame Numbers\n\n\
    ` - Toggle Loop\n\
    B - Set Play Range Start\n\
    N - Set Play Range End\n\
    Ctrl+B - Reset Play Range\n\n\
    Playback:\n\
    Space / ↑ - Play/Pause Toggle\n\
    K / . / ↓ - Stop\n\
    J / , - Jog Backward (accelerates)\n\
    L / / - Jog Forward (accelerates)\n\n\
    Frame Navigation:\n\
    ← → - Step 1 frame\n\
    PgUp/PgDn - Step 1 frame\n\
    Shift+Arrows/PgUp/PgDn - Step 25 frames\n\
    Ctrl+Arrows/PgUp/PgDn - Jump to Start/End\n\
    1 / Home - Jump to Start\n\
    2 / End - Jump to End\n\
    ; - Jump to Previous Layer Edge\n\
    ' - Jump to Next Layer Edge\n\n\
    Timeline (Layer Operations):\n\
    [ - Align Layer Start to Cursor\n\
    ] - Align Layer End to Cursor\n\
    Alt+[ - Trim Layer Start to Cursor\n\
    Alt+] - Trim Layer End to Cursor\n\
    Delete - Remove Selected Layer\n\
    F - Fit Timeline\n\
    A - Reset Timeline Zoom\n\n\
    FPS Control (Presets):\n\
    - - Decrease Base FPS\n\
    = / + - Increase Base FPS\n\n\
    Viewport:\n\
    A / H - 100% Zoom\n\
    F - Fit to View\n\n\
    Mouse:\n\
    Mouse Wheel - Zoom\n\
    Middle Drag - Pan\n\
    Left Click - Scrub"
}

/// Render timeline panel inside a dock tab. Returns true if shader changed.
pub fn render_timeline_panel(
    ui: &mut egui::Ui,
    player: &mut Player,
    project: &Project,
    shader_manager: &mut Shaders,
    timeline_state: &mut TimelineState,
    event_bus: &EventBus,
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

            let media = project.media.read().expect("media lock poisoned");
            if let Some(comp) = media.get(&comp_uuid) {
                let config = TimelineConfig::default();

                // CRITICAL ORDER: Toolbar and view selector MUST be rendered BEFORE calculating
                // splitter_height. If we calculate height first, then render toolbar (which takes
                // ~45px), the panels will receive incorrect height and egui will add unwanted
                // vertical scrollbar. By rendering fixed-height elements first, available_height()
                // returns the correct remaining space for panels.

                // Toolbar first (before view selector) - takes ~30px
                render_toolbar(ui, timeline_state, |evt| event_bus.emit_boxed(evt));
                ui.add_space(4.0);

                // View selector (Split/Canvas/Outline buttons) - takes ~20px
                ui.horizontal(|ui| {
                    ui.label("View:");
                    for (label, mode) in [
                        ("Split", crate::widgets::timeline::TimelineViewMode::Split),
                        (
                            "Canvas",
                            crate::widgets::timeline::TimelineViewMode::CanvasOnly,
                        ),
                        (
                            "Outline",
                            crate::widgets::timeline::TimelineViewMode::OutlineOnly,
                        ),
                    ] {
                        let selected = timeline_state.view_mode == mode;
                        if ui.selectable_label(selected, label).clicked() {
                            timeline_state.view_mode = mode;
                        }
                    }
                });
                ui.add_space(4.0);

                // Now calculate remaining height for panels (after toolbar ~30px + view selector ~20px)
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

                        egui::CentralPanel::default().show_inside(ui, |ui| {
                            // Same as outline: lock to exact height to prevent unwanted vertical scroll
                            ui.set_height(splitter_height);
                            ui.set_max_height(splitter_height);
                            timeline_actions = render_canvas(
                                ui,
                                comp_uuid,
                                comp,
                                &config,
                                timeline_state,
                                timeline_state.view_mode,
                                |evt| event_bus.emit_boxed(evt),
                            );
                        });
                    }
                    crate::widgets::timeline::TimelineViewMode::CanvasOnly => {
                        egui::CentralPanel::default().show_inside(ui, |ui| {
                            ui.set_height(splitter_height);
                            ui.set_max_height(splitter_height);
                            timeline_actions = render_canvas(
                                ui,
                                comp_uuid,
                                comp,
                                &config,
                                timeline_state,
                                timeline_state.view_mode,
                                |evt| event_bus.emit_boxed(evt),
                            );
                        });
                    }
                    crate::widgets::timeline::TimelineViewMode::OutlineOnly => {
                        egui::CentralPanel::default().show_inside(ui, |ui| {
                            ui.set_height(splitter_height);
                            ui.set_max_height(splitter_height);
                            render_outline(
                                ui,
                                comp_uuid,
                                comp,
                                &config,
                                timeline_state,
                                timeline_state.view_mode,
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
