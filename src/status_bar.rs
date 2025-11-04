use eframe::egui;
use std::sync::mpsc;
use crate::cache::CacheMessage;
use crate::frame::{Frame, PixelFormat};
use crate::player::Player;
use crate::progress_bar::ProgressBar;
use crate::sequence::Sequence;
use crate::viewport::ViewportState;

/// Status bar component that receives updates from cache via messages
pub struct StatusBar {
    message_rx: mpsc::Receiver<CacheMessage>,
    pub current_message: String,
    cached_count: usize,
    total_count: usize,
    progress_bar: ProgressBar,
}

impl StatusBar {
    pub fn new(rx: mpsc::Receiver<CacheMessage>) -> Self {
        Self {
            message_rx: rx,
            current_message: String::new(),
            cached_count: 0,
            total_count: 0,
            progress_bar: ProgressBar::new(150.0, 10.0), // 150px wide, 10px tall
        }
    }

    /// Read messages from channel and update status
    /// Returns detected sequences that should be added to cache
    pub fn update(&mut self, ctx: &egui::Context) -> Vec<Sequence> {
        let mut has_updates = false;
        let mut detected_sequences = Vec::new();

        while let Ok(msg) = self.message_rx.try_recv() {
            match msg {
                CacheMessage::SequenceDetected(seq) => {
                    detected_sequences.push(seq);
                }
                CacheMessage::StatusMessage(s) => {
                    self.current_message = s;
                }
                CacheMessage::LoadProgress { cached_count, total_count } => {
                    self.cached_count = cached_count;
                    self.total_count = total_count;
                    self.progress_bar.set_progress(cached_count, total_count);
                }
                CacheMessage::FrameLoaded => {
                    // Individual frame loaded - will be followed by LoadProgress
                }
            }
            has_updates = true;
        }

        if has_updates {
            ctx.request_repaint();
        }

        detected_sequences
    }

    /// Render status bar at bottom of screen
    pub fn render(
        &self,
        ctx: &egui::Context,
        frame: Option<&Frame>,
        player: &Player,
        viewport_state: &ViewportState,
        render_time_ms: f32,
    ) {
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                // Filename
                if let Some(frame) = frame {
                    if let Some(path) = frame.file() {
                        if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                            ui.monospace(filename);
                        } else {
                            ui.monospace("---");
                        }
                    } else {
                        ui.monospace("No file");
                    }
                } else {
                    ui.monospace("No file");
                }

                ui.separator();

                // Resolution
                if let Some(img) = frame {
                    ui.monospace(format!("{:>4}x{:<4}", img.width(), img.height()));
                } else {
                    ui.monospace("   0x0   ");
                }

                ui.separator();

                // Pixel format
                if let Some(img) = frame {
                    ui.monospace(Self::format_pixel_format(img.pixel_format()));
                } else {
                    ui.monospace("---");
                }

                ui.separator();

                // Zoom
                ui.monospace(format!("{:>6.1}%", viewport_state.zoom * 100.0));

                ui.separator();

                // FPS
                ui.monospace(format!("{:>5.1}", player.fps));

                ui.separator();

                // Memory usage
                let (used_bytes, max_bytes) = player.cache.mem();
                let used_mb = used_bytes / 1024 / 1024;
                let max_mb = max_bytes / 1024 / 1024;
                ui.monospace(format!("{}M/{}M", used_mb, max_mb));

                ui.separator();

                // Progress bar for loading
                self.progress_bar.render(ui);

                ui.separator();

                // Render time
                ui.monospace(format!("{:.1}ms", render_time_ms));

                // Status message (if any)
                if !self.current_message.is_empty() {
                    ui.separator();
                    ui.monospace(&self.current_message);
                }
            });
        });
    }

    /// Format pixel format for display
    fn format_pixel_format(format: PixelFormat) -> &'static str {
        match format {
            PixelFormat::Rgba8 => "RGBA u8",
            PixelFormat::RgbaF16 => "RGBA f16",
            PixelFormat::RgbaF32 => "RGBA f32",
        }
    }
}
