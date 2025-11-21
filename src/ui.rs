//! Top-level egui wiring for the Playa UI.
//! - Drives timeline/viewport panels using shared Player/TimelineState/Shader state.
//! - Bridges widget events into the central EventBus (set frame, layer ops, playback).
//! Data flow: UI interactions → EventBus → Player/Project/Comps → next UI frame/render.
use eframe::egui;

use crate::events::EventBus;
use crate::player::Player;
use crate::widgets::timeline::{
    TimelineAction, TimelineConfig, TimelineState, render_canvas, render_outline,
};
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

    ui.vertical(|ui| {
        // Loop and FPS info at top of panel
        ui.horizontal(|ui| {
            ui.checkbox(&mut player.loop_enabled, "Loop");
            ui.add_space(16.0);
            ui.label("FPS:");
            let fps = if player.is_playing {
                player.fps_play
            } else {
                player.fps_base
            };
            ui.label(format!("{:.2}", fps));
        });

        ui.add_space(4.0);
        ui.separator();

        // Timeline section (split: outline + canvas)
        if let Some(comp_uuid) = &player.active_comp.clone() {
            if let Some(comp) = player.project.media.get_mut(comp_uuid) {
                let mut config = TimelineConfig::default();
                config.show_frame_numbers = timeline_state.show_frame_numbers;

                // Recenter pan when switching comps so start is visible (supports negative starts)
                if timeline_state
                    .last_comp_uuid
                    .as_ref()
                    .map(|u| u != comp_uuid)
                    .unwrap_or(true)
                {
                    timeline_state.pan_offset = comp.start() as f32;
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

                let active_comp_uuid = player.active_comp.clone();

                let splitter_height = ui.available_height();
                let mut timeline_actions: Vec<TimelineAction> = Vec::new();

                egui::SidePanel::left("timeline_outline")
                    .resizable(true)
                    .min_width(100.0)
                    .max_width(400.0)
                    .show_inside(ui, |ui| {
                        ui.set_height(splitter_height);
                        render_outline(ui, comp, &config, timeline_state, |act| {
                            timeline_actions.push(act);
                        });
                    });

                egui::CentralPanel::default().show_inside(ui, |ui| {
                    ui.set_height(splitter_height);
                    render_canvas(ui, comp, &config, timeline_state, |act| {
                        timeline_actions.push(act);
                    });
                });

                for act in timeline_actions {
                    dispatch_timeline_action(act, &active_comp_uuid, event_bus, comp);
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

/// Dispatch TimelineAction to EventBus.
fn dispatch_timeline_action(
    action: TimelineAction,
    active_comp_uuid: &Option<String>,
    event_bus: &EventBus,
    comp: &crate::entities::Comp,
) {
    match action {
        TimelineAction::SetFrame(new_frame) => {
            event_bus.send(crate::events::AppEvent::SetFrame(new_frame));
        }
        TimelineAction::SelectLayer(idx) => {
            event_bus.send(crate::events::AppEvent::SelectLayer(idx));
        }
        TimelineAction::ClearSelection => {
            event_bus.send(crate::events::AppEvent::DeselectLayer);
        }
        TimelineAction::ToStart => {
            event_bus.send(crate::events::AppEvent::JumpToStart);
        }
        TimelineAction::ToEnd => {
            event_bus.send(crate::events::AppEvent::JumpToEnd);
        }
        TimelineAction::TogglePlay => {
            event_bus.send(crate::events::AppEvent::TogglePlayPause);
        }
        TimelineAction::Stop => {
            event_bus.send(crate::events::AppEvent::Stop);
        }
        TimelineAction::JumpToPrevEdge => {
            if let Some(frame) = comp
                .get_child_edges_near(comp.current_frame)
                .iter()
                .find(|(f, _)| *f < comp.current_frame)
                .map(|(f, _)| *f)
            {
                event_bus.send(crate::events::AppEvent::SetFrame(frame));
            }
        }
        TimelineAction::JumpToNextEdge => {
            if let Some(frame) = comp
                .get_child_edges_near(comp.current_frame)
                .iter()
                .find(|(f, _)| *f > comp.current_frame)
                .map(|(f, _)| *f)
            {
                event_bus.send(crate::events::AppEvent::SetFrame(frame));
            }
        }
        TimelineAction::AddLayer {
            source_uuid,
            start_frame,
        } => {
            if let Some(comp_uuid) = active_comp_uuid.clone() {
                event_bus.send(crate::events::AppEvent::AddLayer {
                    comp_uuid,
                    source_uuid,
                    start_frame,
                });
            }
        }
        TimelineAction::ReorderLayer { from_idx, to_idx } => {
            if let Some(comp_uuid) = active_comp_uuid.clone() {
                event_bus.send(crate::events::AppEvent::ReorderLayer {
                    comp_uuid,
                    from_idx,
                    to_idx,
                });
            }
        }
        TimelineAction::MoveAndReorderLayer {
            layer_idx,
            new_start,
            new_idx,
        } => {
            if let Some(comp_uuid) = active_comp_uuid.clone() {
                event_bus.send(crate::events::AppEvent::MoveAndReorderLayer {
                    comp_uuid,
                    layer_idx,
                    new_start,
                    new_idx,
                });
            }
        }
        TimelineAction::SetLayerPlayStart {
            layer_idx,
            new_play_start,
        } => {
            if let Some(comp_uuid) = active_comp_uuid.clone() {
                event_bus.send(crate::events::AppEvent::SetLayerPlayStart {
                    comp_uuid,
                    layer_idx,
                    new_play_start,
                });
            }
        }
        TimelineAction::SetLayerPlayEnd {
            layer_idx,
            new_play_end,
        } => {
            if let Some(comp_uuid) = active_comp_uuid.clone() {
                event_bus.send(crate::events::AppEvent::SetLayerPlayEnd {
                    comp_uuid,
                    layer_idx,
                    new_play_end,
                });
            }
        }
        TimelineAction::SetCompPlayStart { frame } => {
            if let Some(comp_uuid) = active_comp_uuid.clone() {
                event_bus.send(crate::events::AppEvent::SetCompPlayStart { comp_uuid, frame });
            }
        }
        TimelineAction::SetCompPlayEnd { frame } => {
            if let Some(comp_uuid) = active_comp_uuid.clone() {
                event_bus.send(crate::events::AppEvent::SetCompPlayEnd { comp_uuid, frame });
            }
        }
        TimelineAction::ResetCompPlayArea => {
            if let Some(comp_uuid) = active_comp_uuid.clone() {
                event_bus.send(crate::events::AppEvent::ResetCompPlayArea { comp_uuid });
            }
        }
        TimelineAction::None => {}
    }
}
