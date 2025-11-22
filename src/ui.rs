//! Top-level egui wiring for the Playa UI.
//! - Drives timeline/viewport panels using shared Player/TimelineState/Shader state.
//! - Bridges widget events into the central EventBus (set frame, layer ops, playback).
//! Data flow: UI interactions → EventBus → Player/Project/Comps → next UI frame/render.
use eframe::egui;

use crate::events::EventBus;
use crate::player::Player;
use crate::widgets::timeline::{TimelineConfig, TimelineState, render_canvas, render_outline};
use crate::widgets::viewport::shaders::Shaders;

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

/// Render timeline panel inside a dock tab. Returns true if shader changed.
pub fn render_timeline_panel(
    ui: &mut egui::Ui,
    player: &mut Player,
    shader_manager: &mut Shaders,
    timeline_state: &mut TimelineState,
    event_bus: &EventBus,
) -> bool {
    let old_shader = shader_manager.current_shader.clone();

    // Block vertical scroll - timeline panel should not scroll vertically
    let available_height = ui.available_height();
    ui.vertical(|ui| {
        ui.set_min_height(available_height);
        ui.set_max_height(available_height);

        // Timeline section (split: outline + canvas)
        if let Some(comp_uuid) = &player.active_comp.clone() {
            if let Some(comp) = player.project.media.get_mut(comp_uuid) {
                let config = TimelineConfig::default();

                // Reset pan to frame 0 when switching comps (ruler shows absolute frame numbers)
                if timeline_state
                    .last_comp_uuid
                    .as_ref()
                    .map(|u| u != comp_uuid)
                    .unwrap_or(true)
                {
                    timeline_state.pan_offset = 0.0;
                    timeline_state.last_comp_uuid = Some(comp_uuid.clone());
                }

                // Recalculate bounds on activation; realign play_range only if it matched full range
                let old_start = comp.start();
                let old_end = comp.end();
                let old_play = comp.play_range();
                comp.rebound();
                if old_play == (old_start, old_end) {
                    comp.set_comp_play_start(0);
                    comp.set_comp_play_end(0);
                }

                let splitter_height = ui.available_height();

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

                match timeline_state.view_mode {
                    crate::widgets::timeline::TimelineViewMode::Split => {
                        egui::SidePanel::left("timeline_outline")
                            .resizable(true)
                            .min_width(100.0)
                            .max_width(400.0)
                            .show_inside(ui, |ui| {
                                ui.set_height(splitter_height);
                                render_outline(
                                    ui,
                                    comp_uuid,
                                    comp,
                                    &config,
                                    timeline_state,
                                    timeline_state.view_mode,
                                    |evt| event_bus.send(evt),
                                );
                            });

                        egui::CentralPanel::default().show_inside(ui, |ui| {
                            ui.set_height(splitter_height);
                            render_canvas(ui, comp_uuid, comp, &config, timeline_state, timeline_state.view_mode, |evt| {
                                event_bus.send(evt)
                            });
                        });
                    }
                    crate::widgets::timeline::TimelineViewMode::CanvasOnly => {
                        egui::CentralPanel::default().show_inside(ui, |ui| {
                            ui.set_height(splitter_height);
                            render_canvas(ui, comp_uuid, comp, &config, timeline_state, timeline_state.view_mode, |evt| {
                                event_bus.send(evt)
                            });
                        });
                    }
                    crate::widgets::timeline::TimelineViewMode::OutlineOnly => {
                        egui::CentralPanel::default().show_inside(ui, |ui| {
                            ui.set_height(splitter_height);
                            render_outline(ui, comp_uuid, comp, &config, timeline_state, timeline_state.view_mode, |evt| {
                                event_bus.send(evt)
                            });
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

    old_shader != shader_manager.current_shader
}
