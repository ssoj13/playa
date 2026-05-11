//! Egui ↔ glam adapter layer for viewport coord conversions.
//!
//! All actual math lives in the `playa-coord` crate (re-exported via
//! `playa_engine::entities::space`). These wrappers exist only to bridge
//! `egui::Vec2` (used by pointer events and viewport sizing) and
//! `glam::Vec2` (used by the canonical helpers). Single-formula
//! delegations — no inline math.

use eframe::egui;
use playa_engine::entities::space;

/// Flip Y for a 2D vector. Egui-vec2 wrapper around `space::flip_y`.
pub fn flip_y_vec2(v: egui::Vec2) -> egui::Vec2 {
    let g = space::flip_y(glam::Vec2::new(v.x, v.y));
    egui::vec2(g.x, g.y)
}

/// Convert egui screen-space pos to centered viewport space (+Y up).
/// Egui-vec2 wrapper around `space::screen_to_viewport`.
pub fn screen_to_viewport_centered(
    screen_pos: egui::Vec2,
    viewport_size: egui::Vec2,
) -> egui::Vec2 {
    let g = space::screen_to_viewport(
        glam::Vec2::new(screen_pos.x, screen_pos.y),
        glam::Vec2::new(viewport_size.x, viewport_size.y),
    );
    egui::vec2(g.x, g.y)
}

/// Convert egui screen-space delta to viewport delta (+Y up).
/// Same as flipping Y (no center-offset for deltas).
pub fn screen_delta_to_viewport(delta: egui::Vec2) -> egui::Vec2 {
    flip_y_vec2(delta)
}
