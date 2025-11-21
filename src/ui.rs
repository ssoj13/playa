use eframe::egui;

use crate::player::Player;
use crate::widgets::timeline::{TimelineAction, TimelineConfig, TimelineState};
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

        // Timeline section (with integrated transport controls)
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

                match crate::widgets::timeline::render(ui, comp, &config, timeline_state) {
                    TimelineAction::SetFrame(new_frame) => {
                        player.set_frame(new_frame);
                    }
                    TimelineAction::SelectLayer(idx) => {
                        if let Some(comp_uuid) = &player.active_comp.clone() {
                            if let Some(comp) = player.project.media.get_mut(comp_uuid) {
                                comp.set_selected_layer(Some(idx));
                            }
                        }
                    }
                    TimelineAction::ClearSelection => {
                        if let Some(comp_uuid) = &player.active_comp.clone() {
                            if let Some(comp) = player.project.media.get_mut(comp_uuid) {
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
                        if let Some(&(frame, _)) =
                            edges.iter().find(|(f, _)| *f < comp.current_frame)
                        {
                            player.set_frame(frame);
                        }
                    }
                    TimelineAction::JumpToNextEdge => {
                        // Get child edges sorted by distance from current frame
                        let edges = comp.get_child_edges_near(comp.current_frame);

                        // Find first edge that is after current frame
                        if let Some(&(frame, _)) =
                            edges.iter().find(|(f, _)| *f > comp.current_frame)
                        {
                            player.set_frame(frame);
                        }
                    }
                    TimelineAction::AddLayer {
                        source_uuid,
                        start_frame,
                    } => {
                        if let Some(comp_uuid) = &player.active_comp.clone() {
                            // Get duration before mutable borrow to avoid borrow checker issues
                            let duration = player
                                .project
                                .media
                                .get(&source_uuid)
                                .map(|s| s.frame_count())
                                .unwrap_or(1);

                            if let Some(comp) = player.project.media.get_mut(comp_uuid) {
                                if let Err(e) =
                                    comp.add_child_with_duration(source_uuid, start_frame, duration)
                                {
                                    eprintln!("Failed to add child: {}", e);
                                }
                            }
                        }
                    }
                    TimelineAction::ReorderLayer { from_idx, to_idx } => {
                        if let Some(comp_uuid) = &player.active_comp.clone() {
                            if let Some(comp) = player.project.media.get_mut(comp_uuid) {
                                if from_idx != to_idx
                                    && from_idx < comp.children.len()
                                    && to_idx < comp.children.len()
                                {
                                    let child_uuid = comp.children.remove(from_idx);
                                    comp.children.insert(to_idx, child_uuid);
                                    comp.clear_cache();
                                }
                            }
                        }
                    }
                    TimelineAction::MoveAndReorderLayer {
                        layer_idx,
                        new_start,
                        new_idx,
                    } => {
                        eprintln!(
                            "[DEBUG] MoveAndReorderLayer: layer_idx={}, new_start={}, new_idx={}",
                            layer_idx, new_start, new_idx
                        );
                        if let Some(comp_uuid) = &player.active_comp.clone() {
                            if let Some(comp) = player.project.media.get_mut(comp_uuid) {
                                eprintln!(
                                    "[DEBUG] Active comp has {} children",
                                    comp.children.len()
                                );

                                // Step 1: Reorder if needed
                                if layer_idx != new_idx
                                    && layer_idx < comp.children.len()
                                    && new_idx < comp.children.len()
                                {
                                    eprintln!(
                                        "[DEBUG] Reordering from {} to {}",
                                        layer_idx, new_idx
                                    );
                                    let child_uuid = comp.children.remove(layer_idx);
                                    comp.children.insert(new_idx, child_uuid);
                                }

                                // Step 2: Move horizontally (use new_idx if reordered)
                                let final_idx = if layer_idx != new_idx {
                                    new_idx
                                } else {
                                    layer_idx
                                };
                                eprintln!(
                                    "[DEBUG] Moving child at index {} to start={}",
                                    final_idx, new_start
                                );

                                if let Err(e) = comp.move_child(final_idx, new_start) {
                                    eprintln!("Failed to move child: {}", e);
                                } else {
                                    eprintln!("[DEBUG] Move successful!");
                                }
                            }
                        }
                    }
                    TimelineAction::SetLayerPlayStart {
                        layer_idx,
                        new_play_start,
                    } => {
                        if let Some(comp_uuid) = &player.active_comp.clone() {
                            if let Some(comp) = player.project.media.get_mut(comp_uuid) {
                                if let Err(e) = comp.set_child_play_start(layer_idx, new_play_start)
                                {
                                    eprintln!("Failed to set child play start: {}", e);
                                }
                            }
                        }
                    }
                    TimelineAction::SetLayerPlayEnd {
                        layer_idx,
                        new_play_end,
                    } => {
                        if let Some(comp_uuid) = &player.active_comp.clone() {
                            if let Some(comp) = player.project.media.get_mut(comp_uuid) {
                                if let Err(e) = comp.set_child_play_end(layer_idx, new_play_end) {
                                    eprintln!("Failed to set child play end: {}", e);
                                }
                            }
                        }
                    }
                    TimelineAction::SetCompPlayStart { frame } => {
                        if let Some(comp_uuid) = &player.active_comp.clone() {
                            if let Some(comp) = player.project.media.get_mut(comp_uuid) {
                                let play_start = (frame as i32 - comp.start() as i32).max(0);
                                comp.set_comp_play_start(play_start);
                            }
                        }
                    }
                    TimelineAction::SetCompPlayEnd { frame } => {
                        if let Some(comp_uuid) = &player.active_comp.clone() {
                            if let Some(comp) = player.project.media.get_mut(comp_uuid) {
                                let play_end = (comp.end() as i32 - frame as i32).max(0);
                                comp.set_comp_play_end(play_end);
                            }
                        }
                    }
                    TimelineAction::ResetCompPlayArea => {
                        if let Some(comp_uuid) = &player.active_comp.clone() {
                            if let Some(comp) = player.project.media.get_mut(comp_uuid) {
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
    });

    old_shader != shader_manager.current_shader
}
