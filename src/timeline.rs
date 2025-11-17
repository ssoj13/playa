//! After Effects-style timeline: vertical stack of layers with horizontal bars
//!
//! Each layer is displayed as a row showing:
//! - Layer name / clip name
//! - Start..End range as horizontal bar
//! - Visual indication of current_frame (playhead)

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Ui, Vec2};
use crate::comp::Comp;

/// Configuration for timeline widget
#[derive(Clone, Debug)]
pub struct TimelineConfig {
    pub layer_height: f32,
    pub name_column_width: f32,
    pub show_frame_numbers: bool,
    pub pixels_per_frame: f32, // Zoom level
}

impl Default for TimelineConfig {
    fn default() -> Self {
        Self {
            layer_height: 32.0,
            name_column_width: 150.0,
            show_frame_numbers: true,
            pixels_per_frame: 2.0, // 2 pixels per frame by default
        }
    }
}

/// Timeline interaction result
pub enum TimelineAction {
    None,
    SetFrame(usize), // User clicked/dragged on timeline
    SelectLayer(usize), // User clicked on layer name
}

/// Render After Effects-style timeline
pub fn render_timeline(
    ui: &mut Ui,
    comp: &Comp,
    config: &TimelineConfig,
) -> TimelineAction {
    let mut action = TimelineAction::None;

    // Calculate dimensions
    let total_frames = comp.total_frames();
    if total_frames == 0 || comp.layers.is_empty() {
        ui.label("No layers in composition");
        return action;
    }

    let timeline_width = (total_frames as f32 * config.pixels_per_frame).max(ui.available_width() - config.name_column_width);
    let total_height = comp.layers.len() as f32 * config.layer_height;

    // Header: frame numbers ruler
    if config.show_frame_numbers {
        draw_frame_ruler(ui, comp, config, timeline_width);
    }

    // Scrollable area for layers
    // Use id_salt for persistence and let egui manage sizing naturally
    egui::ScrollArea::both()
        .id_salt("timeline_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Two-column layout: layer names | timeline bars
            ui.horizontal(|ui| {
                // Left column: layer names
                ui.vertical(|ui| {
                    ui.set_width(config.name_column_width);
                    for (idx, layer) in comp.layers.iter().enumerate() {
                        let layer_name = &layer.source_uuid;
                        let (rect, response) = ui.allocate_exact_size(
                            Vec2::new(config.name_column_width, config.layer_height),
                            Sense::click(),
                        );

                        // Draw layer name background
                        ui.painter().rect_filled(
                            rect,
                            2.0,
                            Color32::from_gray(40),
                        );

                        // Draw layer name text
                        ui.painter().text(
                            Pos2::new(rect.min.x + 8.0, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            layer_name,
                            egui::FontId::proportional(12.0),
                            Color32::from_gray(200),
                        );

                        if response.clicked() {
                            action = TimelineAction::SelectLayer(idx);
                        }
                    }
                });

                // Right column: timeline bars
                ui.vertical(|ui| {
                    ui.set_width(timeline_width);
                    ui.set_height(total_height);

                    let (timeline_rect, timeline_response) = ui.allocate_exact_size(
                        Vec2::new(timeline_width, total_height),
                        Sense::click_and_drag(),
                    );

                    if ui.is_rect_visible(timeline_rect) {
                        let painter = ui.painter();

                        // Draw layer bars
                        for (idx, layer) in comp.layers.iter().enumerate() {
                            let layer_y = timeline_rect.min.y + (idx as f32 * config.layer_height);
                            let layer_rect = Rect::from_min_size(
                                Pos2::new(timeline_rect.min.x, layer_y),
                                Vec2::new(timeline_width, config.layer_height),
                            );

                            // Layer background (alternating colors)
                            let bg_color = if idx % 2 == 0 {
                                Color32::from_gray(30)
                            } else {
                                Color32::from_gray(35)
                            };
                            painter.rect_filled(layer_rect, 0.0, bg_color);

                            // Get layer start/end from attrs
                            let layer_start = layer.attrs.get_u32("start").unwrap_or(0) as usize;
                            let layer_end = layer.attrs.get_u32("end").unwrap_or(0) as usize;

                            // Draw layer bar (clip range)
                            let bar_x_start = timeline_rect.min.x + (layer_start as f32 * config.pixels_per_frame);
                            let bar_x_end = timeline_rect.min.x + ((layer_end + 1) as f32 * config.pixels_per_frame);
                            let bar_rect = Rect::from_min_max(
                                Pos2::new(bar_x_start, layer_y + 4.0),
                                Pos2::new(bar_x_end, layer_y + config.layer_height - 4.0),
                            );

                            // Layer bar color (use hash of source_uuid for stable color)
                            let bar_color = hash_color(&layer.source_uuid);

                            painter.rect_filled(bar_rect, 4.0, bar_color);
                            painter.rect_stroke(
                                bar_rect,
                                4.0,
                                egui::Stroke::new(1.0, Color32::from_gray(150)),
                                egui::epaint::StrokeKind::Middle,
                            );
                        }

                        // Draw playhead (current_frame) as vertical line
                        draw_playhead(painter, timeline_rect, comp.current_frame, config);

                        // Handle click/drag interaction
                        if timeline_response.clicked() || timeline_response.dragged() {
                            if let Some(pos) = timeline_response.interact_pointer_pos() {
                                let frame = ((pos.x - timeline_rect.min.x) / config.pixels_per_frame) as usize;
                                action = TimelineAction::SetFrame(frame.min(total_frames.saturating_sub(1)));
                            }
                        }
                    }
                });
            });
        });

    action
}

/// Draw frame number ruler at top of timeline
fn draw_frame_ruler(ui: &mut Ui, comp: &Comp, config: &TimelineConfig, timeline_width: f32) {
    let total_frames = comp.total_frames();
    let ruler_height = 20.0;

    ui.horizontal(|ui| {
        // Empty space for name column
        ui.allocate_exact_size(Vec2::new(config.name_column_width, ruler_height), Sense::hover());

        // Ruler area
        let (rect, _) = ui.allocate_exact_size(
            Vec2::new(timeline_width, ruler_height),
            Sense::hover(),
        );

        if ui.is_rect_visible(rect) {
            let painter = ui.painter();

            // Ruler background
            painter.rect_filled(rect, 0.0, Color32::from_gray(25));

            // Draw frame markers every N frames (adaptive based on zoom)
            let frame_step = if config.pixels_per_frame > 10.0 {
                1
            } else if config.pixels_per_frame > 2.0 {
                5
            } else if config.pixels_per_frame > 0.5 {
                10
            } else {
                50
            };

            for frame in (0..=total_frames).step_by(frame_step) {
                let x = rect.min.x + (frame as f32 * config.pixels_per_frame);
                if x > rect.max.x {
                    break;
                }

                // Draw tick mark
                painter.line_segment(
                    [Pos2::new(x, rect.max.y - 5.0), Pos2::new(x, rect.max.y)],
                    (1.0, Color32::from_gray(100)),
                );

                // Draw frame number
                if frame % (frame_step * 2) == 0 {
                    painter.text(
                        Pos2::new(x, rect.min.y + 2.0),
                        egui::Align2::CENTER_TOP,
                        format!("{}", frame),
                        egui::FontId::monospace(9.0),
                        Color32::from_gray(150),
                    );
                }
            }
        }
    });
}

/// Draw playhead indicator at current frame
fn draw_playhead(
    painter: &egui::Painter,
    timeline_rect: Rect,
    current_frame: usize,
    config: &TimelineConfig,
) {
    let x = timeline_rect.min.x + (current_frame as f32 * config.pixels_per_frame);

    // Vertical line through all layers
    painter.line_segment(
        [Pos2::new(x, timeline_rect.min.y), Pos2::new(x, timeline_rect.max.y)],
        (2.0, Color32::from_rgb(255, 220, 100)),
    );

    // Triangle indicator at top
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

/// Generate stable color from string using hash
fn hash_color(s: &str) -> Color32 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    let hash = hasher.finish();

    // Use hash to generate hue (0-360)
    let hue = (hash % 360) as f32;

    // Fixed saturation and value for consistent look
    let saturation = 0.65;
    let value = 0.55;

    hsv_to_rgb(hue, saturation, value)
}

/// Convert HSV to RGB (for color generation)
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> Color32 {
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
