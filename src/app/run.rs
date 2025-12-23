//! Main application loop - eframe::App implementation.
//!
//! Contains the core update() method that runs each frame:
//! - Event processing
//! - UI rendering (dock panels, dialogs)
//! - Input handling
//! - State persistence

use std::sync::Arc;

use eframe::glow;
use egui_dock::DockArea;
use log::{info, trace};

use crate::app::{DockTab, DockTabs, PlayaApp};
use crate::dialogs::prefs::render_settings_window;

impl eframe::App for PlayaApp {
    /// Main frame update - called every frame by eframe.
    ///
    /// Flow:
    /// 1. Handle API exit request
    /// 2. Start API server (lazy init)
    /// 3. Update compositor backend
    /// 4. Apply theme and font settings
    /// 5. Process player updates and events
    /// 6. Handle dropped files
    /// 7. Render UI (dock panels, dialogs)
    /// 8. Handle keyboard input
    /// 9. Handle screenshots
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // Reset node editor flags each frame - will be set if tab is rendered
        // Handle exit request from REST API
        if self.exit_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        self.node_editor_hovered = false;
        self.node_editor_tab_active = false;

        // Start REST API server if enabled (lazy start on first frame)
        self.start_api_server(ctx);

        // Get GL context and update compositor backend
        if let Some(gl) = frame.gl() {
            self.update_compositor_backend(gl);
        }

        // NOTE: Events processed after player.update() to catch events from player too
        // NOTE: Dirty checking is handled automatically by CompNode::compute()
        // which checks attrs.is_dirty() and recomputes if needed

        // Periodic cache statistics logging (every 10 seconds)
        let current_time = ctx.input(|i| i.time);
        if current_time - self.last_stats_log_time > 10.0 {
            if let Some(ref global_cache) = self.project.global_cache {
                let stats = global_cache.stats();
                let cache_size = global_cache.len();
                log::info!(
                    "Cache stats: {} entries | hits: {} | misses: {} | hit rate: {:.1}%",
                    cache_size,
                    stats.hits(),
                    stats.misses(),
                    stats.hit_rate() * 100.0
                );
            }
            self.last_stats_log_time = current_time;
        }

        // Apply theme based on settings
        if self.settings.dark_mode {
            ctx.set_visuals(egui::Visuals::dark());
        } else {
            ctx.set_visuals(egui::Visuals::light());
        }

        // Apply font size from settings
        let mut style = (*ctx.style()).clone();
        for (_, font_id) in style.text_styles.iter_mut() {
            font_id.size = self.settings.font_size;
        }
        ctx.set_style(style);

        // Apply pending fullscreen changes requested via events
        if self.fullscreen_dirty {
            self.set_cinema_mode(ctx, self.is_fullscreen);
            self.fullscreen_dirty = false;
        }

        // Apply pending settings reset requested via events
        if self.reset_settings_pending {
            self.reset_settings(ctx);
            if self.is_fullscreen {
                self.set_cinema_mode(ctx, false);
            }
            self.reset_settings_pending = false;
        }

        // Enable multipass for better taffy layout recalculation responsiveness
        ctx.options_mut(|opts| {
            opts.max_passes = std::num::NonZeroUsize::new(2).unwrap();
        });

        // Apply memory settings from UI if changed
        let mem_fraction = (self.settings.cache_memory_percent as f64 / 100.0).clamp(0.25, 0.95);
        let reserve_gb = self.settings.reserve_system_memory_gb as f64;

        if (mem_fraction - self.applied_mem_fraction).abs() > f64::EPSILON {
            // Update cache manager with new limits (now lock-free via atomic)
            self.cache_manager.set_memory_limit(mem_fraction, reserve_gb);
            self.applied_mem_fraction = mem_fraction;
        }

        // Unified frame change path: both playback and scrubbing go through SetFrameEvent.
        // Why: single codepath for preload logic, distance-based epoch increment, etc.
        // player.update() returns Some(frame) if frame changed during playback.
        if let Some(new_frame) = self.player.update(&mut self.project) {
            // Emit same event as scrubbing - unified handling in handle_events()
            self.event_bus
                .emit(crate::core::player_events::SetFrameEvent(new_frame));
        }

        // Handle composition events (SetFrameEvent -> triggers frame loading)
        self.handle_events();

        // Update REST API state and handle commands from remote clients
        self.update_api_state();
        self.handle_api_commands();

        // Sync preload delay from settings and check debounced preloader
        self.debounced_preloader.set_delay(self.settings.preload_delay_ms);
        if let Some(_comp_uuid) = self.debounced_preloader.tick() {
            // Delayed preload triggered - load full radius around playhead
            self.enqueue_frame_loads_around_playhead(self.settings.preload_radius);
        }

        // Handle drag-and-drop files/folders - queue for async loading
        ctx.input(|i| {
            let mut dropped: Vec<std::path::PathBuf> = Vec::new();
            for file in &i.raw.dropped_files {
                if let Some(path) = &file.path {
                    dropped.push(path.clone());
                }
            }
            if !dropped.is_empty() {
                info!("Files dropped: {:?}", dropped);
                let _ = self.load_sequences(dropped);
            }
        });

        // Request repaint if:
        // 1. Playing (continuous animation)
        // 2. Cache changed (workers loaded frames, need to update indicators)
        let cache_dirty = self.cache_manager.take_dirty();
        if self.player.is_playing() || cache_dirty || !self.pending_screenshots.is_empty() {
            ctx.request_repaint();
        }

        // Update status messages BEFORE laying out panels
        self.status_bar.update(ctx);

        // Status bar (bottom panel)
        if !self.is_fullscreen {
            let cache_mgr = self.project.cache_manager().map(Arc::clone);
            self.status_bar.render(
                ctx,
                self.frame.as_ref(),
                &self.player,
                &self.project,
                &self.viewport_state,
                self.last_render_time_ms,
                cache_mgr.as_ref(),
                |evt| self.event_bus.emit_boxed(evt),
            );
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.is_fullscreen {
                self.render_viewport_tab(ui);
            } else {
                // Remove hidden tabs before rendering
                self.sync_dock_tabs_visibility();

                let dock_style = egui_dock::Style::from_egui(ctx.style().as_ref());
                let mut dock_state =
                    std::mem::replace(&mut self.dock_state, PlayaApp::default_dock_state());

                // Snapshot dock state before rendering (for change detection)
                let dock_before = serde_json::to_string(&dock_state).ok();

                {
                    let mut tabs = DockTabs { app: self };
                    DockArea::new(&mut dock_state)
                        .style(dock_style)
                        .show_inside(ui, &mut tabs);
                }
                self.dock_state = dock_state;

                // Save split positions after DockArea rendering (only if changed by user)
                self.save_dock_split_positions();

                // Detect dock state changes and update current layout
                let dock_after = serde_json::to_string(&self.dock_state).ok();
                if dock_before != dock_after {
                    self.event_bus
                        .emit(crate::core::layout_events::LayoutUpdatedEvent);
                }
            }
        });

        // Process keyboard input after hover states were updated by panel rendering
        self.handle_keyboard_input(ctx);

        // Settings window (can be shown even in cinema mode)
        if self.show_settings {
            render_settings_window(
                ctx,
                &mut self.show_settings,
                &mut self.settings,
                Some(&self.project),
                Some(&self.event_bus),
            );
        }

        // Encode dialog (can be shown even in cinema mode)
        if self.show_encode_dialog
            && let Some(ref mut dialog) = self.encode_dialog
        {
            let media = self.project.media.read().expect("media lock poisoned");
            let active_comp = self
                .player
                .active_comp()
                .and_then(|uuid| media.get(&uuid))
                .and_then(|node| node.as_comp());
            let should_stay_open = dialog.render(ctx, &self.project, active_comp);

            // Save dialog state (on every render - cheap clone)
            self.settings.encode_dialog = dialog.save_to_settings();

            if !should_stay_open {
                trace!("Encode dialog closed, settings saved to AppSettings");
                self.show_encode_dialog = false;
            }
        }

        // Apply settings that affect runtime infrastructure/state.
        // This must not depend on "Settings window opened".
        self.apply_cache_strategy_if_changed();

        // Handle pending screenshots (glReadPixels after all rendering)
        self.handle_pending_screenshots(ctx);
    }

    /// Save app state to persistent storage.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        // Gather all settings from components
        self.settings.fps_base = self.player.fps_base();
        self.settings.loop_enabled = self.player.loop_enabled();
        self.settings.current_shader = self.shader_manager.current_shader.clone();
        self.settings.show_help = self.show_help;
        self.settings.show_playlist = self.show_playlist;
        self.settings.show_attributes_editor = self.show_attributes_editor;

        // Serialize and save app settings
        if let Ok(json) = serde_json::to_string(self) {
            storage.set_string(eframe::APP_KEY, json);
            trace!(
                "App state saved: FPS={}, Loop={}, Shader={}",
                self.settings.fps_base,
                self.settings.loop_enabled,
                self.settings.current_shader
            );
        }
    }

    /// Cleanup on application exit.
    fn on_exit(&mut self, gl: Option<&glow::Context>) {
        // Cancel all pending frame loads by incrementing epoch
        // Workers check epoch before executing, so stale tasks will be skipped
        self.cache_manager.increment_epoch();
        self.debounced_preloader.cancel();
        trace!("Cancelled pending frame loads for fast shutdown");

        // Cleanup OpenGL resources
        if let Some(gl) = gl {
            let mut renderer = self.viewport_renderer.lock().unwrap();
            renderer.destroy(gl);
            trace!("ViewportRenderer resources cleaned up");
        }
    }
}

impl PlayaApp {
    /// Toggle cinema/fullscreen mode.
    pub fn set_cinema_mode(&mut self, ctx: &egui::Context, enabled: bool) {
        self.is_fullscreen = enabled;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(enabled));
        // Hide window decorations in cinema mode for a cleaner look
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(!enabled));
        // Request repaint to immediately reflect UI visibility/background changes
        ctx.request_repaint();
    }

    /// Reset all settings to defaults.
    pub fn reset_settings(&mut self, ctx: &egui::Context) {
        info!("Resetting settings to default");
        self.settings = crate::dialogs::prefs::AppSettings::default();
        self.player.reset_settings();
        self.viewport_state = crate::widgets::viewport::ViewportState::new();
        self.shader_manager.reset_settings();

        // Reset window size
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(1280.0, 720.0)));

        // Re-apply image-dependent viewport settings if an image is loaded
        if let Some(frame) = &self.frame {
            let (width, height) = frame.resolution();
            self.viewport_state
                .set_image_size(egui::vec2(width as f32, height as f32));
            self.viewport_state.set_mode_fit();
        }
    }

    /// Update compositor backend based on settings (CPU vs GPU).
    pub fn update_compositor_backend(&mut self, gl: &std::sync::Arc<glow::Context>) {
        use crate::entities::compositor::{CompositorType, CpuCompositor};
        use crate::entities::gpu_compositor::GpuCompositor;

        // Check current backend type first (cheap)
        let current_is_cpu = matches!(
            *self.project.compositor.lock().unwrap_or_else(|e| e.into_inner()),
            CompositorType::Cpu(_)
        );
        let desired_is_cpu = matches!(
            self.settings.compositor_backend,
            crate::dialogs::prefs::CompositorBackend::Cpu
        );

        // Only create new compositor if we need to switch
        if current_is_cpu != desired_is_cpu {
            info!(
                "Switching compositor to: {:?}",
                self.settings.compositor_backend
            );
            let new_backend = match self.settings.compositor_backend {
                crate::dialogs::prefs::CompositorBackend::Cpu => CompositorType::Cpu(CpuCompositor),
                crate::dialogs::prefs::CompositorBackend::Gpu => {
                    CompositorType::Gpu(GpuCompositor::new(gl.clone()))
                }
            };
            self.project.set_compositor(new_backend);
        }
    }

    /// Apply cache strategy changes from settings.
    pub fn apply_cache_strategy_if_changed(&mut self) {
        let desired = self.settings.cache_strategy;
        if desired == self.applied_cache_strategy {
            return;
        }

        log::info!("Cache strategy changed to: {:?}", desired);
        if let Some(ref global_cache) = self.project.global_cache {
            global_cache.set_strategy(desired);
        }
        self.applied_cache_strategy = desired;
    }

    /// Handle pending screenshot requests via glReadPixels.
    /// Broadcasts one capture to all waiting clients.
    fn handle_pending_screenshots(&mut self, ctx: &egui::Context) {
        if self.pending_screenshots.is_empty() {
            return;
        }

        // Drain all waiters: (viewport_only, sender)
        let all_waiters: Vec<_> = std::mem::take(&mut self.pending_screenshots);

        // Split by type
        let mut window_waiters: Vec<crossbeam_channel::Sender<Result<Vec<u8>, String>>> =
            Vec::new();
        let mut frame_waiters: Vec<crossbeam_channel::Sender<Result<Vec<u8>, String>>> = Vec::new();
        for (viewport_only, sender) in all_waiters {
            if viewport_only {
                window_waiters.push(sender);
            } else {
                frame_waiters.push(sender);
            }
        }

        // Pre-capture raw frame data for frame_waiters (before callback, on main thread)
        let frame_result: Option<Result<Vec<u8>, String>> = if !frame_waiters.is_empty() {
            Some(self.capture_raw_frame())
        } else {
            None
        };

        // Get window size for glReadPixels
        let screen_rect = ctx.input(|i| i.viewport_rect());
        let width = screen_rect.width() as i32;
        let height = screen_rect.height() as i32;
        log::info!(
            "Screenshot: {} window + {} frame waiters, {}x{}",
            window_waiters.len(),
            frame_waiters.len(),
            width,
            height
        );

        // Wrap both sets of waiters for callback access
        type WaiterList = Vec<crossbeam_channel::Sender<Result<Vec<u8>, String>>>;
        let holder: Arc<
            std::sync::Mutex<(WaiterList, WaiterList, Option<Result<Vec<u8>, String>>)>,
        > = Arc::new(std::sync::Mutex::new((
            window_waiters,
            frame_waiters,
            frame_result,
        )));
        let holder_clone = Arc::clone(&holder);

        // Add paint callback via layer_painter
        let layer_id =
            egui::LayerId::new(egui::Order::Foreground, egui::Id::new("screenshot_capture"));
        let painter = ctx.layer_painter(layer_id);
        painter.add(egui::PaintCallback {
            rect: screen_rect,
            callback: Arc::new(egui_glow::CallbackFn::new(move |_info, gl_painter| {
                use eframe::glow::HasContext;
                let gl = gl_painter.gl();

                // Take all data (only first callback execution processes)
                let (window_waiters, frame_waiters, frame_result) =
                    std::mem::take(&mut *holder_clone.lock().unwrap());
                if window_waiters.is_empty() && frame_waiters.is_empty() {
                    return;
                }

                log::info!(
                    "PaintCallback: {} window + {} frame waiters",
                    window_waiters.len(),
                    frame_waiters.len()
                );

                // Send pre-captured frame data to frame waiters
                if let Some(result) = frame_result {
                    for waiter in frame_waiters {
                        let _ = waiter.send(result.clone());
                    }
                }

                // Capture full window for window waiters
                if !window_waiters.is_empty() {
                    // Allocate buffer for pixels (RGBA)
                    let mut pixels = vec![0u8; (width * height * 4) as usize];

                    unsafe {
                        gl.read_pixels(
                            0,
                            0,
                            width,
                            height,
                            eframe::glow::RGBA,
                            eframe::glow::UNSIGNED_BYTE,
                            eframe::glow::PixelPackData::Slice(Some(&mut pixels)),
                        );
                    }

                    // Flip vertically (OpenGL origin is bottom-left) - fast row swap
                    let row_size = (width * 4) as usize;
                    let half_height = height as usize / 2;
                    for y in 0..half_height {
                        let top_start = y * row_size;
                        let bottom_start = ((height as usize) - 1 - y) * row_size;
                        let (top_slice, rest) = pixels.split_at_mut(top_start + row_size);
                        let top_row = &mut top_slice[top_start..];
                        let bottom_row =
                            &mut rest[bottom_start - top_start - row_size..][..row_size];
                        top_row.swap_with_slice(bottom_row);
                    }

                    // Encode to JPEG
                    use image::{ImageBuffer, Rgba};
                    let result = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(
                        width as u32,
                        height as u32,
                        pixels,
                    )
                    .ok_or_else(|| "Failed to create image buffer".to_string())
                    .and_then(|img| {
                        let mut jpeg_bytes: Vec<u8> = Vec::new();
                        let mut cursor = std::io::Cursor::new(&mut jpeg_bytes);
                        let rgb_img = image::DynamicImage::ImageRgba8(img).to_rgb8();
                        let mut encoder =
                            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, 90);
                        encoder
                            .encode(
                                rgb_img.as_raw(),
                                rgb_img.width(),
                                rgb_img.height(),
                                image::ExtendedColorType::Rgb8,
                            )
                            .map_err(|e| format!("JPEG encoding failed: {}", e))?;
                        log::info!(
                            "Window screenshot: {}x{}, {} bytes",
                            width,
                            height,
                            jpeg_bytes.len()
                        );
                        Ok(jpeg_bytes)
                    });

                    // Broadcast to window waiters
                    for waiter in window_waiters {
                        let _ = waiter.send(result.clone());
                    }
                }
            })),
        });
        let _ = holder; // Keep alive until callback runs
    }
}
