//! Timeline widget - state and configuration.
//! Shared by outline/canvas renderers and the UI tab. Data flow: UI mutations
//! update `TimelineState` (zoom/pan/selection) and emit `AppEvent`s which
//! are bridged to the EventBus; renderers read `TimelineConfig`/`TimelineState`
//! to draw rows/bars and handle interactions.

use crate::widgets::dnd::GlobalDragState;
use eframe::egui;
use playa_engine::entities::Attrs;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
            layer_height: 30.0,
            name_column_width: 80.0,
            pixels_per_frame: 2.0, // 2 pixels per frame by default
        }
    }
}

impl TimelineConfig {
    /// Create config with custom settings
    pub fn new(layer_height: f32, name_column_width: f32) -> Self {
        Self {
            layer_height,
            name_column_width,
            ..Default::default()
        }
    }
}

/// Timeline state (persistent between frames)
#[derive(Clone, Serialize, Deserialize)]
pub struct TimelineState {
    pub zoom: f32,       // Zoom multiplier (1.0 = default, range 0.1..4.0)
    pub pan_offset: f32, // Horizontal scroll offset in frames
    /// View state for the consumed `egui-track-timeline` widget. `zoom`/`pan_offset`
    /// above remain playa's canonical values and are mirrored into this each frame
    /// (so the toolbar slider, Fit, the ruler-aligned cache status strip, and
    /// persistence keep working); the widget's transient `drag` is serde-skipped
    /// inside `TimelineView`.
    #[serde(default)]
    pub track_view: egui_track_timeline::TimelineView,
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

    // === Layout rename dialog state ===
    /// Whether the layout rename dialog is currently open.
    /// Triggered by clicking the rename button (pencil icon) in toolbar.
    #[serde(skip)]
    pub rename_dialog_open: bool,
    /// The new name being edited in the rename dialog text field.
    #[serde(skip)]
    pub rename_dialog_name: String,
    /// The original layout name before rename (needed for LayoutRenamedEvent).
    #[serde(skip)]
    pub rename_dialog_old_name: String,
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
            .field(
                "hatch_texture",
                &self.hatch_texture.as_ref().map(|_| "TextureHandle"),
            )
            .field("clipboard", &format!("{} layers", self.clipboard.len()))
            .field("track_view", &self.track_view)
            .field("rename_dialog_open", &self.rename_dialog_open)
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
            track_view: egui_track_timeline::TimelineView::default(),
            rename_dialog_open: false,
            rename_dialog_name: String::new(),
            rename_dialog_old_name: String::new(),
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

#[derive(Clone, Debug, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimelineViewMode {
    Split,
    CanvasOnly,
    OutlineOnly,
}

