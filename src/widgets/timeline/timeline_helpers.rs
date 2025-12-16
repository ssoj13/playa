//! Timeline UI helpers: tools, math and drawing utilities.
use crate::entities::Comp;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Ui, Vec2};

use super::{GlobalDragState, TimelineConfig, TimelineState};
use crate::entities::keys::{A_IN, A_SPEED, A_TRIM_IN, A_TRIM_OUT};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum LayerTool {
    AdjustPlayStart,
    AdjustPlayEnd,
    Move,
    /// Slide tool - drag in trim zones to slide layer in/out while keeping visible content in place
    Slide,
}

impl LayerTool {
    pub(super) fn cursor(&self) -> egui::CursorIcon {
        match self {
            LayerTool::AdjustPlayStart | LayerTool::AdjustPlayEnd => {
                egui::CursorIcon::ResizeHorizontal
            }
            LayerTool::Move => egui::CursorIcon::Grab,
            LayerTool::Slide => egui::CursorIcon::ResizeColumn,
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
                GlobalDragState::AdjustPlayStart {
                    layer_idx,
                    initial_play_start: attrs.layer_start(),
                    drag_start_x: drag_start_pos.x,
                }
            }
            LayerTool::AdjustPlayEnd => {
                GlobalDragState::AdjustPlayEnd {
                    layer_idx,
                    initial_play_end: attrs.layer_end(),
                    drag_start_x: drag_start_pos.x,
                }
            }
            LayerTool::Move => {
                let initial_start = attrs.get_i32_or_zero(A_IN);
                GlobalDragState::MovingLayer {
                    layer_idx,
                    initial_start,
                    drag_start_x: drag_start_pos.x,
                    drag_start_y: drag_start_pos.y,
                }
            }
            LayerTool::Slide => {
                let initial_in = attrs.get_i32_or_zero(A_IN);
                let initial_trim_in = attrs.get_i32_or_zero(A_TRIM_IN);
                let initial_trim_out = attrs.get_i32_or_zero(A_TRIM_OUT);
                let speed = attrs.get_float_or(A_SPEED, 1.0);
                log::trace!(
                    "[SLIDE START] in={}, trim_in={}, trim_out={}, speed={}",
                    initial_in, initial_trim_in, initial_trim_out, speed
                );
                GlobalDragState::SlidingLayer {
                    layer_idx,
                    initial_in,
                    initial_trim_in,
                    initial_trim_out,
                    speed,
                    drag_start_x: drag_start_pos.x,
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

/// Detect layer tool with full geometry - supports Slide in trim zones
pub(super) fn detect_layer_tool_with_geom(
    hover_pos: Pos2,
    full_bar_rect: Rect,
    visible_bar_rect: Option<Rect>,
    edge_threshold: f32,
) -> Option<LayerTool> {
    // Must be within full bar expanded by threshold
    if !full_bar_rect.expand(edge_threshold).contains(hover_pos) {
        return None;
    }

    // If no visible bar (fully trimmed), treat as Move on full bar
    let Some(visible_rect) = visible_bar_rect else {
        if full_bar_rect.contains(hover_pos) {
            return Some(LayerTool::Move);
        }
        return None;
    };

    // Check if in trim zones (between full_bar and visible_bar)
    let in_left_trim_zone = hover_pos.x >= full_bar_rect.min.x
        && hover_pos.x < visible_rect.min.x - edge_threshold
        && hover_pos.y >= full_bar_rect.min.y
        && hover_pos.y <= full_bar_rect.max.y;

    let in_right_trim_zone = hover_pos.x > visible_rect.max.x + edge_threshold
        && hover_pos.x <= full_bar_rect.max.x
        && hover_pos.y >= full_bar_rect.min.y
        && hover_pos.y <= full_bar_rect.max.y;

    if in_left_trim_zone || in_right_trim_zone {
        return Some(LayerTool::Slide);
    }

    // Use standard detection for visible bar (handles + move)
    detect_layer_tool(hover_pos, visible_rect, edge_threshold)
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
        
        // Linear formula: keep labels ~60px apart, tick marks ~15px apart
        let target_label_px = 60.0;
        let target_tick_px = 15.0;
        
        // Raw step values
        let raw_label_step = (target_label_px / effective_ppf).ceil() as usize;
        let raw_tick_step = (target_tick_px / effective_ppf).ceil() as usize;
        
        // Round to nice numbers (1, 2, 5, 10, 20, 50, 100, ...)
        fn round_to_nice(n: usize) -> usize {
            if n <= 1 { return 1; }
            let pow = 10_usize.pow((n as f32).log10().floor() as u32);
            let norm = n / pow;
            let nice = if norm < 2 { 1 } else if norm < 5 { 2 } else { 5 };
            nice * pow
        }
        
        let label_step = round_to_nice(raw_label_step).max(1) as i32;
        let frame_step = round_to_nice(raw_tick_step).max(1).min(label_step as usize) as i32;

        // Use rect.width() for visible range - allow negative frames
        let visible_start = state.pan_offset.floor() as i32;
        let visible_end = (state.pan_offset + (rect.width() / effective_ppf)).ceil() as i32;
        
        // Align start_frame to frame_step grid (works with negatives)
        let start_frame = if visible_start >= 0 {
            (visible_start / frame_step) * frame_step
        } else {
            ((visible_start - frame_step + 1) / frame_step) * frame_step
        };

        let mut frame = start_frame;
        while frame <= visible_end {
            let x = frame_to_screen_x(frame as f32, rect.min.x, config, state);
            if x >= rect.min.x && x <= rect.max.x {
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
            frame += frame_step;
        }

        let is_middle_down = ui
            .ctx()
            .input(|i| i.pointer.button_down(egui::PointerButton::Middle));

        if !is_middle_down && (ruler_response.clicked() || ruler_response.dragged())
            && let Some(pos) = ruler_response.interact_pointer_pos() {
                let frame = screen_x_to_frame(pos.x, rect.min.x, config, state).round() as i32;
                frame_clicked = Some(frame.min(total_frames.saturating_sub(1)));
            }
    }

    (frame_clicked, rect)
}

/// Convert row index to Y coordinate in timeline
pub(super) fn row_to_y(row: usize, config: &TimelineConfig, timeline_rect: Rect) -> f32 {
    timeline_rect.min.y + (row as f32 * config.layer_height)
}

/// Draw drop preview (ghost) using the standard layer move style.
/// If `is_cycle` is true, draws red to indicate invalid drop.
pub(super) fn draw_drop_preview(
    painter: &egui::Painter,
    frame: i32,
    row_y: f32,
    duration: i32,
    timeline_rect: Rect,
    config: &TimelineConfig,
    state: &TimelineState,
    is_cycle: bool,
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
    let color = if is_cycle {
        Color32::from_rgba_unmultiplied(255, 80, 80, 200) // Red for cycle
    } else {
        Color32::from_rgba_unmultiplied(100, 220, 255, 180) // Blue normal
    };
    painter.rect_stroke(
        thumb_rect,
        4.0,
        egui::Stroke::new(2.0, color),
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
