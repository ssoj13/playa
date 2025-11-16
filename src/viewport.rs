use eframe::egui;
use eframe::glow;
use eframe::glow::HasContext;
use log::{debug, error, info};

use crate::frame::{PixelBuffer, PixelFormat};
use crate::shaders::Shaders;

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

    /// Draw all viewport overlays (scrubber, guides, safe zones, etc.)
    pub fn draw(&self, ui: &egui::Ui, panel_rect: egui::Rect) {
        // Draw scrubber line during scrubbing
        self.scrubber.draw(ui, panel_rect);

        // Future: добавить guides, safe zones, grid, и т.д.
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

/// OpenGL renderer for viewport
pub struct ViewportRenderer {
    program: Option<glow::Program>,
    vao: Option<glow::VertexArray>,
    vbo: Option<glow::Buffer>,
    texture: Option<glow::Texture>,
    texture_width: usize,
    texture_height: usize,
    current_pixel_format: PixelFormat, // Track current format for shader uniforms
    current_shader_name: String,       // Track the current shader to know when to recompile
    current_vertex_shader: String,
    current_fragment_shader: String,
    needs_recompile: bool, // Flag to indicate shader needs recompilation
    // For async texture uploads
    pbos: [Option<glow::Buffer>; 2],
    pbo_index: usize,
    pbo_width: usize,
    pbo_height: usize,
    pbo_pixel_format: PixelFormat, // Track PBO format to detect when recreate is needed
    // HDR controls
    pub exposure: f32, // Exposure multiplier (default 1.0)
    pub gamma: f32,    // Gamma correction (default 2.2 for sRGB)

    // Scratch buffer to avoid per-frame allocations when converting f16 -> u16
    f16_scratch: Vec<u16>,

    // Last shader error message (if any)
    last_error: Option<String>,
}

impl ViewportRenderer {
    pub fn new() -> Self {
        let default_shader_manager = Shaders::new();
        let (vertex_shader, fragment_shader) = default_shader_manager.get_current_shaders();

        Self {
            program: None,
            vao: None,
            vbo: None,
            texture: None,
            texture_width: 0,
            texture_height: 0,
            current_pixel_format: PixelFormat::Rgba8, // Default to LDR
            current_shader_name: default_shader_manager.current_shader.clone(),
            current_vertex_shader: vertex_shader.to_string(),
            current_fragment_shader: fragment_shader.to_string(),
            needs_recompile: true, // Need to compile the initial shader
            pbos: [None, None],
            pbo_index: 0,
            pbo_width: 0,
            pbo_height: 0,
            pbo_pixel_format: PixelFormat::Rgba8, // Default format
            exposure: 1.0,                        // Default exposure
            gamma: 2.2,                           // Default sRGB gamma

            f16_scratch: Vec::new(),
            last_error: None,
        }
    }

    /// Update the current shader
    pub fn update_shader(&mut self, shader_manager: &Shaders) {
        if self.current_shader_name != shader_manager.current_shader {
            let (vertex_shader, fragment_shader) = shader_manager.get_current_shaders();
            info!(
                "Switching to shader: {}, recompiling...",
                shader_manager.current_shader
            );
            self.current_shader_name = shader_manager.current_shader.clone();
            self.current_vertex_shader = vertex_shader.to_string();
            self.current_fragment_shader = fragment_shader.to_string();
            self.needs_recompile = true; // Flag that recompilation is needed
        }
    }

    /// Initialize OpenGL resources (shaders, VAO, VBO)
    fn initialize(
        &mut self,
        gl: &glow::Context,
        vertex_shader_src: &str,
        fragment_shader_src: &str,
    ) {
        unsafe {
            // Clean up any existing program
            if let Some(program) = self.program.take() {
                gl.delete_program(program);
            }

            // Compile shaders
            let vertex_shader = match gl.create_shader(glow::VERTEX_SHADER) {
                Ok(shader) => shader,
                Err(e) => {
                    let msg = format!("Failed to create vertex shader: {}", e);
                    self.last_error = Some(msg.clone());
                    error!("{}", msg);
                    return;
                }
            };
            gl.shader_source(vertex_shader, vertex_shader_src);
            gl.compile_shader(vertex_shader);

            if !gl.get_shader_compile_status(vertex_shader) {
                let log = gl.get_shader_info_log(vertex_shader);
                let msg = format!("Vertex shader compilation failed: {}", log);
                self.last_error = Some(msg.clone());
                error!("{}", msg);
                gl.delete_shader(vertex_shader);
                return;
            }

            let fragment_shader = match gl.create_shader(glow::FRAGMENT_SHADER) {
                Ok(shader) => shader,
                Err(e) => {
                    let msg = format!("Failed to create fragment shader: {}", e);
                    self.last_error = Some(msg.clone());
                    error!("{}", msg);
                    gl.delete_shader(vertex_shader);
                    return;
                }
            };
            gl.shader_source(fragment_shader, fragment_shader_src);
            gl.compile_shader(fragment_shader);

            if !gl.get_shader_compile_status(fragment_shader) {
                let log = gl.get_shader_info_log(fragment_shader);
                let msg = format!("Fragment shader compilation failed: {}", log);
                self.last_error = Some(msg.clone());
                error!("{}", msg);
                gl.delete_shader(vertex_shader);
                gl.delete_shader(fragment_shader);
                return;
            }

            // Link program
            let program = match gl.create_program() {
                Ok(p) => p,
                Err(e) => {
                    let msg = format!("Failed to create shader program: {}", e);
                    self.last_error = Some(msg.clone());
                    error!("{}", msg);
                    gl.delete_shader(vertex_shader);
                    gl.delete_shader(fragment_shader);
                    return;
                }
            };
            gl.attach_shader(program, vertex_shader);
            gl.attach_shader(program, fragment_shader);
            gl.link_program(program);

            if !gl.get_program_link_status(program) {
                let log = gl.get_program_info_log(program);
                let msg = format!("Shader program linking failed: {}", log);
                self.last_error = Some(msg.clone());
                error!("{}", msg);
                gl.delete_shader(vertex_shader);
                gl.delete_shader(fragment_shader);
                gl.delete_program(program);
                return;
            }

            gl.delete_shader(vertex_shader);
            gl.delete_shader(fragment_shader);

            self.program = Some(program);
            // Clear last error on success
            self.last_error = None;

            // Create VAO and VBO for textured quad
            if self.vao.is_none() {
                let vao = match gl.create_vertex_array() {
                    Ok(arr) => arr,
                    Err(e) => {
                        error!("Failed to create vertex array: {}", e);
                        return;
                    }
                };
                gl.bind_vertex_array(Some(vao));

                let vbo = match gl.create_buffer() {
                    Ok(buf) => buf,
                    Err(e) => {
                        error!("Failed to create buffer: {}", e);
                        return;
                    }
                };
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));

                // Quad vertices: position (vec2) + uv (vec2)
                // Centered at origin, will be transformed by view/projection matrices
                #[rustfmt::skip]
                let vertices: [f32; 16] = [
                    // pos.x, pos.y, uv.x, uv.y
                    -0.5, -0.5,  0.0, 1.0,  // bottom-left
                     0.5, -0.5,  1.0, 1.0,  // bottom-right
                     0.5,  0.5,  1.0, 0.0,  // top-right
                    -0.5,  0.5,  0.0, 0.0,  // top-left
                ];

                gl.buffer_data_u8_slice(
                    glow::ARRAY_BUFFER,
                    bytemuck::cast_slice(&vertices),
                    glow::STATIC_DRAW,
                );

                // Position attribute
                gl.enable_vertex_attrib_array(0);
                gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 16, 0);

                // UV attribute
                gl.enable_vertex_attrib_array(1);
                gl.vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, 16, 8);

                gl.bind_vertex_array(None);

                self.vao = Some(vao);
                self.vbo = Some(vbo);
            }

            info!("ViewportRenderer initialized successfully with new shaders");
        }
    }

    /// Recreate PBOs if image dimensions or pixel format have changed
    fn recreate_pbos_if_needed(
        &mut self,
        gl: &glow::Context,
        width: usize,
        height: usize,
        pixel_format: PixelFormat,
    ) {
        if self.pbo_width == width
            && self.pbo_height == height
            && self.pbo_pixel_format == pixel_format
        {
            return; // PBOs are already the correct size and format
        }

        unsafe {
            // Delete old PBOs
            if let Some(pbo) = self.pbos[0].take() {
                gl.delete_buffer(pbo);
            }
            if let Some(pbo) = self.pbos[1].take() {
                gl.delete_buffer(pbo);
            }

            // Calculate buffer size based on pixel format
            // U8: 1 byte per channel, F16: 2 bytes per channel, F32: 4 bytes per channel
            let bytes_per_channel = match pixel_format {
                PixelFormat::Rgba8 => 1,
                PixelFormat::RgbaF16 => 2,
                PixelFormat::RgbaF32 => 4,
            };
            let buffer_size = (width * height * 4 * bytes_per_channel) as i32;

            for i in 0..2 {
                let pbo = gl.create_buffer().ok();
                if let Some(pbo) = pbo {
                    gl.bind_buffer(glow::PIXEL_UNPACK_BUFFER, Some(pbo));
                    gl.buffer_data_size(glow::PIXEL_UNPACK_BUFFER, buffer_size, glow::STREAM_DRAW);
                    self.pbos[i] = Some(pbo);
                } else {
                    error!("Failed to create PBO {}", i);
                }
            }
            gl.bind_buffer(glow::PIXEL_UNPACK_BUFFER, None);

            self.pbo_width = width;
            self.pbo_height = height;
            self.pbo_pixel_format = pixel_format;
            debug!(
                "Recreated PBOs for size {}x{} format {:?} (buffer_size: {} bytes)",
                width, height, pixel_format, buffer_size
            );
        }
    }

    /// Upload texture to GPU asynchronously using PBOs
    pub fn upload_texture(
        &mut self,
        gl: &glow::Context,
        width: usize,
        height: usize,
        pixel_buffer: &PixelBuffer,
        pixel_format: PixelFormat,
    ) {
        // Save pixel format for shader uniform
        self.current_pixel_format = pixel_format;

        unsafe {
            // Prepare bytes from pixel buffer and map to GL formats
            // For F16 we create an owned byte buffer to avoid borrowing self across calls
            let mut owned_bytes: Option<Vec<u8>> = None;
            let (pixels_bytes, gl_internal_format, gl_format, gl_type) = match pixel_buffer {
                PixelBuffer::U8(vec) => (
                    vec.as_slice(),
                    glow::RGBA as i32,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                ),
                PixelBuffer::F16(_) => {
                    // Convert F16 → u16 → bytes (reuse scratch buffer)
                    if let PixelBuffer::F16(src) = pixel_buffer {
                        self.f16_scratch.clear();
                        self.f16_scratch.extend(src.iter().map(|f| f.to_bits()));
                    }
                    let bytes_u8: Vec<u8> =
                        bytemuck::cast_slice(self.f16_scratch.as_slice()).to_vec();
                    owned_bytes = Some(bytes_u8);
                    (
                        owned_bytes.as_ref().unwrap().as_slice(),
                        glow::RGBA16F as i32,
                        glow::RGBA,
                        glow::HALF_FLOAT,
                    )
                }
                PixelBuffer::F32(vec) => {
                    let bytes = bytemuck::cast_slice(vec.as_slice());
                    (bytes, glow::RGBA32F as i32, glow::RGBA, glow::FLOAT)
                }
            };

            let is_initial_upload = self.texture.is_none()
                || self.texture_width != width
                || self.texture_height != height;

            // Ensure texture exists and is the correct size
            if is_initial_upload {
                if self.texture.is_none() {
                    self.texture = gl.create_texture().ok();
                }
                let texture = self.texture.unwrap();
                gl.bind_texture(glow::TEXTURE_2D, Some(texture));
                gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    gl_internal_format,
                    width as i32,
                    height as i32,
                    0,
                    gl_format,
                    gl_type,
                    eframe::glow::PixelUnpackData::Slice(None), // Allocate texture memory
                );
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_MIN_FILTER,
                    glow::LINEAR as i32,
                );
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_MAG_FILTER,
                    glow::LINEAR as i32,
                );
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_WRAP_S,
                    glow::CLAMP_TO_EDGE as i32,
                );
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_WRAP_T,
                    glow::CLAMP_TO_EDGE as i32,
                );

                self.texture_width = width;
                self.texture_height = height;
                debug!(
                    "Recreated texture for size {}x{} format {:?}",
                    width, height, pixel_format
                );
            }

            // Ensure PBOs are the correct size and format
            self.recreate_pbos_if_needed(gl, width, height, pixel_format);

            let texture = self.texture.unwrap();
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));

            let write_pbo_index = self.pbo_index;
            let transfer_pbo_index = (self.pbo_index + 1) % 2;

            // --- Step 1: Write current frame's data to the "write" PBO ---
            if let Some(write_pbo) = self.pbos[write_pbo_index] {
                gl.bind_buffer(glow::PIXEL_UNPACK_BUFFER, Some(write_pbo));
                // Orphan previous data to avoid stalls
                gl.buffer_data_size(
                    glow::PIXEL_UNPACK_BUFFER,
                    pixels_bytes.len() as i32,
                    glow::STREAM_DRAW,
                );
                let ptr = gl.map_buffer_range(
                    glow::PIXEL_UNPACK_BUFFER,
                    0,
                    pixels_bytes.len() as i32,
                    glow::MAP_WRITE_BIT | glow::MAP_INVALIDATE_BUFFER_BIT,
                );

                if !ptr.is_null() {
                    let dest_slice = std::slice::from_raw_parts_mut(ptr, pixels_bytes.len());
                    dest_slice.copy_from_slice(pixels_bytes);
                    gl.unmap_buffer(glow::PIXEL_UNPACK_BUFFER);
                } else {
                    error!("Failed to map PBO for writing");
                }
                gl.bind_buffer(glow::PIXEL_UNPACK_BUFFER, None);
            }

            // --- Step 2: Transfer data to texture ---
            if is_initial_upload {
                // On the first upload, we do a synchronous transfer from the PBO we just wrote to.
                // This populates the texture immediately.
                if let Some(write_pbo) = self.pbos[write_pbo_index] {
                    gl.bind_buffer(glow::PIXEL_UNPACK_BUFFER, Some(write_pbo));
                    gl.tex_sub_image_2d(
                        glow::TEXTURE_2D,
                        0,
                        0,
                        0,
                        width as i32,
                        height as i32,
                        gl_format,
                        gl_type,
                        glow::PixelUnpackData::BufferOffset(0),
                    );
                    gl.bind_buffer(glow::PIXEL_UNPACK_BUFFER, None);
                }
            } else {
                // On subsequent frames, we do an asynchronous transfer from the *other* PBO
                // (which contains the data from the previous frame).
                if let Some(transfer_pbo) = self.pbos[transfer_pbo_index] {
                    gl.bind_buffer(glow::PIXEL_UNPACK_BUFFER, Some(transfer_pbo));
                    gl.tex_sub_image_2d(
                        glow::TEXTURE_2D,
                        0,
                        0,
                        0,
                        width as i32,
                        height as i32,
                        gl_format,
                        gl_type,
                        glow::PixelUnpackData::BufferOffset(0),
                    );
                    gl.bind_buffer(glow::PIXEL_UNPACK_BUFFER, None);
                }
            }

            gl.bind_texture(glow::TEXTURE_2D, None);

            // Swap PBOs for the next frame
            self.pbo_index = (self.pbo_index + 1) % 2;
        }
    }

    /// Return the last shader error message, if any
    pub fn shader_error(&self) -> Option<String> {
        self.last_error.clone()
    }

    /// Render the viewport
    pub fn render(&mut self, gl: &glow::Context, viewport_state: &ViewportState) {
        // Check if we need to (re)compile shaders - store values to avoid borrow checker issues
        let needs_recompile = self.needs_recompile || self.program.is_none();
        if needs_recompile {
            let vertex_shader = self.current_vertex_shader.clone();
            let fragment_shader = self.current_fragment_shader.clone();
            info!("Recompiling shader: {}", self.current_shader_name);
            self.initialize(gl, &vertex_shader, &fragment_shader);
            self.needs_recompile = false; // Reset the recompile flag after compilation
        }

        let program = match self.program {
            Some(p) => p,
            None => return,
        };

        let vao = match self.vao {
            Some(v) => v,
            None => return,
        };

        let texture = match self.texture {
            Some(t) => t,
            None => return, // No texture to render
        };

        unsafe {
            gl.use_program(Some(program));

            // Set uniforms
            let view_matrix = viewport_state.get_view_matrix();
            let proj_matrix = viewport_state.get_projection_matrix();

            if let Some(loc) = gl.get_uniform_location(program, "u_view") {
                gl.uniform_matrix_4_f32_slice(
                    Some(&loc),
                    false,
                    bytemuck::cast_slice(&view_matrix),
                );
            }

            if let Some(loc) = gl.get_uniform_location(program, "u_projection") {
                gl.uniform_matrix_4_f32_slice(
                    Some(&loc),
                    false,
                    bytemuck::cast_slice(&proj_matrix),
                );
            }

            // Bind texture
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));

            if let Some(loc) = gl.get_uniform_location(program, "u_texture") {
                gl.uniform_1_i32(Some(&loc), 0);
            }

            // Set HDR uniforms (exposure and gamma) if shader supports them
            if let Some(loc) = gl.get_uniform_location(program, "u_exposure") {
                gl.uniform_1_f32(Some(&loc), self.exposure);
            }
            if let Some(loc) = gl.get_uniform_location(program, "u_gamma") {
                gl.uniform_1_f32(Some(&loc), self.gamma);
            }

            // Set u_is_hdr based on pixel format (0 for LDR/U8, 1 for HDR/F16/F32)
            if let Some(loc) = gl.get_uniform_location(program, "u_is_hdr") {
                let is_hdr = match self.current_pixel_format {
                    PixelFormat::Rgba8 => 0,   // LDR - already in sRGB
                    PixelFormat::RgbaF16 => 1, // HDR - needs processing
                    PixelFormat::RgbaF32 => 1, // HDR - needs processing
                };
                gl.uniform_1_i32(Some(&loc), is_hdr);
            }

            // Draw quad
            gl.bind_vertex_array(Some(vao));
            gl.draw_arrays(glow::TRIANGLE_FAN, 0, 4);
            gl.bind_vertex_array(None);

            gl.use_program(None);
        }
    }

    /// Check if texture needs update
    pub fn needs_texture_update(&self, width: usize, height: usize) -> bool {
        self.texture.is_none() || self.texture_width != width || self.texture_height != height
    }

    /// Cleanup OpenGL resources
    pub fn destroy(&mut self, gl: &glow::Context) {
        unsafe {
            if let Some(texture) = self.texture.take() {
                gl.delete_texture(texture);
            }
            if let Some(vbo) = self.vbo.take() {
                gl.delete_buffer(vbo);
            }
            if let Some(pbo) = self.pbos[0].take() {
                gl.delete_buffer(pbo);
            }
            if let Some(pbo) = self.pbos[1].take() {
                gl.delete_buffer(pbo);
            }
            if let Some(vao) = self.vao.take() {
                gl.delete_vertex_array(vao);
            }
            if let Some(program) = self.program.take() {
                gl.delete_program(program);
            }
        }
    }
}

impl Drop for ViewportRenderer {
    fn drop(&mut self) {
        // Note: Cannot safely cleanup OpenGL resources here without context
        // Must call destroy() explicitly before dropping
        if self.program.is_some() {
            error!("ViewportRenderer dropped without calling destroy()");
        }
    }
}
