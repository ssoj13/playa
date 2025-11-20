use eframe::egui;
use log::{debug, info};


/// Scrubber line color when inside image bounds (white, 50% transparent)
const SCRUB_NORMAL: (f32, f32, f32, f32) = (1.0, 1.0, 1.0, 0.5);

/// Scrubber line color when outside image bounds (dark red, 50% transparent)
const SCRUB_OUTSIDE: (f32, f32, f32, f32) = (0.75, 0.0, 0.0, 0.5);

// Zoom constants
const ZOOM_STEP: f32 = 0.025;
const ZOOM_IN_FACTOR: f32 = 1.0 + ZOOM_STEP;
const ZOOM_OUT_FACTOR: f32 = 1.0 / ZOOM_IN_FACTOR;

/// Viewport mode
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum ViewportMode {
    /// Manual mode - user controls zoom/pan, nothing auto-adjusts
    Manual,
    /// Auto-fit mode - image fits to window, adjusts on resize
    AutoFit,
    /// Auto-100% mode - image at 100% zoom, no auto-adjust on resize
    Auto100,
}

/// Viewport state for pan/zoom
#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub struct ViewportState {
    pub zoom: f32,
    pub pan: egui::Vec2,
    pub mode: ViewportMode,
    #[serde(skip)]
    pub image_size: egui::Vec2,
    #[serde(skip)]
    pub viewport_size: egui::Vec2,
    #[serde(skip)]
    pub scrubber: ViewportScrubber,
}

impl Default for ViewportState {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
            mode: ViewportMode::AutoFit,
            image_size: egui::Vec2::new(1920.0, 1080.0),
            viewport_size: egui::Vec2::new(1920.0, 1080.0),
            scrubber: ViewportScrubber::new(),
        }
    }
}

impl ViewportState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset viewport to default zoom and pan
    pub fn reset(&mut self) {
        self.zoom = 1.0;
        self.pan = egui::Vec2::ZERO;
        self.mode = ViewportMode::AutoFit;
    }

    /// Draw all viewport overlays (scrubber, guides, safe zones, etc.)
    pub fn draw(&self, ui: &egui::Ui, panel_rect: egui::Rect) {
        // Draw scrubber line during scrubbing
        self.scrubber.draw(ui, panel_rect);

        // Future: Ð´Ð¾Ð±Ð°Ð²Ð¸Ñ‚ÑŒ guides, safe zones, grid, Ð¸ Ñ‚.Ð´.
    }

    /// Update viewport size (called when window resizes)
    pub fn set_viewport_size(&mut self, size: egui::Vec2) {
        self.viewport_size = size;
        // Auto-refit if in AutoFit mode
        if self.mode == ViewportMode::AutoFit {
            self.apply_fit();
        }
    }

    /// Update image size (called when new image loads)
    pub fn set_image_size(&mut self, size: egui::Vec2) {
        self.image_size = size;
        // If we're in AutoFit, recompute fit when image size changes
        if self.mode == ViewportMode::AutoFit {
            self.apply_fit();
        }
    }

    /// Set AutoFit mode and apply fit
    pub fn set_mode_fit(&mut self) {
        info!("Viewport mode: AutoFit");
        self.mode = ViewportMode::AutoFit;
        self.apply_fit();
    }

    /// Set Auto100 mode and apply 100% zoom
    pub fn set_mode_100(&mut self) {
        info!("Viewport mode: Auto100");
        self.mode = ViewportMode::Auto100;
        self.apply_100();
    }

    /// Apply fit to window
    fn apply_fit(&mut self) {
        if self.image_size.x <= 0.0 || self.image_size.y <= 0.0 {
            return;
        }
        let scale_x = self.viewport_size.x / self.image_size.x;
        let scale_y = self.viewport_size.y / self.image_size.y;
        self.zoom = scale_x.min(scale_y);
        self.pan = egui::Vec2::ZERO;
    }

    /// Apply 100% zoom
    fn apply_100(&mut self) {
        self.zoom = 1.0;
        self.pan = egui::Vec2::ZERO;
    }

    /// Handle zoom with center-on-cursor (switches to Manual mode)
    pub fn handle_zoom(&mut self, zoom_delta: f32, cursor_pos: egui::Vec2) {
        if zoom_delta.abs() < 0.001 {
            return;
        }

        self.mode = ViewportMode::Manual;

        let old_zoom = self.zoom;
        let zoom_factor = if zoom_delta > 0.0 {
            ZOOM_IN_FACTOR
        } else {
            ZOOM_OUT_FACTOR
        };
        self.zoom = (self.zoom * zoom_factor).clamp(0.01, 100.0);

        // Adjust pan to keep the point under the cursor stationary
        let zoom_ratio = self.zoom / old_zoom;
        let mut cursor_to_center = cursor_pos - self.viewport_size * 0.5;
        cursor_to_center.y = -cursor_to_center.y;
        self.pan = cursor_to_center - (cursor_to_center - self.pan) * zoom_ratio;

        debug!(
            "Zoom: {:.2}x, Pan: ({:.1}, {:.1})",
            self.zoom, self.pan.x, self.pan.y
        );
    }

    /// Handle pan (switches to Manual mode)
    pub fn handle_pan(&mut self, delta: egui::Vec2) {
        self.mode = ViewportMode::Manual;
        self.pan += egui::vec2(delta.x, -delta.y);
        debug!("Pan: ({:.1}, {:.1})", self.pan.x, self.pan.y);
    }

    /// Get image bounds in screen space
    pub fn get_image_screen_bounds(&self) -> egui::Rect {
        let min = self.image_to_screen(egui::vec2(0.0, 0.0));
        let max = self.image_to_screen(self.image_size);
        egui::Rect::from_min_max(min.to_pos2(), max.to_pos2())
    }

    /// Check if screen position is over the image
    #[allow(dead_code)]
    pub fn is_point_over_image(&self, screen_pos: egui::Vec2) -> bool {
        self.screen_to_image(screen_pos).is_some()
    }

    /// Convert image space coordinates (0..image_size) to screen space
    pub fn image_to_screen(&self, image_pos: egui::Vec2) -> egui::Vec2 {
        // image (0..image_size) -> local (-0.5..0.5)
        let local = egui::vec2(
            image_pos.x / self.image_size.x - 0.5,
            image_pos.y / self.image_size.y - 0.5,
        );

        // local -> viewport space (apply view transform)
        let viewport = egui::vec2(
            local.x * self.image_size.x * self.zoom + self.pan.x,
            local.y * self.image_size.y * self.zoom + self.pan.y,
        );

        // viewport -> screen space
        egui::vec2(
            viewport.x + self.viewport_size.x / 2.0,
            viewport.y + self.viewport_size.y / 2.0,
        )
    }

    /// Convert screen space coordinates to image space (0..image_size)
    /// Returns None if position is outside the image bounds
    #[allow(dead_code)]
    pub fn screen_to_image(&self, screen_pos: egui::Vec2) -> Option<egui::Vec2> {
        // screen -> viewport space
        let viewport = egui::vec2(
            screen_pos.x - self.viewport_size.x / 2.0,
            screen_pos.y - self.viewport_size.y / 2.0,
        );

        // viewport -> local space (inverse view transform)
        let local = egui::vec2(
            (viewport.x - self.pan.x) / (self.image_size.x * self.zoom),
            (viewport.y - self.pan.y) / (self.image_size.y * self.zoom),
        );

        // local (-0.5..0.5) -> image (0..image_size)
        let image = egui::vec2(
            (local.x + 0.5) * self.image_size.x,
            (local.y + 0.5) * self.image_size.y,
        );

        // Check bounds
        if image.x >= 0.0
            && image.x <= self.image_size.x
            && image.y >= 0.0
            && image.y <= self.image_size.y
        {
            Some(image)
        } else {
            None
        }
    }

    /// High-level scrubbing handler. Returns Some(frame_idx) when scrubbing
    /// requests a new frame, or None if nothing changed.
    pub fn handle_scrubbing(
        &mut self,
        response: &egui::Response,
        double_clicked: bool,
        total_frames: usize,
    ) -> Option<usize> {
        if double_clicked || total_frames == 0 {
            return None;
        }

        // Precompute bounds before mutably borrowing scrubber to avoid borrow conflicts
        let current_bounds = self.get_image_screen_bounds();
        let current_size = self.image_size;

        let scrubber = &mut self.scrubber;

        // Start or continue scrubbing on primary click/drag
        if (response.clicked_by(egui::PointerButton::Primary)
            || response.dragged_by(egui::PointerButton::Primary))
            && let Some(mouse_pos) = response.interact_pointer_pos()
        {
            // Start scrubbing - freeze bounds
                if !scrubber.is_active() {
                    let normalized =
                        ViewportScrubber::mouse_to_normalized(mouse_pos.x, current_bounds);
                    scrubber.start_scrubbing(current_bounds, current_size, normalized);
                    scrubber.set_last_mouse_x(mouse_pos.x);
                }

                // Use frozen bounds for entire scrubbing session
                let image_bounds = scrubber
                    .frozen_bounds()
                    .unwrap_or(current_bounds);

            let frame_idx = if scrubber.mouse_moved(mouse_pos.x) {
                // Mouse moved - recalculate normalized from mouse
                let normalized =
                    ViewportScrubber::mouse_to_normalized(mouse_pos.x, image_bounds);
                scrubber.set_normalized_position(normalized);
                scrubber.set_last_mouse_x(mouse_pos.x);

                let is_clamped = !(0.0..=1.0).contains(&normalized);
                scrubber.set_clamped(is_clamped);

                let frame_idx =
                    ViewportScrubber::normalized_to_frame(normalized, total_frames);
                scrubber.set_current_frame(frame_idx);

                // Visual line follows mouse everywhere (can be outside image bounds)
                scrubber.set_visual_x(mouse_pos.x);
                frame_idx
            } else {
                // Mouse didn't move - keep saved normalized position
                let saved_normalized = scrubber.normalized_position().unwrap_or(0.5);
                let is_clamped = !(0.0..=1.0).contains(&saved_normalized);
                scrubber.set_clamped(is_clamped);

                let frame_idx = ViewportScrubber::normalized_to_frame(
                    saved_normalized,
                    total_frames,
                );
                scrubber.set_current_frame(frame_idx);

                let visual_x =
                    ViewportScrubber::normalized_to_pixel(saved_normalized, image_bounds);
                scrubber.set_visual_x(visual_x);
                frame_idx
            };

            Some(frame_idx)
        } else if response.drag_stopped() || response.clicked() {
            // On release, stop scrubbing but keep last frame
            scrubber.stop_scrubbing();
            None
        } else {
            None
        }
    }

    /// Get view matrix for shader (2D transform: translate + scale)
    pub fn get_view_matrix(&self) -> [[f32; 4]; 4] {
        // 2D transform matrix: scale + translate
        // We center the image in viewport space
        let aspect_corrected_zoom_x = self.zoom * self.image_size.x;
        let aspect_corrected_zoom_y = self.zoom * self.image_size.y;

        [
            [aspect_corrected_zoom_x, 0.0, 0.0, 0.0],
            [0.0, aspect_corrected_zoom_y, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [self.pan.x, self.pan.y, 0.0, 1.0],
        ]
    }

    /// Get orthographic projection matrix for shader
    pub fn get_projection_matrix(&self) -> [[f32; 4]; 4] {
        // Orthographic projection: map viewport to [-1, 1] clip space
        let w = self.viewport_size.x;
        let h = self.viewport_size.y;

        if w <= 0.0 || h <= 0.0 {
            return Self::identity_matrix();
        }

        let left = -w / 2.0;
        let right = w / 2.0;
        let bottom = -h / 2.0;
        let top = h / 2.0;

        [
            [2.0 / (right - left), 0.0, 0.0, 0.0],
            [0.0, 2.0 / (top - bottom), 0.0, 0.0],
            [0.0, 0.0, -1.0, 0.0],
            [
                -(right + left) / (right - left),
                -(top + bottom) / (top - bottom),
                0.0,
                1.0,
            ],
        ]
    }

    fn identity_matrix() -> [[f32; 4]; 4] {
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
    }
}

/// Scrubbing control for timeline navigation via mouse, attached to the viewport.
#[derive(Clone, Default)]
pub struct ViewportScrubber {
    is_active: bool,
    normalized_position: Option<f32>, // Normalized position along timeline (can be outside 0.0..1.0)
    visual_x: Option<f32>,            // Pixel X coordinate for drawing
    current_frame: Option<usize>,
    is_clamped: bool, // True when normalized is outside 0.0..1.0 (frame is clamped)
    frozen_bounds: Option<egui::Rect>, // Frozen image bounds during scrubbing
    frozen_image_size: Option<egui::Vec2>, // Frozen image size for detecting changes
    last_mouse_x: Option<f32>, // Last mouse X position for movement detection
}

impl ViewportScrubber {
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

    pub fn draw(&self, ui: &egui::Ui, panel_rect: egui::Rect) {
        if !self.is_active {
            return;
        }

        if let Some(visual_x) = self.visual_x {
            let painter = ui.painter();

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

            let line_top = egui::pos2(visual_x, panel_rect.top());
            let line_bottom = egui::pos2(visual_x, panel_rect.bottom());
            painter.line_segment([line_top, line_bottom], egui::Stroke::new(1.0, line_color));

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

    pub fn is_active(&self) -> bool {
        self.is_active
    }

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

    pub fn frozen_bounds(&self) -> Option<egui::Rect> {
        self.frozen_bounds
    }

    pub fn set_normalized_position(&mut self, normalized: f32) {
        self.normalized_position = Some(normalized);
    }

    pub fn set_visual_x(&mut self, x: f32) {
        self.visual_x = Some(x);
    }

    pub fn set_current_frame(&mut self, frame: usize) {
        self.current_frame = Some(frame);
    }

    pub fn set_clamped(&mut self, clamped: bool) {
        self.is_clamped = clamped;
    }

    pub fn normalized_position(&self) -> Option<f32> {
        self.normalized_position
    }

    pub fn set_last_mouse_x(&mut self, mouse_x: f32) {
        self.last_mouse_x = Some(mouse_x);
    }

    pub fn mouse_moved(&self, current_mouse_x: f32) -> bool {
        if let Some(last_x) = self.last_mouse_x {
            (current_mouse_x - last_x).abs() > 0.1
        } else {
            true
        }
    }

    pub fn mouse_to_normalized(mouse_x: f32, bounds: egui::Rect) -> f32 {
        let left = bounds.min.x;
        let right = bounds.max.x;
        if right > left {
            (mouse_x - left) / (right - left)
        } else {
            0.5
        }
    }

    pub fn normalized_to_pixel(normalized: f32, bounds: egui::Rect) -> f32 {
        let left = bounds.min.x;
        let right = bounds.max.x;
        left + normalized * (right - left)
    }

    pub fn normalized_to_frame(normalized: f32, total_frames: usize) -> usize {
        if total_frames > 1 {
            let clamped = normalized.clamp(0.0, 1.0);
            (clamped * (total_frames - 1) as f32).round() as usize
        } else {
            0
        }
    }
}
