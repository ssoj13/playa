use crate::core::cache_man::CacheManager;
use crate::entities::Project;
use crate::entities::frame::{Frame, PixelFormat};
use crate::core::event_bus::BoxedEvent;
use crate::core::player::Player;
use crate::widgets::viewport::ViewportState;
use eframe::egui;
use std::sync::Arc;

/// Status bar component (simplified, no cache progress)
pub struct StatusBar {
    pub current_message: String,
}

impl StatusBar {
    pub fn new() -> Self {
        Self {
            current_message: String::new(),
        }
    }

    /// Read messages from channel and update status (no-op for now)
    pub fn update(&mut self, ctx: &egui::Context) {
        let _ = ctx;
    }

    /// Render status bar at bottom of screen
    pub fn render(
        &self,
        ctx: &egui::Context,
        frame: Option<&Frame>,
        player: &Player,
        project: &Project,
        viewport_state: &ViewportState,
        render_time_ms: f32,
        cache_manager: Option<&Arc<CacheManager>>,
        mut dispatch: impl FnMut(BoxedEvent),
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

                // Render time
                ui.monospace(format!("{:.1}ms", render_time_ms));

                ui.separator();

                // Memory usage
                if let Some(manager) = cache_manager {
                    let (usage, limit) = manager.mem();
                    let usage_mb = usage / 1024 / 1024;
                    let limit_mb = limit / 1024 / 1024;
                    let percent = if limit > 0 {
                        (usage as f64 / limit as f64 * 100.0) as u32
                    } else {
                        0
                    };
                    log::debug!("StatusBar: cache_manager present, usage={}MB, limit={}MB", usage_mb, limit_mb);
                    ui.monospace(format!("Mem: {}/{}MB ({}%)", usage_mb, limit_mb, percent));
                    ui.separator();
                } else {
                    log::warn!("StatusBar: cache_manager is None!");
                }

                // Loop toggle
                let mut loop_enabled = player.loop_enabled();
                if ui.checkbox(&mut loop_enabled, "Loop").changed() {
                    dispatch(Box::new(crate::core::player_events::SetLoopEvent(loop_enabled)));
                }

                ui.separator();

                // FPS info: base/play
                let base_fps = player.fps_base();
                let play_fps = player.fps_play();
                ui.monospace(format!("{:.0}/{:.0} fps", base_fps, play_fps));

                // Comp/Clip range info: <start | play_start <current_frame> play_end | end>
                if let Some(comp_uuid) = player.active_comp() {
                    let media = project.media.read().expect("media lock poisoned");
                    if let Some(comp) = media.get(&comp_uuid) {
                        ui.separator();
                        let start = comp._in();
                        let end = comp._out();
                        let (play_start, play_end) = comp.play_range(true);
                        let current = comp.frame();
                        ui.monospace(format!(
                            "<{} | {} <{}> {} | {}>",
                            start, play_start, current, play_end, end
                        ));
                    }
                }

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
