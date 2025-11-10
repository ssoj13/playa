use eframe::egui::{self, Color32, Pos2, Rect, Response, Sense, Ui, Vec2};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::cache::Cache;
use crate::frame::FrameStatus;

// Load indicator colors
const COLOR_PLACEHOLDER: Color32 = Color32::from_rgb(40, 40, 45); // Тёмно-серый
const COLOR_HEADER: Color32 = Color32::from_rgb(60, 100, 180); // Синий
const COLOR_LOADING: Color32 = Color32::from_rgb(220, 160, 60); // Оранжевый
const COLOR_LOADED: Color32 = Color32::from_rgb(80, 200, 120); // Зелёный
const COLOR_ERROR: Color32 = Color32::from_rgb(200, 60, 60); // Красный

/// Cache for load indicator state
#[derive(Clone, Debug)]
struct LoadIndicatorCache {
    statuses: Vec<FrameStatus>,
    cached_count: usize,      // Number of cached frames (for detecting changes)
    loaded_events: usize,     // Number of successful frame loads (monotonic)
    sequences_version: usize, // Sequences version (changes when playlist changes)
}

/// Represents a sequence range in global frame space
#[derive(Clone, Debug)]
pub struct SequenceRange {
    pub start_frame: usize,
    pub end_frame: usize,
    pub pattern: String,
}

/// Configuration for the time slider widget
#[derive(Clone, Debug)]
pub struct TimeSliderConfig {
    pub height: f32,
    pub show_labels: bool,
    pub show_dividers: bool,
    pub label_min_width: f32,
    pub show_load_indicator: bool,
    pub load_indicator_height: f32,
}

impl Default for TimeSliderConfig {
    fn default() -> Self {
        Self {
            height: 24.0,
            show_labels: true,
            show_dividers: true,
            label_min_width: 60.0,
            show_load_indicator: true,
            load_indicator_height: 4.0,
        }
    }
}

/// Main time slider widget with colored sequence zones
/// Returns Some(new_frame) if user interacted with the slider
pub fn time_slider(
    ui: &mut Ui,
    current_frame: usize,
    total_frames: usize,
    sequences: &[SequenceRange],
    config: &TimeSliderConfig,
    cache: &Cache,
) -> Option<usize> {
    if total_frames == 0 {
        return None;
    }

    // Get/update cached statuses using egui persistence
    let cache_id = ui.id().with("load_indicator_cache");
    let current_cached_count = cache.cached_frames_count();
    let current_loaded_events = cache.loaded_events_counter();
    let current_seq_ver = cache.sequences_version();

    let cached_statuses = ui.ctx().memory_mut(|mem| {
        let stored: Option<LoadIndicatorCache> = mem.data.get_temp(cache_id);

        match stored {
            Some(cached)
                if cached.cached_count == current_cached_count
                    && cached.loaded_events == current_loaded_events
                    && cached.sequences_version == current_seq_ver =>
            {
                // Cache is up-to-date
                cached.statuses
            }
            _ => {
                // Rebuild cache when any token changes
                let statuses = cache.get_frame_stats();
                mem.data.insert_temp(
                    cache_id,
                    LoadIndicatorCache {
                        statuses: statuses.clone(),
                        cached_count: current_cached_count,
                        loaded_events: current_loaded_events,
                        sequences_version: current_seq_ver,
                    },
                );
                statuses
            }
        }
    });

    // Allocate space for the widget (include load indicator height if enabled)
    let total_height = if config.show_load_indicator {
        config.height + config.load_indicator_height
    } else {
        config.height
    };
    let desired_size = Vec2::new(ui.available_width(), total_height);
    let (rect, response) = ui.allocate_exact_size(desired_size, Sense::click_and_drag());

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();

        // Draw sequence backgrounds
        draw_seq_backgrounds(painter, rect, sequences, total_frames);

        // Draw play range (work area)
        draw_play_range(painter, rect, cache.get_play_range(), total_frames);

        // Draw dividers between sequences
        if config.show_dividers {
            draw_seq_dividers(painter, rect, sequences, total_frames);
        }

        // Draw sequence labels
        if config.show_labels {
            draw_seq_labels(
                painter,
                rect,
                sequences,
                total_frames,
                config.label_min_width,
            );
        }

        // Draw playhead (current frame indicator)
        draw_playhead(painter, rect, current_frame, total_frames);

        // Draw load indicator
        if config.show_load_indicator {
            draw_load_indicator(
                painter,
                rect,
                &cached_statuses,
                config.load_indicator_height,
            );
        }
    }

    // Handle interaction (only on slider rect, not including load indicator)
    let slider_rect =
        Rect::from_min_max(rect.min, Pos2::new(rect.max.x, rect.min.y + config.height));
    handle_interaction(&response, slider_rect, total_frames)
}

/// Draw colored backgrounds for each sequence
fn draw_seq_backgrounds(
    painter: &egui::Painter,
    rect: Rect,
    sequences: &[SequenceRange],
    total_frames: usize,
) {
    let frame_to_x =
        |frame: usize| -> f32 { rect.min.x + (frame as f32 / total_frames as f32) * rect.width() };

    for seq in sequences {
        let x_start = frame_to_x(seq.start_frame);
        let x_end = frame_to_x(seq.end_frame + 1); // +1 to include end frame

        let seq_rect =
            Rect::from_min_max(Pos2::new(x_start, rect.min.y), Pos2::new(x_end, rect.max.y));

        let color = hash_color(&seq.pattern);
        painter.rect_filled(seq_rect, 0.0, color);
    }
}

/// Draw play range (work area) indicator - grey bar in middle 50% height
fn draw_play_range(
    painter: &egui::Painter,
    rect: Rect,
    play_range: (usize, usize),
    total_frames: usize,
) {
    if total_frames == 0 {
        return;
    }

    let (start, end) = play_range;

    let frame_to_x =
        |frame: usize| -> f32 { rect.min.x + (frame as f32 / total_frames as f32) * rect.width() };

    let x_start = frame_to_x(start);
    let x_end = frame_to_x(end + 1); // +1 to include end frame

    // Position bar in middle 50% of height
    let bar_height = rect.height() * 0.5;
    let bar_y_offset = rect.height() * 0.25; // Center vertically

    let play_rect = Rect::from_min_max(
        Pos2::new(x_start, rect.min.y + bar_y_offset),
        Pos2::new(x_end, rect.min.y + bar_y_offset + bar_height),
    );

    // Semi-transparent grey overlay
    painter.rect_filled(
        play_rect,
        0.0,
        Color32::from_rgba_premultiplied(120, 120, 120, 80),
    );
}

/// Draw vertical divider lines between sequences
fn draw_seq_dividers(
    painter: &egui::Painter,
    rect: Rect,
    sequences: &[SequenceRange],
    total_frames: usize,
) {
    let frame_to_x =
        |frame: usize| -> f32 { rect.min.x + (frame as f32 / total_frames as f32) * rect.width() };

    // Draw dividers at sequence boundaries (except first)
    for seq in sequences.iter().skip(1) {
        let x = frame_to_x(seq.start_frame);
        painter.line_segment(
            [Pos2::new(x, rect.min.y), Pos2::new(x, rect.max.y)],
            (1.5, Color32::from_gray(200)),
        );
    }
}

/// Draw sequence labels (numbers or filenames)
fn draw_seq_labels(
    painter: &egui::Painter,
    rect: Rect,
    sequences: &[SequenceRange],
    total_frames: usize,
    min_width: f32,
) {
    let frame_to_x =
        |frame: usize| -> f32 { rect.min.x + (frame as f32 / total_frames as f32) * rect.width() };

    for (idx, seq) in sequences.iter().enumerate() {
        let x_start = frame_to_x(seq.start_frame);
        let x_end = frame_to_x(seq.end_frame + 1);
        let zone_width = x_end - x_start;

        // Determine what to show based on available width
        let label = if zone_width > min_width {
            // Show filename if enough space
            extract_filename(&seq.pattern)
        } else if zone_width > 20.0 {
            // Show just the sequence number
            format!("{}", idx)
        } else {
            // Too narrow, skip label
            continue;
        };

        let center_x = (x_start + x_end) / 2.0;
        let center_y = (rect.min.y + rect.max.y) / 2.0;

        painter.text(
            Pos2::new(center_x, center_y),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(11.0),
            Color32::from_gray(240),
        );
    }
}

/// Draw playhead indicator at current frame
fn draw_playhead(painter: &egui::Painter, rect: Rect, current_frame: usize, total_frames: usize) {
    let x = rect.min.x + (current_frame as f32 / total_frames as f32) * rect.width();

    // Draw vertical line
    painter.line_segment(
        [Pos2::new(x, rect.min.y), Pos2::new(x, rect.max.y)],
        (2.0, Color32::from_rgb(255, 220, 100)),
    );

    // Draw frame number next to the line
    let frame_text = format!("{}", current_frame);
    let text_pos = Pos2::new(x + 4.0, rect.min.y + 2.0);

    // Draw background for better readability
    let galley = painter.layout_no_wrap(
        frame_text.clone(),
        egui::FontId::proportional(11.0),
        Color32::WHITE,
    );
    let text_rect = egui::Rect::from_min_size(text_pos, galley.size());
    painter.rect_filled(text_rect.expand(2.0), 2.0, Color32::from_black_alpha(180));

    // Draw frame number text
    painter.text(
        text_pos,
        egui::Align2::LEFT_TOP,
        frame_text,
        egui::FontId::proportional(11.0),
        Color32::from_rgba_unmultiplied(255, 255, 255, 128),
    );
}

/// Handle mouse interaction (click and drag)
fn handle_interaction(response: &Response, rect: Rect, total_frames: usize) -> Option<usize> {
    if response.dragged() || response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            let ratio = ((pos.x - rect.min.x) / rect.width()).clamp(0.0, 1.0);
            let new_frame = (ratio * total_frames as f32) as usize;
            return Some(new_frame.min(total_frames.saturating_sub(1)));
        }
    }
    None
}

/// Generate a stable color from a pattern string using hash
fn hash_color(pattern: &str) -> Color32 {
    let mut hasher = DefaultHasher::new();
    pattern.hash(&mut hasher);
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

/// Extract filename from pattern (e.g., "c:/temp/seq.*.exr" -> "seq")
fn extract_filename(pattern: &str) -> String {
    // Get the last path component
    let normalized = pattern.replace('\\', "/");
    let filename = normalized.split('/').last().unwrap_or(pattern);

    // Remove the .* or #### pattern and extension
    filename.split('.').next().unwrap_or(filename).to_string()
}

/// Draw load indicator showing frame load status
fn draw_load_indicator(painter: &egui::Painter, rect: Rect, statuses: &[FrameStatus], height: f32) {
    let total = statuses.len();
    if total == 0 {
        return;
    }

    let indicator_rect = Rect::from_min_max(
        Pos2::new(rect.min.x, rect.max.y),
        Pos2::new(rect.max.x, rect.max.y + height),
    );

    let block_width = indicator_rect.width() / total as f32;

    for (idx, status) in statuses.iter().enumerate() {
        let x_start = indicator_rect.min.x + (idx as f32 * block_width);
        let x_end = x_start + block_width;

        let color = match status {
            FrameStatus::Placeholder => COLOR_PLACEHOLDER,
            FrameStatus::Header => COLOR_HEADER,
            FrameStatus::Loading => COLOR_LOADING,
            FrameStatus::Loaded => COLOR_LOADED,
            FrameStatus::Error => COLOR_ERROR,
        };

        let block_rect = Rect::from_min_max(
            Pos2::new(x_start, indicator_rect.min.y),
            Pos2::new(x_end, indicator_rect.max.y),
        );

        painter.rect_filled(block_rect, 0.0, color);
    }
}
