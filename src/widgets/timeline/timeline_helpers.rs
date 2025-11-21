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
                let initial_play_start = attrs.get_i32("play_start").unwrap_or(0);
                GlobalDragState::AdjustPlayStart {
                    layer_idx,
                    initial_play_start,
                    drag_start_x: drag_start_pos.x,
                }
            }
            LayerTool::AdjustPlayEnd => {
                let initial_play_end = attrs.get_i32("play_end").unwrap_or(0);
                GlobalDragState::AdjustPlayEnd {
                    layer_idx,
                    initial_play_end,
                    drag_start_x: drag_start_pos.x,
                }
            }
            LayerTool::Move => {
                let initial_start = attrs.get_i32("start").unwrap_or(0);
                let initial_end = attrs.get_i32("end").unwrap_or(0);
                GlobalDragState::MovingLayer {
                    layer_idx,
                    initial_start,
                    initial_end,
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
) -> (Option<i32>, Rect) {
    let total_frames = comp.frame_count();
    let ruler_height = 20.0;

    let (frame, rect) = ui
        .horizontal(|ui| {
            ui.allocate_exact_size(
                Vec2::new(config.name_column_width, ruler_height),
                Sense::hover(),
            );

            let (rect, ruler_response) = ui.allocate_exact_size(
                Vec2::new(timeline_width, ruler_height),
                Sense::click_and_drag(),
            );

            if ui.is_rect_visible(rect) {
                let painter = ui.painter();
                painter.rect_filled(rect, 0.0, Color32::from_gray(25));

                let playhead_x =
                    frame_to_screen_x(comp.current_frame as f32, rect.min.x, config, state);
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

                let visible_start = state.pan_offset.max(0.0) as usize;
                let visible_end = (state.pan_offset + (timeline_width / effective_ppf))
                    .min(total_frames as f32) as usize;
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

                if ruler_response.clicked() || ruler_response.dragged() {
                    if let Some(pos) = ruler_response.interact_pointer_pos() {
                        let frame =
                            screen_x_to_frame(pos.x, rect.min.x, config, state).round() as i32;
                        return (Some(frame.min(total_frames.saturating_sub(1))), rect);
                    }
                }
            }

            (None, rect)
        })
        .inner;

    (frame, rect)
}

pub(super) fn draw_playhead(
    painter: &egui::Painter,
    timeline_rect: Rect,
    current_frame: usize,
    config: &TimelineConfig,
    state: &TimelineState,
) {
    let x = frame_to_screen_x(current_frame as f32, timeline_rect.min.x, config, state);
    painter.line_segment(
        [
            Pos2::new(x, timeline_rect.min.y),
            Pos2::new(x, timeline_rect.max.y),
        ],
        (2.0, Color32::from_rgb(255, 220, 100)),
    );

    let triangle_size = 8.0;
    let top_y = timeline_rect.min.y;
    let points = [
        Pos2::new(x, top_y),
        Pos2::new(x - triangle_size / 2.0, top_y - triangle_size),
        Pos2::new(x + triangle_size / 2.0, top_y - triangle_size),
    ];
    painter.add(egui::Shape::convex_polygon(
        points.to_vec(),
        Color32::from_rgb(255, 220, 100),
        (0.0, Color32::TRANSPARENT),
    ));
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

pub(super) fn hash_color(s: &str) -> Color32 {
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
