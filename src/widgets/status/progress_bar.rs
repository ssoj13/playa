use eframe::egui;

/// Progress bar widget for displaying loading progress
pub struct ProgressBar {
    current: usize,
    total: usize,
    width: f32,
    height: f32,
    fill_color: egui::Color32,
}

impl ProgressBar {
    /// Create new progress bar with specified dimensions
    /// Default fill color: light gray (0.7, 0.7, 0.7)
    pub fn new(width: f32, height: f32) -> Self {
        Self::with_color(width, height, egui::Color32::from_rgb(178, 178, 178))
    }

    /// Create new progress bar with custom fill color
    pub fn with_color(width: f32, height: f32, fill_color: egui::Color32) -> Self {
        Self {
            current: 0,
            total: 0,
            width,
            height,
            fill_color,
        }
    }

    /// Update progress values
    pub fn set_progress(&mut self, current: usize, total: usize) {
        self.current = current;
        self.total = total;
    }

    /// Render progress bar
    pub fn render(&self, ui: &mut egui::Ui) {
        // Calculate progress percentage
        let progress = if self.total > 0 {
            self.current as f32 / self.total as f32
        } else {
            0.0
        };

        // Reserve space for the progress bar
        let (rect, _response) =
            ui.allocate_exact_size(egui::vec2(self.width, self.height), egui::Sense::hover());

        // Draw background (dark)
        let bg_color = egui::Color32::from_gray(40);
        ui.painter().rect_filled(
            rect, 2.0, // rounding
            bg_color,
        );

        // Draw progress fill
        if progress > 0.0 {
            let fill_width = rect.width() * progress.clamp(0.0, 1.0);
            let fill_rect =
                egui::Rect::from_min_size(rect.min, egui::vec2(fill_width, rect.height()));

            ui.painter().rect_filled(
                fill_rect,
                2.0, // rounding
                self.fill_color,
            );
        }

        // Draw text overlay (percentage or count)
        let text = if self.total > 0 {
            format!("{}/{}", self.current, self.total)
        } else {
            "0/0".to_string()
        };

        let text_color = egui::Color32::from_gray(220);
        let font_id = egui::FontId::monospace(9.0);

        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            text,
            font_id,
            text_color,
        );
    }
}
