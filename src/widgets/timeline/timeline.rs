//! Timeline widget - state and configuration.
//! Shared by outline/canvas renderers and the UI tab. Data flow: UI mutations
//! update `TimelineState` (zoom/pan/selection) and emit `AppEvent`s which
//! are bridged to the EventBus; renderers read `TimelineConfig`/`TimelineState`
//! to draw rows/bars and handle interactions.

use crate::entities::Attrs;
use eframe::egui::{self, Pos2};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use std::collections::HashMap;

/// Clipboard entry for copied layers
/// Stores source UUID and a clone of the layer attributes
#[derive(Clone, Debug)]
pub struct ClipboardLayer {
    pub source_uuid: Uuid,
    pub attrs: Attrs,
    /// Original start frame (for calculating relative offsets)
    pub original_start: i32,
}

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
            name_column_width: 300.0,
            pixels_per_frame: 2.0, // 2 pixels per frame by default
        }
    }
}

impl TimelineConfig {
    /// Create config with custom layer height from settings
    pub fn with_layer_height(layer_height: f32) -> Self {
        Self {
            layer_height,
            ..Default::default()
        }
    }
}

/// Timeline state (persistent between frames)
#[derive(Clone, Serialize, Deserialize)]
pub struct TimelineState {
    pub zoom: f32,                     // Zoom multiplier (1.0 = default, range 0.1..4.0)
    pub pan_offset: f32,               // Horizontal scroll offset in frames
    #[serde(skip)]
    pub drag_state: Option<GlobalDragState>, // Active drag operation (centralized for all drag types)
    pub snap_enabled: bool,
    pub lock_work_area: bool,
    pub last_comp_uuid: Option<Uuid>, // Track last active comp to recenter on change
    pub view_mode: TimelineViewMode,
    #[serde(skip)]
    pub last_canvas_width: f32, // Last known canvas width for Fit calculation
    pub outline_width: f32, // Width of outline panel in Split mode (persistent)
    #[serde(skip)]
    pub hatch_texture: Option<egui::TextureHandle>, // Diagonal hatch pattern for file comps
    #[serde(skip)]
    pub clipboard: Vec<ClipboardLayer>, // Copied layers for Ctrl-C/Ctrl-V
    #[serde(skip)]
    pub geom_cache: HashMap<usize, LayerGeom>, // Cached layer geometry for interactions
}

impl std::fmt::Debug for TimelineState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TimelineState")
            .field("zoom", &self.zoom)
            .field("pan_offset", &self.pan_offset)
            .field("drag_state", &self.drag_state)
            .field("snap_enabled", &self.snap_enabled)
            .field("lock_work_area", &self.lock_work_area)
            .field("last_comp_uuid", &self.last_comp_uuid)
            .field("view_mode", &self.view_mode)
            .field("last_canvas_width", &self.last_canvas_width)
            .field("outline_width", &self.outline_width)
            .field("hatch_texture", &self.hatch_texture.as_ref().map(|_| "TextureHandle"))
            .field("clipboard", &format!("{} layers", self.clipboard.len()))
            .field("geom_cache", &format!("{} entries", self.geom_cache.len()))
            .finish()
    }
}

impl Default for TimelineState {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan_offset: 0.0,
            drag_state: None,
            snap_enabled: true,
            lock_work_area: false,
            last_comp_uuid: None,
            view_mode: TimelineViewMode::Split,
            last_canvas_width: 800.0, // Default estimate
            outline_width: 400.0,     // Default outline panel width
            hatch_texture: None,
            clipboard: Vec::new(),
            geom_cache: HashMap::new(),
        }
    }
}

impl TimelineState {
    /// Get or create the diagonal hatch pattern texture for file comp bars
    pub fn get_hatch_texture(&mut self, ctx: &egui::Context) -> egui::TextureId {
        if self.hatch_texture.is_none() {
            self.hatch_texture = Some(create_hatch_texture(ctx));
        }
        self.hatch_texture.as_ref().unwrap().id()
    }
}

/// Create diagonal hatch pattern texture (64x64 pixels, thick lines)
/// Returns grayscale multiplier: white = full color, dark = dimmed
fn create_hatch_texture(ctx: &egui::Context) -> egui::TextureHandle {
    const SIZE: usize = 64;
    const LINE_WIDTH: usize = 14;
    const SPACING: usize = 32; // Must divide SIZE evenly for seamless tiling

    let mut pixels = vec![egui::Color32::WHITE; SIZE * SIZE];

    // Draw diagonal stripes - dark bands on white background
    // The texture will be multiplied with base color
    for y in 0..SIZE {
        for x in 0..SIZE {
            // Diagonal pattern: (x + y) mod spacing
            let diag = (x + y) % SPACING;
            if diag < LINE_WIDTH {
                // Very subtle stripe (~95% brightness)
                pixels[y * SIZE + x] = egui::Color32::from_gray(243);
            }
            // else: white (full base color)
        }
    }

    let image = egui::ColorImage::from_rgba_unmultiplied(
        [SIZE, SIZE],
        &pixels.iter().flat_map(|c| c.to_array()).collect::<Vec<_>>(),
    );

    ctx.load_texture(
        "hatch_pattern",
        image,
        egui::TextureOptions {
            magnification: egui::TextureFilter::Linear,
            minification: egui::TextureFilter::Linear,
            wrap_mode: egui::TextureWrapMode::Repeat,
            ..Default::default()
        },
    )
}

/// Global drag state - tracks what is currently being dragged
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
    /// Sliding layer - moves in while compensating trim_in/trim_out to keep visible content in place
    SlidingLayer {
        layer_idx: usize,
        initial_in: i32,
        initial_trim_in: i32,
        initial_trim_out: i32,
        speed: f32,
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
/// Computed once in draw pass and reused in interaction pass to avoid duplicate calculations.
#[derive(Clone, Copy)]
pub(super) struct LayerGeom {
    pub full_bar_rect: eframe::egui::Rect,
    pub visible_bar_rect: Option<eframe::egui::Rect>,
}

impl LayerGeom {
    /// Calculate layer geometry. play_start/play_end are absolute frames in parent timeline.
    pub fn calc(
        child_start: i32,
        child_end: i32,
        play_start: i32,
        play_end: i32,
        child_y: f32,
        timeline_rect: eframe::egui::Rect,
        config: &TimelineConfig,
        pan_offset: f32,
        zoom: f32,
    ) -> Self {
        use eframe::egui::{Pos2, Rect};

        let frame_to_screen_x = |frame: f32, timeline_min_x: f32| -> f32 {
            let frame_offset = frame - pan_offset;
            timeline_min_x + (frame_offset * config.pixels_per_frame * zoom)
        };

        let full_bar_x_start = frame_to_screen_x(child_start as f32, timeline_rect.min.x);
        let full_bar_x_end = frame_to_screen_x((child_end + 1) as f32, timeline_rect.min.x);
        let full_bar_rect = Rect::from_min_max(
            Pos2::new(full_bar_x_start, child_y + 4.0),
            Pos2::new(full_bar_x_end, child_y + config.layer_height - 4.0),
        );

        let visible_bar_rect = if play_start <= play_end {
            let visible_bar_x_start = frame_to_screen_x(play_start as f32, timeline_rect.min.x);
            let visible_bar_x_end =
                frame_to_screen_x((play_end + 1) as f32, timeline_rect.min.x);
            Some(Rect::from_min_max(
                Pos2::new(visible_bar_x_start, child_y + 4.0),
                Pos2::new(visible_bar_x_end, child_y + config.layer_height - 4.0),
            ))
        } else {
            None
        };

        Self {
            full_bar_rect,
            visible_bar_rect,
        }
    }
}
