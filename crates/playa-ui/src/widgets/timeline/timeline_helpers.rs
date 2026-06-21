//! Timeline UI helpers: time/space math and drawing utilities.
//!
//! The tool-detection, ruler and drag-ghost helpers were retired when the
//! timeline canvas migrated to the `egui-track-timeline` widget (it owns those
//! gestures + ruler internally). What remains is shared by the host overlays
//! playa still paints itself: the project-drop ghost and the frame-cache status
//! strip (via [`frame_to_screen_x`]), plus the stable per-clip bar colour.
use eframe::egui::{Color32, Pos2, Rect};

use super::{TimelineConfig, TimelineState};

/// Map a timeline frame to a screen x using playa's canonical pan/zoom.
/// Kept in sync with `egui_track_timeline::TimelineView::frame_to_x` (which the
/// widget uses for the same mapping); `state.zoom`/`pan_offset` are mirrored into
/// the widget view each frame, so overlays drawn with this align with the bars.
pub(super) fn frame_to_screen_x(
    frame: f32,
    timeline_rect_min_x: f32,
    config: &TimelineConfig,
    state: &TimelineState,
) -> f32 {
    timeline_rect_min_x + (frame - state.pan_offset) * config.pixels_per_frame * state.zoom
}

/// Screen-space rect for the project→timeline drop ghost (a thumbnail-style bar
/// at `frame` on the row starting at `row_y`).
pub(super) fn drop_preview_thumb_rect(
    frame: i32,
    row_y: f32,
    duration: i32,
    timeline_rect: Rect,
    config: &TimelineConfig,
    state: &TimelineState,
) -> Rect {
    let start_x = frame_to_screen_x(frame as f32, timeline_rect.min.x, config, state);
    let end_x = frame_to_screen_x(
        (frame + duration) as f32,
        timeline_rect.min.x,
        config,
        state,
    );
    let bar_height = (config.layer_height - 8.0).max(2.0);
    Rect::from_min_max(
        Pos2::new(start_x, row_y + 4.0),
        Pos2::new(end_x, row_y + 4.0 + bar_height),
    )
}

/// Stable bar colour derived from a hash of the clip name (matches the colour the
/// old hand-painted bars used; passed to the widget via `Clip::with_color`).
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
