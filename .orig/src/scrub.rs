use eframe::egui;

/// Scrubber line color when inside image bounds (white, 50% transparent)
const SCRUB_NORMAL: (f32, f32, f32, f32) = (1.0, 1.0, 1.0, 0.5);

/// Scrubber line color when outside image bounds (dark red, 50% transparent)
const SCRUB_OUTSIDE: (f32, f32, f32, f32) = (0.75, 0.0, 0.0, 0.5);

/// Scrubbing control for timeline navigation via mouse
pub struct Scrubber {
    is_active: bool,
    normalized_position: Option<f32>, // Normalized position along timeline (can be outside 0.0..1.0)
    visual_x: Option<f32>,            // Pixel X coordinate for drawing
    current_frame: Option<usize>,
    is_clamped: bool, // True when normalized is outside 0.0..1.0 (frame is clamped)
    frozen_bounds: Option<egui::Rect>, // Frozen image bounds during scrubbing
    frozen_image_size: Option<egui::Vec2>, // Frozen image size for detecting changes
    last_mouse_x: Option<f32>, // Last mouse X position for movement detection
}

impl Scrubber {
    /// Create a new scrubber instance
    pub fn new() -> Self {
        Self {
            is_active: false,
            normalized_position: None,
            visual_x: None,
            current_frame: None,
            is_clamped: false,
            frozen_bounds: None,
            frozen_image_size: None,
            last_mouse_x: None,
        }
    }

    /// Draw visual feedback for scrubbing
    pub fn draw(&self, ui: &egui::Ui, panel_rect: egui::Rect) {
        if !self.is_active {
            return;
        }

        if let Some(visual_x) = self.visual_x {
            let painter = ui.painter();

            // Select color based on clamped state
            let (r, g, b, a) = if self.is_clamped {
                SCRUB_OUTSIDE
            } else {
                SCRUB_NORMAL
            };
            let line_color = egui::Color32::from_rgba_unmultiplied(
                (r * 255.0) as u8,
                (g * 255.0) as u8,
                (b * 255.0) as u8,
                (a * 255.0) as u8,
            );

            // Draw vertical line
            let line_top = egui::pos2(visual_x, panel_rect.top());
            let line_bottom = egui::pos2(visual_x, panel_rect.bottom());
            painter.line_segment([line_top, line_bottom], egui::Stroke::new(1.0, line_color));

            // Draw frame number overlay (same color as line)
            if let Some(frame) = self.current_frame {
                let text = format!("{}", frame);
                let text_pos = egui::pos2(visual_x + 10.0, panel_rect.top() + 10.0);
                painter.text(
                    text_pos,
                    egui::Align2::LEFT_TOP,
                    text,
                    egui::FontId::proportional(12.0),
                    line_color,
                );
            }
        }
    }

    /// Check if scrubbing is currently active
    pub fn is_active(&self) -> bool {
        self.is_active
    }

    /// Start scrubbing with frozen image bounds, size, and initial normalized position
    pub fn start_scrubbing(
        &mut self,
        image_bounds: egui::Rect,
        image_size: egui::Vec2,
        normalized: f32,
    ) {
        self.is_active = true;
        self.frozen_bounds = Some(image_bounds);
        self.frozen_image_size = Some(image_size);
        self.normalized_position = Some(normalized);
    }

    /// Stop scrubbing
    pub fn stop_scrubbing(&mut self) {
        self.is_active = false;
        self.normalized_position = None;
        self.visual_x = None;
        self.current_frame = None;
        self.is_clamped = false;
        self.frozen_bounds = None;
        self.frozen_image_size = None;
        self.last_mouse_x = None;
    }

    /// Get frozen image bounds (used during scrubbing)
    pub fn frozen_bounds(&self) -> Option<egui::Rect> {
        self.frozen_bounds
    }

    /// Set normalized position (can be outside 0.0..1.0 when mouse is outside image bounds)
    pub fn set_normalized_position(&mut self, normalized: f32) {
        self.normalized_position = Some(normalized);
    }

    /// Set visual X position for drawing
    pub fn set_visual_x(&mut self, x: f32) {
        self.visual_x = Some(x);
    }

    /// Set current frame
    pub fn set_current_frame(&mut self, frame: usize) {
        self.current_frame = Some(frame);
    }

    /// Set clamped state (true when normalized is outside 0.0..1.0)
    pub fn set_clamped(&mut self, clamped: bool) {
        self.is_clamped = clamped;
    }

    /// Get normalized position
    pub fn normalized_position(&self) -> Option<f32> {
        self.normalized_position
    }

    /// Set last mouse X position
    pub fn set_last_mouse_x(&mut self, mouse_x: f32) {
        self.last_mouse_x = Some(mouse_x);
    }

    /// Check if mouse has moved (threshold 0.1 pixels)
    pub fn mouse_moved(&self, current_mouse_x: f32) -> bool {
        if let Some(last_x) = self.last_mouse_x {
            (current_mouse_x - last_x).abs() > 0.1
        } else {
            true // First mouse position always counts as "moved"
        }
    }

    /// Convert mouse X pixel coordinate to normalized position (can be outside 0.0..1.0)
    pub fn mouse_to_normalized(mouse_x: f32, bounds: egui::Rect) -> f32 {
        let left = bounds.min.x;
        let right = bounds.max.x;
        if right > left {
            (mouse_x - left) / (right - left)
        } else {
            0.5
        }
    }

    /// Convert normalized position (0.0..1.0) to pixel X coordinate
    #[allow(dead_code)]
    pub fn normalized_to_pixel(normalized: f32, bounds: egui::Rect) -> f32 {
        let left = bounds.min.x;
        let right = bounds.max.x;
        left + normalized * (right - left)
    }

    /// Convert normalized position to frame index (clamps to valid range)
    pub fn normalized_to_frame(normalized: f32, total_frames: usize) -> usize {
        if total_frames > 1 {
            let clamped = normalized.clamp(0.0, 1.0);
            (clamped * (total_frames - 1) as f32).round() as usize
        } else {
            0
        }
    }

    /// Convert frame index to normalized position (0.0..1.0)
    #[allow(dead_code)]
    pub fn frame_to_normalized(frame_idx: usize, total_frames: usize) -> f32 {
        if total_frames > 1 {
            frame_idx as f32 / (total_frames - 1) as f32
        } else {
            0.5
        }
    }
}
