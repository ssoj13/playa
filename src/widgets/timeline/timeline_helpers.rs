//! Timeline UI helpers: tools, math and drawing utilities.
use crate::entities::Comp;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Ui, Vec2};

use super::{GlobalDragState, TimelineConfig, TimelineState};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum LayerTool {
    AdjustPlayStart,
    AdjustPlayEnd,
    Move,
}

impl LayerTool {
    pub(super) fn cursor(&self) -> egui::CursorIcon {
        match self {
            LayerTool::AdjustPlayStart | LayerTool::AdjustPlayEnd => {
                egui::CursorIcon::ResizeHorizontal
            }
            LayerTool::Move => egui::CursorIcon::Grab,
        }
    }

    pub(super) fn to_drag_state(
        &self,
        layer_idx: usize,
        attrs: &crate::entities::Attrs,
        drag_start_pos: Pos2,
    ) -> GlobalDragState {
        match self {
            LayerTool::AdjustPlayStart => {
                let default_start = attrs.get_i32_or_zero("in");
                let initial_play_start = attrs.get_i32_or("trim_in", default_start);
                GlobalDragState::AdjustPlayStart {
                    layer_idx,
                    initial_play_start,
                    drag_start_x: drag_start_pos.x,
                }
            }
            LayerTool::AdjustPlayEnd => {
                let default_end = attrs.get_i32_or_zero("out");
                let initial_play_end = attrs.get_i32_or("trim_out", default_end);
                GlobalDragState::AdjustPlayEnd {
                    layer_idx,
                    initial_play_end,
                    drag_start_x: drag_start_pos.x,
                }
            }
            LayerTool::Move => {
                let initial_start = attrs.get_i32_or_zero("in");
                GlobalDragState::MovingLayer {
                    layer_idx,
                    initial_start,
                    drag_start_x: drag_start_pos.x,
                    drag_start_y: drag_start_pos.y,
                }
            }
        }
    }
}

pub(super) fn detect_layer_tool(
    hover_pos: Pos2,
    bar_rect: Rect,
    edge_threshold: f32,
) -> Option<LayerTool> {
    // Allow grabbing slightly outside the bar to extend trims
    if !bar_rect.expand(edge_threshold).contains(hover_pos) {
        return None;
    }

    let dist_to_left = (hover_pos.x - bar_rect.min.x).abs();
    let dist_to_right = (hover_pos.x - bar_rect.max.x).abs();

    if dist_to_left < edge_threshold {
        Some(LayerTool::AdjustPlayStart)
    } else if dist_to_right < edge_threshold {
        Some(LayerTool::AdjustPlayEnd)
    } else if bar_rect.contains(hover_pos) {
        Some(LayerTool::Move)
    } else {
        None
    }
}

pub(super) fn frame_to_screen_x(
    frame: f32,
    timeline_rect_min_x: f32,
    config: &TimelineConfig,
    state: &TimelineState,
) -> f32 {
    timeline_rect_min_x + (frame - state.pan_offset) * config.pixels_per_frame * state.zoom
}

pub(super) fn screen_x_to_frame(
    x: f32,
    timeline_rect_min_x: f32,
    config: &TimelineConfig,
    state: &TimelineState,
) -> f32 {
    ((x - timeline_rect_min_x) / (config.pixels_per_frame * state.zoom)) + state.pan_offset
}

pub(super) fn draw_frame_ruler(
    ui: &mut Ui,
    comp: &Comp,
    config: &TimelineConfig,
    state: &TimelineState,
    timeline_width: f32,
    total_frames: i32,
) -> (Option<i32>, Rect) {
    let ruler_height = 20.0;

    let (rect, ruler_response) = ui.allocate_exact_size(
        Vec2::new(timeline_width, ruler_height),
        Sense::click_and_drag(),
    );

    let mut frame_clicked = None;

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect_filled(rect, 0.0, Color32::from_gray(25));

        let playhead_x = frame_to_screen_x(comp.frame() as f32, rect.min.x, config, state);
        if playhead_x >= rect.min.x && playhead_x <= rect.max.x {
            painter.line_segment(
                [
                    Pos2::new(playhead_x, rect.min.y),
                    Pos2::new(playhead_x, rect.max.y),
                ],
                (2.0, Color32::from_rgb(255, 220, 100)),
            );
        }

        let effective_ppf = config.pixels_per_frame * state.zoom;
        let frame_step = if effective_ppf > 10.0 {
            1
        } else if effective_ppf > 2.0 {
            5
        } else if effective_ppf > 0.5 {
            10
        } else {
            50
        };

        let label_step = if effective_ppf > 50.0 {
            10
        } else if effective_ppf > 20.0 {
            5
        } else {
            (frame_step * 2).max(frame_step)
        };

        // Use rect.width() for visible range, not timeline_width
        let visible_start = state.pan_offset.max(0.0) as usize;
        let visible_end =
            (state.pan_offset + (rect.width() / effective_ppf)).min(total_frames as f32) as usize;
        let start_frame = (visible_start / frame_step.max(1)) * frame_step.max(1);

        for frame in (start_frame..=visible_end).step_by(frame_step.max(1)) {
            let x = frame_to_screen_x(frame as f32, rect.min.x, config, state);
            if x < rect.min.x || x > rect.max.x {
                continue;
            }

            painter.line_segment(
                [Pos2::new(x, rect.max.y - 5.0), Pos2::new(x, rect.max.y)],
                (1.0, Color32::from_gray(100)),
            );

            if frame % label_step == 0 {
                painter.text(
                    Pos2::new(x, rect.min.y + 2.0),
                    egui::Align2::CENTER_TOP,
                    format!("{}", frame),
                    egui::FontId::monospace(9.0),
                    Color32::from_gray(150),
                );
            }
        }

        let is_middle_down = ui
            .ctx()
            .input(|i| i.pointer.button_down(egui::PointerButton::Middle));

        if !is_middle_down && (ruler_response.clicked() || ruler_response.dragged()) {
            if let Some(pos) = ruler_response.interact_pointer_pos() {
                let frame = screen_x_to_frame(pos.x, rect.min.x, config, state).round() as i32;
                frame_clicked = Some(frame.min(total_frames.saturating_sub(1)));
            }
        }
    }

    (frame_clicked, rect)
}

/// Compute visual rows for ALL layers using greedy layout algorithm.
/// Returns mapping of child_idx -> row. Delegates to Comp::compute_layer_rows.
pub(super) fn compute_all_layer_rows(
    comp: &Comp,
    child_order: &[usize],
) -> std::collections::HashMap<usize, usize> {
    comp.compute_layer_rows(child_order)
}

/// Convert row index to Y coordinate in timeline
pub(super) fn row_to_y(row: usize, config: &TimelineConfig, timeline_rect: Rect) -> f32 {
    timeline_rect.min.y + (row as f32 * config.layer_height)
}

/// Draw drop preview (ghost) using the standard layer move style.
pub(super) fn draw_drop_preview(
    painter: &egui::Painter,
    frame: i32,
    row_y: f32,
    duration: i32,
    timeline_rect: Rect,
    config: &TimelineConfig,
    state: &TimelineState,
) {
    let start_x = frame_to_screen_x(frame as f32, timeline_rect.min.x, config, state);
    let end_x = frame_to_screen_x(
        (frame + duration) as f32,
        timeline_rect.min.x,
        config,
        state,
    );
    let bar_height = (config.layer_height - 8.0).max(2.0);
    let thumb_rect = Rect::from_min_max(
        Pos2::new(start_x, row_y + 4.0),
        Pos2::new(end_x, row_y + 4.0 + bar_height),
    );
    painter.rect_stroke(
        thumb_rect,
        4.0,
        egui::Stroke::new(2.0, Color32::from_rgba_unmultiplied(100, 220, 255, 180)),
        egui::epaint::StrokeKind::Middle,
    );
}

pub(super) fn hash_color_str(s: &str) -> Color32 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    let hash = hasher.finish();
    let hue = (hash % 360) as f32;
    let saturation = 0.65;
    let value = 0.55;
    hsv_to_rgb(hue, saturation, value)
}

pub(super) fn hsv_to_rgb(h: f32, s: f32, v: f32) -> Color32 {
    let c = v * s;
    let h_prime = h / 60.0;
    let x = c * (1.0 - ((h_prime % 2.0) - 1.0).abs());
    let m = v - c;

    let (r, g, b) = if h_prime < 1.0 {
        (c, x, 0.0)
    } else if h_prime < 2.0 {
        (x, c, 0.0)
    } else if h_prime < 3.0 {
        (0.0, c, x)
    } else if h_prime < 4.0 {
        (0.0, x, c)
    } else if h_prime < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    Color32::from_rgb(
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}
