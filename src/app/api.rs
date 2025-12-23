//! REST API handling for PlayaApp.
//!
//! Contains methods for:
//! - Starting the API server (start_api_server)
//! - Updating API state snapshot (update_api_state)
//! - Handling API commands (handle_api_commands)
//! - Screenshot capture (take_screenshot, capture_raw_frame)

use super::PlayaApp;
use crate::core::player_events::*;
use crate::entities::frame::{PixelBuffer, TonemapMode};
use crate::entities::node::Node;
use crate::server::ApiCommand;

use eframe::egui;
use image::{ImageBuffer, Rgba};

impl PlayaApp {
    /// Start REST API server if enabled in settings.
    pub fn start_api_server(&mut self, ctx: &egui::Context) {
        if self.api_command_rx.is_some() {
            return; // Already started
        }

        // Store egui context for API thread to trigger repaints
        *self.api_state.egui_ctx.write().unwrap() = Some(ctx.clone());

        let port = self.settings.api_server_port.unwrap_or(9876);
        if self.settings.api_server_enabled {
            log::info!("Starting REST API server on port {}", port);
            let rx = crate::server::ApiServer::start(port, self.api_state.clone());
            self.api_command_rx = Some(rx);
        }
    }

    /// Update API state snapshot for remote clients.
    pub fn update_api_state(&mut self) {
        // Update player snapshot
        {
            let mut player = self.api_state.player.write().unwrap();
            player.frame = self.player.current_frame(&self.project);
            player.fps = self.player.fps_play();
            player.playing = self.player.is_playing();
            player.loop_enabled = self.player.loop_enabled();
            player.active_comp = self.player.active_comp();
        }

        // Update comp snapshot
        {
            let mut comp = self.api_state.comp.write().unwrap();
            *comp = self.player.active_comp().and_then(|uuid| {
                self.project.with_comp(uuid, |c| crate::server::CompSnapshot {
                    uuid,
                    name: c.name().to_string(),
                    width: c.dim().0 as u32,
                    height: c.dim().1 as u32,
                    duration: c.frame_count(),
                    in_frame: c._in(),
                    out_frame: c._out(),
                })
            });
        }

        // Update cache snapshot
        {
            let mut cache = self.api_state.cache.write().unwrap();
            let (used, limit) = self.cache_manager.mem();
            cache.memory_used_mb = used as f32 / (1024.0 * 1024.0);
            cache.memory_limit_mb = limit as f32 / (1024.0 * 1024.0);
        }
    }

    /// Handle commands from REST API.
    pub fn handle_api_commands(&mut self) {
        // Collect all pending commands first (avoids borrow issues)
        let commands: Vec<ApiCommand> = if let Some(ref rx) = self.api_command_rx {
            let mut cmds = Vec::new();
            while let Ok(cmd) = rx.try_recv() {
                cmds.push(cmd);
            }
            cmds
        } else {
            return;
        };

        // Process collected commands
        for cmd in commands {
            log::trace!("API command: {:?}", cmd);
            match cmd {
                ApiCommand::Play => {
                    self.event_bus.emit(TogglePlayPauseEvent);
                    if !self.player.is_playing() {
                        self.event_bus.emit(TogglePlayPauseEvent);
                    }
                }
                ApiCommand::Pause => {
                    if self.player.is_playing() {
                        self.event_bus.emit(TogglePlayPauseEvent);
                    }
                }
                ApiCommand::Stop => {
                    self.event_bus.emit(StopEvent);
                }
                ApiCommand::SetFrame(frame) => {
                    self.event_bus.emit(SetFrameEvent(frame));
                }
                ApiCommand::SetFps(fps) => {
                    self.player.set_fps_base(fps);
                }
                ApiCommand::ToggleLoop => {
                    self.event_bus.emit(ToggleLoopEvent);
                }
                ApiCommand::LoadSequence(path) => {
                    let _ = self.load_sequences(vec![std::path::PathBuf::from(path)]);
                }
                ApiCommand::EmitEvent { event_type, payload } => {
                    // Dispatch common events by name
                    match event_type.as_str() {
                        "TogglePlayPause" => self.event_bus.emit(TogglePlayPauseEvent),
                        "Stop" => self.event_bus.emit(StopEvent),
                        "JumpToStart" => self.event_bus.emit(JumpToStartEvent),
                        "JumpToEnd" => self.event_bus.emit(JumpToEndEvent),
                        "StepForward" => self.event_bus.emit(StepForwardEvent),
                        "StepBackward" => self.event_bus.emit(StepBackwardEvent),
                        "ToggleLoop" => self.event_bus.emit(ToggleLoopEvent),
                        _ => {
                            log::warn!("Unknown event type: {} (payload: {})", event_type, payload);
                        }
                    }
                }
                ApiCommand::Screenshot { viewport_only, response } => {
                    self.take_screenshot(viewport_only, response);
                }
                ApiCommand::Exit => {
                    log::info!("Exit command received via REST API");
                    self.exit_requested = true;
                }
                ApiCommand::NextFrame => {
                    self.event_bus.emit(StepForwardEvent);
                }
                ApiCommand::PrevFrame => {
                    self.event_bus.emit(StepBackwardEvent);
                }
            }
        }
    }

    /// Queue screenshot request.
    /// viewport_only=true: full window via glReadPixels (includes UI)
    /// viewport_only=false: raw frame data only (no UI)
    /// Both go through paint callback for unified async handling.
    pub fn take_screenshot(&mut self, viewport_only: bool, response: crossbeam_channel::Sender<Result<Vec<u8>, String>>) {
        self.pending_screenshots.push((viewport_only, response));
        log::trace!("Screenshot request queued ({} waiting), viewport_only={}", self.pending_screenshots.len(), viewport_only);
    }

    /// Capture raw frame data (no GL, immediate).
    pub fn capture_raw_frame(&self) -> Result<Vec<u8>, String> {
        let frame = match &self.frame {
            Some(f) => f,
            None => return Err("No frame loaded".to_string()),
        };

        let (width, height) = frame.resolution();
        let buffer = frame.buffer();

        // Convert to RGBA8 if needed (tonemap HDR)
        let rgba_data: Vec<u8> = match buffer.as_ref() {
            PixelBuffer::U8(data) => data.clone(),
            PixelBuffer::F16(_) | PixelBuffer::F32(_) => {
                let tonemapped = frame.tonemap(TonemapMode::ACES)
                    .map_err(|e| format!("Tonemap failed: {}", e))?;
                match tonemapped.buffer().as_ref() {
                    PixelBuffer::U8(data) => data.clone(),
                    _ => return Err("Tonemap did not produce U8 buffer".to_string()),
                }
            }
        };

        let img: ImageBuffer<Rgba<u8>, Vec<u8>> = 
            ImageBuffer::from_raw(width as u32, height as u32, rgba_data)
                .ok_or_else(|| "Failed to create image buffer".to_string())?;

        // JPEG is much faster than PNG
        let mut jpeg_bytes: Vec<u8> = Vec::new();
        let rgb_img = image::DynamicImage::ImageRgba8(img).to_rgb8();
        let mut cursor = std::io::Cursor::new(&mut jpeg_bytes);
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, 90);
        encoder.encode(
            rgb_img.as_raw(),
            rgb_img.width(),
            rgb_img.height(),
            image::ExtendedColorType::Rgb8
        ).map_err(|e| format!("JPEG encoding failed: {}", e))?;

        log::info!("Raw frame screenshot: {}x{}, {} bytes", width, height, jpeg_bytes.len());
        Ok(jpeg_bytes)
    }
}
