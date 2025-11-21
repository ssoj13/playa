//! Timeline widget - state and configuration

use eframe::egui::Pos2;
use serde::{Deserialize, Serialize};

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
    pub show_frame_numbers: bool,
    pub last_comp_uuid: Option<String>, // Track last active comp to recenter on change
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
            show_frame_numbers: true,
            last_comp_uuid: None,
        }
    }
}

/// Global drag state - tracks what is currently being dragged
#[derive(Clone, Debug)]
pub enum GlobalDragState {
    /// Dragging clip/comp from Project Window to timeline
    ProjectItem {
        source_uuid: String,
        display_name: String,
        duration: Option<i32>,
        drag_start_pos: Pos2,
    },
    /// Scrubbing timeline by dragging on ruler or timeline area
    TimelineScrub { drag_start_pos: Pos2 },
    /// Panning timeline horizontally (middle mouse button)
    TimelinePan {
        drag_start_pos: Pos2,
        initial_pan_offset: f32,
    },
    /// Moving layer horizontally and/or vertically
    MovingLayer {
        layer_idx: usize,
        initial_start: i32, // Now supports negative values
        initial_end: i32,   // Now supports negative values
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

/// Timeline interaction result
#[derive(Debug, Clone)]
pub enum TimelineAction {
    None,
    SetFrame(i32),      // User clicked/dragged on timeline
    SelectLayer(usize), // User clicked on layer name
    ClearSelection,     // User clicked on empty space
    ToStart,            // Jump to start
    ToEnd,              // Jump to end
    TogglePlay,         // Toggle play/pause
    Stop,               // Stop playback
    JumpToPrevEdge,     // Jump to previous layer edge ([)
    JumpToNextEdge,     // Jump to next layer edge (])
    AddLayer {
        source_uuid: String,
        start_frame: i32,
    }, // Drop item on timeline
    ReorderLayer {
        from_idx: usize,
        to_idx: usize,
    }, // Reorder layer vertically (DnD in left column)
    MoveAndReorderLayer {
        layer_idx: usize,
        new_start: i32,
        new_idx: usize,
    }, // Move layer both horizontally and vertically
    SetLayerPlayStart {
        layer_idx: usize,
        new_play_start: i32,
    }, // Adjust layer play start
    SetLayerPlayEnd {
        layer_idx: usize,
        new_play_end: i32,
    }, // Adjust layer play end
    SetCompPlayStart {
        frame: i32,
    }, // Set comp work area start (B key)
    SetCompPlayEnd {
        frame: i32,
    }, // Set comp work area end (N key)
    ResetCompPlayArea,  // Reset comp work area to full (Ctrl+B)
}
