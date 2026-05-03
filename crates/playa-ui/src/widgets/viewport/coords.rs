//! Coordinate conversion helpers for viewport interactions.
//!
//! Conventions:
//! - egui screen space: +Y is down.
//! - Viewport space: +Y is up, origin at viewport center.
//!
//! Use these helpers to keep Y inversion consistent across viewport input.

use eframe::egui;

/// Flip Y for a 2D vector.
pub fn flip_y_vec2(v: egui::Vec2) -> egui::Vec2 {
    egui::vec2(v.x, -v.y)
}

/// Convert screen-space pos to centered viewport space (+Y up).
pub fn screen_to_viewport_centered(screen_pos: egui::Vec2, viewport_size: egui::Vec2) -> egui::Vec2 {
    let local = screen_pos - viewport_size * 0.5;
    flip_y_vec2(local)
}

/// Convert screen-space delta to viewport delta (+Y up).
pub fn screen_delta_to_viewport(delta: egui::Vec2) -> egui::Vec2 {
    flip_y_vec2(delta)
}
