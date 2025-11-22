//! Timeline widget - state and configuration.
//! Shared by outline/canvas renderers and the UI tab. Data flow: UI mutations
//! update `TimelineState` (zoom/pan/selection) and emit `AppEvent`s which
//! are bridged to the EventBus; renderers read `TimelineConfig`/`TimelineState`
//! to draw rows/bars and handle interactions.

use eframe::egui::Pos2;
use serde::{Deserialize, Serialize};

/// Timeline actions result - returned from render functions
#[derive(Default)]
pub struct TimelineActions {
    pub hovered: bool,
}

/// Configuration for timeline widget
#[derive(Clone, Debug)]
pub struct TimelineConfig {
    pub layer_height: f32,
    pub name_column_width: f32,
    pub pixels_per_frame: f32, // Zoom level
}

impl Default for TimelineConfig {
    fn default() -> Self {
        Self {
            layer_height: 32.0,
            name_column_width: 150.0,
            pixels_per_frame: 2.0, // 2 pixels per frame by default
        }
    }
}

/// Timeline state (persistent between frames)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimelineState {
    pub zoom: f32,                     // Zoom multiplier (1.0 = default, range 0.1..4.0)
    pub pan_offset: f32,               // Horizontal scroll offset in frames
    pub selected_layer: Option<usize>, // Currently selected layer index
    #[serde(skip)]
    pub drag_state: Option<GlobalDragState>, // Active drag operation (centralized for all drag types)
    pub snap_enabled: bool,
    pub lock_work_area: bool,
    pub last_comp_uuid: Option<String>, // Track last active comp to recenter on change
    pub view_mode: TimelineViewMode,
    #[serde(skip)]
    pub last_canvas_width: f32, // Last known canvas width for Fit calculation
    pub outline_width: f32, // Width of outline panel in Split mode (persistent)
}

impl Default for TimelineState {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan_offset: 0.0,
            selected_layer: None,
            drag_state: None,
            snap_enabled: true,
            lock_work_area: false,
            last_comp_uuid: None,
            view_mode: TimelineViewMode::Split,
            last_canvas_width: 800.0, // Default estimate
            outline_width: 400.0, // Default outline panel width
        }
    }
}

/// Global drag state - tracks what is currently being dragged
#[derive(Clone, Debug)]
pub enum GlobalDragState {
    /// Dragging clip/comp from Project Window to timeline
    ProjectItem {
        source_uuid: String,
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
        initial_start: i32, // Now supports negative values
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
}

#[derive(Clone, Debug, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimelineViewMode {
    Split,
    CanvasOnly,
    OutlineOnly,
}

/// Precomputed layer geometry - shared between draw and interaction passes.
pub(super) struct LayerGeom {
    pub visible_start: i32,
    pub visible_end: i32,
    pub full_bar_rect: eframe::egui::Rect,
    pub visible_bar_rect: Option<eframe::egui::Rect>,
}

impl LayerGeom {
    /// Calculate layer geometry. play_start/play_end are ABSOLUTE source frames.
    pub fn calc(
        child_start: i32,
        child_end: i32,
        play_start: i32,
        play_end: i32,
        child_y: f32,
        timeline_rect: eframe::egui::Rect,
        config: &TimelineConfig,
        state: &TimelineState,
    ) -> Self {
        use eframe::egui::{Pos2, Rect};
        
        let visible_start = child_start + play_start;
        let visible_end = child_start + play_end;

        let frame_to_screen_x = |frame: f32, timeline_min_x: f32| -> f32 {
            let frame_offset = frame - state.pan_offset;
            timeline_min_x + (frame_offset * config.pixels_per_frame * state.zoom)
        };

        let full_bar_x_start = frame_to_screen_x(child_start as f32, timeline_rect.min.x);
        let full_bar_x_end = frame_to_screen_x((child_end + 1) as f32, timeline_rect.min.x);
        let full_bar_rect = Rect::from_min_max(
            Pos2::new(full_bar_x_start, child_y + 4.0),
            Pos2::new(full_bar_x_end, child_y + config.layer_height - 4.0),
        );

        let visible_bar_rect = if visible_start <= visible_end {
            let visible_bar_x_start = frame_to_screen_x(visible_start as f32, timeline_rect.min.x);
            let visible_bar_x_end = frame_to_screen_x((visible_end + 1) as f32, timeline_rect.min.x);
            Some(Rect::from_min_max(
                Pos2::new(visible_bar_x_start, child_y + 4.0),
                Pos2::new(visible_bar_x_end, child_y + config.layer_height - 4.0),
            ))
        } else {
            None
        };

        Self { visible_start, visible_end, full_bar_rect, visible_bar_rect }
    }
}
