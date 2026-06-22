use crate::widgets::viewport::ViewportState;
use eframe::egui;
use egui_statusbar::{Section, StatusBar as Bar, StatusBarLayout};
use playa_engine::core::cache_man::CacheManager;
use playa_engine::core::event_bus::BoxedEvent;
use playa_engine::core::player::Player;
use playa_engine::entities::Project;
use playa_engine::entities::frame::{Frame, PixelFormat};
use playa_engine::entities::node::Node;
use std::sync::Arc;

/// Bottom status bar built on the reusable `egui-statusbar` widget: fixed,
/// drag-resizable sections (double-click a splitter to reset) with a flexing
/// tail. `layout` holds the per-section widths and persists across frames.
#[derive(Default)]
pub struct StatusBar {
    pub current_message: String,
    layout: StatusBarLayout,
}

impl StatusBar {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, _ctx: &egui::Context) {}

    /// Render the status bar at the bottom of `ui`. Section content is computed
    /// up front into owned strings so the per-section draw closures stay free of
    /// engine borrows; `egui_statusbar` lays them out with resizable splitters.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        ui: &mut egui::Ui,
        frame: Option<&Frame>,
        player: &Player,
        project: &Project,
        viewport_state: &ViewportState,
        render_time_ms: f32,
        cache_manager: Option<&Arc<CacheManager>>,
        mut dispatch: impl FnMut(BoxedEvent),
    ) {
        // Precompute display strings (decouples the section closures from the
        // engine refs, keeping the borrow checker happy).
        let file_text = frame
            .and_then(Frame::file)
            .and_then(|p| p.file_name().and_then(|n| n.to_str()).map(str::to_string))
            .unwrap_or_else(|| "No file".to_string());

        let res_text = match frame {
            Some(img) => format!("{:>4}x{:<4}", img.width(), img.height()),
            None => "   0x0   ".to_string(),
        };

        let fmt_text = match frame {
            Some(img) => Self::format_pixel_format(img.pixel_format()),
            None => "---",
        };

        let zoom_text = format!("{:>6.1}%", viewport_state.zoom * 100.0);
        let time_text = format!("{:.1}ms", render_time_ms);

        let mem_text = cache_manager.map(|manager| {
            let (usage, limit) = manager.mem();
            let usage_mb = usage / 1024 / 1024;
            let limit_mb = limit / 1024 / 1024;
            let percent = if limit > 0 {
                (usage as f64 / limit as f64 * 100.0) as u32
            } else {
                0
            };
            format!("Mem: {}/{}MB ({}%)", usage_mb, limit_mb, percent)
        });

        let mut loop_enabled = player.loop_enabled();
        let fps_text = format!("{:.0}/{:.0} fps", player.fps_base(), player.fps_play());

        // Comp/clip range: <start | play_start <current> play_end | end>
        let range_text = player.active_comp().and_then(|comp_uuid| {
            let media = project.media.read().unwrap_or_else(|e| e.into_inner());
            media.get(&comp_uuid).map(|comp| {
                let (play_start, play_end) = comp.play_range(true);
                format!(
                    "<{} | {} <{}> {} | {}>",
                    comp._in(),
                    play_start,
                    comp.frame(),
                    play_end,
                    comp._out()
                )
            })
        });

        let msg = self.current_message.clone();

        egui::Panel::bottom("status_bar").show_inside(ui, |ui| {
            let mut sections: Vec<Section> = vec![
                Section::new(170.0, |ui| {
                    ui.monospace(&file_text);
                }),
                Section::new(90.0, |ui| {
                    ui.monospace(&res_text);
                }),
                Section::new(80.0, |ui| {
                    ui.monospace(fmt_text);
                }),
                Section::new(70.0, |ui| {
                    ui.monospace(&zoom_text);
                }),
                Section::new(70.0, |ui| {
                    ui.monospace(&time_text);
                }),
                Section::new(150.0, |ui| {
                    if let Some(t) = &mem_text {
                        ui.monospace(t);
                    }
                }),
                // Flexing tail: loop toggle + fps + range + status message.
                Section::new(0.0, |ui| {
                    if ui.checkbox(&mut loop_enabled, "Loop").changed() {
                        dispatch(Box::new(
                            playa_engine::core::player_events::SetLoopEvent(loop_enabled),
                        ));
                    }
                    ui.separator();
                    ui.monospace(&fps_text);
                    if let Some(r) = &range_text {
                        ui.separator();
                        ui.monospace(r);
                    }
                    if !msg.is_empty() {
                        ui.separator();
                        ui.monospace(&msg);
                    }
                }),
            ];
            Bar::new().show(ui, &mut self.layout, &mut sections);
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
