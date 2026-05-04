//! Drag-and-drop state shared across dock panels (e.g. Project → Timeline).
//!
//! Stored in [`eframe::egui`]'s ephemeral data ([`global_drag_state_id`]) and in
//! [`crate::widgets::timeline::TimelineState::drag_state`]. Lives here so **Project** does not depend
//! on the **timeline** module path and **timeline** does not pull **project** for types.
//!
//! After the dock renders, [`paint_global_project_drag_overlay`] paints a foreground ghost so
//! project→timeline drags stay visible outside the timeline panel. [`GlobalDragState::ProjectItem`]
//! is cleared on **primary** button release if the drop was not consumed — middle-button release
//! (e.g. timeline pan) must not cancel a project drag.

use eframe::egui::{
    self, Color32, Id, LayerId, Order, Pos2, Rect, Stroke,
};
use uuid::Uuid;

/// Ephemeral `insert_temp` / `get_temp` id for [`GlobalDragState`] (Project ↔ Timeline).
#[inline]
pub fn global_drag_state_id() -> egui::Id {
    egui::Id::new("global_drag_state")
}

/// Timeline inserts this while [`GlobalDragState::ProjectItem`] hovers over the timeline X-band;
/// the overlay reads it to draw an accurate snapped thumb above all panels.
#[inline]
pub fn project_drag_snap_overlay_id() -> egui::Id {
    egui::Id::new("project_drag_snap_overlay")
}

/// Snapped drop preview rectangle in screen coordinates (from timeline layout math).
#[derive(Clone, Copy, Debug)]
pub struct ProjectDragSnapOverlay {
    pub rect: Rect,
    pub is_cycle: bool,
}

/// Paints Project→Timeline ghost on a foreground layer; clears [`GlobalDragState::ProjectItem`]
/// on primary release when still present (drop outside timeline or unconsumed release).
///
/// Call once per frame after [`egui::CentralPanel`] / dock rendering so timeline can set [`ProjectDragSnapOverlay`] first.
pub fn paint_global_project_drag_overlay(ctx: &egui::Context) {
    let project_drag = ctx.data(|d| d.get_temp::<GlobalDragState>(global_drag_state_id()));

    let Some(GlobalDragState::ProjectItem { duration, .. }) = project_drag else {
        ctx.data_mut(|d| {
            d.remove::<ProjectDragSnapOverlay>(project_drag_snap_overlay_id());
        });
        return;
    };

    let released = ctx.input(|i| i.pointer.primary_released());

    let mut painter = ctx.layer_painter(LayerId::new(
        Order::Foreground,
        Id::new("playa_project_drag_ghost"),
    ));
    painter.set_clip_rect(ctx.viewport_rect());

    if let Some(snap) =
        ctx.data(|d| d.get_temp::<ProjectDragSnapOverlay>(project_drag_snap_overlay_id()))
    {
        let color = if snap.is_cycle {
            Color32::from_rgba_unmultiplied(255, 80, 80, 200)
        } else {
            Color32::from_rgba_unmultiplied(100, 220, 255, 180)
        };
        painter.rect_stroke(
            snap.rect,
            4.0,
            Stroke::new(2.0, color),
            egui::epaint::StrokeKind::Middle,
        );
    } else if let Some(hover) =
        ctx.input(|i| i.pointer.hover_pos().or_else(|| i.pointer.latest_pos()))
    {
        // Screen-space heuristic when cursor is outside the snapped timeline band.
        let dur = duration.unwrap_or(10).max(1);
        let w = (dur as f32 * 6.0).clamp(48.0, 800.0);
        let h = 28.0_f32;
        let rect = Rect::from_center_size(hover, egui::vec2(w, h));
        let color = Color32::from_rgba_unmultiplied(100, 220, 255, 180);
        painter.rect_stroke(
            rect,
            4.0,
            Stroke::new(2.0, color),
            egui::epaint::StrokeKind::Middle,
        );
    }

    if released {
        ctx.data_mut(|d| {
            if matches!(
                d.get_temp::<GlobalDragState>(global_drag_state_id()),
                Some(GlobalDragState::ProjectItem { .. })
            ) {
                d.remove::<GlobalDragState>(global_drag_state_id());
            }
            d.remove::<ProjectDragSnapOverlay>(project_drag_snap_overlay_id());
        });
    }
}

/// What is currently being dragged — project item toward the timeline or in-timeline gestures.
#[derive(Clone, Debug)]
pub enum GlobalDragState {
    /// Dragging clip/comp from Project Window to timeline
    ProjectItem {
        source_uuid: Uuid,
        duration: Option<i32>,
    },
    /// Panning timeline horizontally (middle mouse button)
    TimelinePan {
        drag_start_pos: Pos2,
        initial_pan_offset: f32,
    },
    /// Moving layer horizontally and/or vertically
    MovingLayer {
        layer_idx: usize,
        /// Supports negative timeline positions
        initial_start: i32,
        drag_start_x: f32,
        drag_start_y: f32,
    },
    /// Adjusting layer play start (left edge)
    AdjustPlayStart {
        layer_idx: usize,
        initial_play_start: i32,
        drag_start_x: f32,
    },
    /// Adjusting layer play end (right edge)
    AdjustPlayEnd {
        layer_idx: usize,
        initial_play_end: i32,
        drag_start_x: f32,
    },
    /// Sliding layer — moves `_in` while compensating trim_in/trim_out
    SlidingLayer {
        layer_idx: usize,
        initial_in: i32,
        initial_trim_in: i32,
        initial_trim_out: i32,
        speed: f32,
        drag_start_x: f32,
    },
}
