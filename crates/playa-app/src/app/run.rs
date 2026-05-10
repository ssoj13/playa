//! Main application loop - eframe::App implementation.
//!
//! Contains the core update() method that runs each frame:
//! - Event processing
//! - UI rendering (dock panels, dialogs)
//! - Input handling
//! - State persistence

use std::sync::Arc;

use eframe::egui;
use egui_dock::DockArea;
use log::{info, trace};

use crate::app::api::WindowScreenshotWaiters;
use crate::app::{DockTabs, PlayaApp};
use playa_ui::dialogs::prefs::render_settings_window;

impl eframe::App for PlayaApp {
    /// Main frame update - called every frame by eframe.
    ///
    /// Each frame: API exit, lazy API start, wgpu output format + compositor sync, screenshot
    /// deliveries, GPU blend drain, settings/player/event loop, docked UI & dialogs; ends by
    /// scheduling any new screenshots for the compositor-backed capture path.
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

        if let Some(rs) = frame.wgpu_render_state() {
            self.viewport_renderer
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .set_output_format(rs.target_format);
            self.update_compositor_backend(&rs.device, &rs.queue);
        }

        self.consume_egui_screenshots(ctx);
        self.drain_gpu_blend_queue(ctx);

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

        // Apply theme based on settings - skip if unchanged
        if self.last_applied_dark_mode != Some(self.settings.dark_mode) {
            if self.settings.dark_mode {
                ctx.set_visuals(egui::Visuals::dark());
            } else {
                ctx.set_visuals(egui::Visuals::light());
            }
            self.last_applied_dark_mode = Some(self.settings.dark_mode);
        }

        // Apply font size from settings - skip if unchanged (cloning style is expensive)
        if (self.settings.font_size - self.last_applied_font_size).abs() > f32::EPSILON {
            let mut style = (*ctx.style()).clone();
            for (_, font_id) in style.text_styles.iter_mut() {
                font_id.size = self.settings.font_size;
            }
            ctx.set_style(style);
            self.last_applied_font_size = self.settings.font_size;
        }

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

        // Enable multipass for better taffy layout recalculation responsiveness (one-time init)
        if !self.options_initialized {
            ctx.options_mut(|opts| {
                opts.max_passes = std::num::NonZeroUsize::new(2).unwrap();
            });
            self.options_initialized = true;
        }

        // Apply memory settings from UI if changed
        let mem_fraction = (self.settings.cache.cache_memory_percent as f64 / 100.0).clamp(0.25, 0.95);
        let reserve_gb = self.settings.cache.reserve_system_memory_gb as f64;

        if (mem_fraction - self.applied_mem_fraction).abs() > f64::EPSILON {
            // Update cache manager with new limits (now lock-free via atomic)
            self.cache_manager
                .set_memory_limit(mem_fraction, reserve_gb);
            self.applied_mem_fraction = mem_fraction;
        }

        // Unified frame change path: both playback and scrubbing go through SetFrameEvent.
        // Why: single codepath for preload logic, distance-based epoch increment, etc.
        // player.update() returns Some(frame) if frame changed during playback.
        if let Some(new_frame) = self.player.update(&mut self.project) {
            // Emit same event as scrubbing - unified handling in handle_events()
            self.event_bus
                .emit(playa_engine::core::player_events::SetFrameEvent(new_frame));
        }

        // Handle composition events (SetFrameEvent -> triggers frame loading)
        self.handle_events();

        // Update REST API state and handle commands from remote clients
        self.update_api_state();
        self.handle_api_commands();

        // Sync preload delay from settings and check debounced preloader
        self.debounced_preloader
            .set_delay(self.settings.playback.preload_delay_ms);
        if let Some(_comp_uuid) = self.debounced_preloader.tick() {
            // Delayed preload triggered - load full radius around playhead
            self.enqueue_frame_loads_around_playhead(self.settings.playback.preload_radius);
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

        // Snapped ghost rect is rewritten only when this frame's timeline draw runs above a valid
        // drop strip; clearing here avoids stale screen-space rects when the timeline tab is hidden.
        ctx.data_mut(|data| {
            data.remove::<playa_ui::widgets::dnd::ProjectDragSnapOverlay>(
                playa_ui::widgets::dnd::project_drag_snap_overlay_id(),
            );
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.is_fullscreen {
                self.render_viewport_tab(ui);
            } else {
                // Remove hidden tabs before rendering
                self.sync_dock_tabs_visibility();

                let dock_style = egui_dock::Style::from_egui(ctx.style().as_ref());
                let mut dock_state =
                    std::mem::replace(&mut self.dock_state, PlayaApp::default_dock_state());

                {
                    let mut tabs = DockTabs { app: self };
                    DockArea::new(&mut dock_state)
                        .style(dock_style)
                        .show_inside(ui, &mut tabs);
                }
                self.dock_state = dock_state;

                // Save split positions after DockArea rendering (only if changed by user)
                self.save_dock_split_positions();

                // Emit LayoutUpdatedEvent only when the pointer is released - dock layout
                // changes (tab drags, resizes) commit on pointer release, so this is the
                // correct and cheap signal instead of serializing dock state twice per frame.
                let pointer_released = ui.input(|i| i.pointer.any_released());
                if pointer_released {
                    self.event_bus
                        .emit(playa_engine::core::layout_events::LayoutUpdatedEvent);
                }
            }
        });

        playa_ui::widgets::dnd::paint_global_project_drag_overlay(ctx);

        // Sync the persisted budget cap into the queue. Cheap atomic
        // write per frame — the queue checks it pre-insert in submit().
        // None when disabled, Some(cap) when enabled.
        #[cfg(feature = "jobs")]
        if let Some(queue) = self.job_queue.as_ref() {
            let cap = if self.settings.jobs.daily_budget_enabled {
                Some(self.settings.jobs.daily_budget_usd)
            } else {
                None
            };
            queue.set_budget_cap(cap);
        }

        // Mirror auto_attach_mp4 into the atomic the JobEvent::Completed
        // listener reads. Toggling the preference takes effect for jobs
        // that complete on or after the next frame.
        #[cfg(feature = "jobs")]
        self.auto_attach_enabled.store(
            self.settings.jobs.auto_attach_mp4,
            std::sync::atomic::Ordering::Relaxed,
        );

        // Drain any mp4 paths the auto-attach listener queued from worker
        // threads. Routes through `load_sequences`, the same import path
        // drag-drop uses.
        #[cfg(feature = "jobs")]
        self.drain_auto_attach_queue();

        // Render the Submit dialog modal (jobs subsystem). Sits on top of the
        // dock so the form covers the panel that opened it. The dialog state
        // machine handles Submit/Cancel internally; on Submit we route to the
        // job queue here.
        #[cfg(feature = "jobs")]
        {
            use playa_jobs::ui::SubmitDialogResult;
            // Handle snapshot button before show() so the dialog can
            // display the filled image_url on the same frame the user
            // clicked the button. capture_raw_frame walks the active
            // comp's decoded frame (NOT the viewport — no UI chrome).
            if self.submit_dialog.snapshot_requested {
                self.submit_dialog.snapshot_requested = false;
                match self.snapshot_current_frame() {
                    Ok(snap) => {
                        log::info!(
                            "Snapshot ready: {} ({} bytes data URL)",
                            snap.path.display(),
                            snap.data_url.len()
                        );
                        self.submit_dialog.image_url = snap.data_url;
                    }
                    Err(e) => log::warn!("Snapshot failed: {e}"),
                }
            }
            match self.submit_dialog.show(ctx) {
                SubmitDialogResult::Submit {
                    kind,
                    params_batch,
                    auto_attach: _,
                } => {
                    if let Some(queue) = self.job_queue.as_ref() {
                        let n = params_batch.len();
                        for (i, params) in params_batch.into_iter().enumerate() {
                            match queue.submit(kind, params) {
                                Ok(id) => log::info!(
                                    "Submitted job {id} (kind={kind}, {}/{n})",
                                    i + 1
                                ),
                                Err(e) => log::warn!("Submit {} failed: {e}", i + 1),
                            }
                        }
                    } else {
                        log::warn!(
                            "Submit dialog accepted but JobQueue is not initialized — request dropped"
                        );
                    }
                }
                SubmitDialogResult::Cancelled => {}
                SubmitDialogResult::None => {}
            }
        }

        // Ctrl+Comma — open the new pluggable preferences modal. Direct
        // input check (rather than going through the legacy event-factory
        // hotkey handler) keeps the wiring local to this app and avoids
        // adding an `OpenPrefsWindowEvent` type for a one-line UX hook.
        // The legacy F12 → ToggleSettingsEvent path still drives the old
        // settings window; the two coexist while migration unfolds.
        let open_prefs_via_hotkey = ctx.input(|i| {
            i.modifiers.ctrl
                && i.events.iter().any(|e| matches!(
                    e,
                    egui::Event::Key {
                        key: egui::Key::Comma,
                        pressed: true,
                        ..
                    }
                ))
        });
        if open_prefs_via_hotkey && !self.prefs_window.is_open() {
            self.prefs_window.open_with(&self.settings);
        }

        match self
            .prefs_window
            .show(ctx, &mut self.prefs_registry, &mut self.settings)
        {
            playa_prefs::PrefsResult::Applied => log::info!("Preferences applied"),
            playa_prefs::PrefsResult::OkClosed => log::info!("Preferences applied (OK)"),
            playa_prefs::PrefsResult::Cancelled => log::info!("Preferences cancelled"),
            playa_prefs::PrefsResult::Open | playa_prefs::PrefsResult::Closed => {}
        }

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

        // Handle queued screenshot requests after UI + egui primitives are finalized for this tick.
        self.handle_pending_screenshots(ctx);
    }

    /// Save app state to persistent storage.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        // Gather all settings from components
        self.settings.playback.fps_base = self.player.fps_base();
        self.settings.playback.loop_enabled = self.player.loop_enabled();
        self.settings.current_shader = self.shader_manager.current_shader.clone();
        self.settings.show_help = self.show_help;
        self.settings.show_playlist = self.show_playlist;
        self.settings.show_attributes_editor = self.show_attributes_editor;

        // Serialize and save app settings
        if let Ok(json) = serde_json::to_string(self) {
            storage.set_string(eframe::APP_KEY, json);
            trace!(
                "App state saved: FPS={}, Loop={}, Shader={}",
                self.settings.playback.fps_base, self.settings.playback.loop_enabled, self.settings.current_shader
            );
        }
    }

    /// Cleanup on application exit.
    fn on_exit(&mut self) {
        // Cancel all pending frame loads by incrementing epoch
        // Workers check epoch before executing, so stale tasks will be skipped
        self.cache_manager.increment_epoch();
        self.debounced_preloader.cancel();
        trace!("Cancelled pending frame loads for fast shutdown");

        // Release worker threads parked inside
        // playa_engine::entities::GpuBlendBridge::delegate_blend_blocking, then flush any
        // queued blend requests so their reply channels close cleanly. Without this,
        // Workers::drop's thread join hangs on a worker waiting for a reply that the UI
        // thread is no longer going to deliver.
        if let Some(bridge) = &self.gpu_blend_bridge {
            bridge.shutdown();
        }
        {
            let rx_guard = self
                .gpu_blend_rx
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let Some(rx) = rx_guard.as_ref() {
                let mut comp = self
                    .project
                    .compositor
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                let _ = playa_engine::entities::GpuBlendBridge::drain_into_compositor(rx, &mut comp);
            }
        }

        let mut renderer = self.viewport_renderer.lock().unwrap_or_else(|e| e.into_inner());
        renderer.destroy();
        trace!("ViewportRenderer GPU resources cleaned up");
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
        self.settings = playa_ui::dialogs::prefs::AppSettings::default();
        self.player.reset_settings();
        self.viewport_state = playa_ui::widgets::viewport::ViewportState::new();
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

    /// Update compositor backend based on settings (CPU vs wgpu offload path).
    pub fn update_compositor_backend(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        use playa_engine::entities::compositor::{CompositorType, CpuCompositor};
        use playa_engine::render_gpu::WgpuCompositor;

        let current_is_cpu = matches!(
            *self
                .project
                .compositor
                .lock()
                .unwrap_or_else(|e| e.into_inner()),
            CompositorType::Cpu(_)
        );
        let desired_is_cpu = matches!(
            self.settings.compositor_backend,
            playa_events::CompositorBackend::Cpu
        );

        if current_is_cpu != desired_is_cpu {
            info!(
                "Switching compositor to: {:?}",
                self.settings.compositor_backend
            );
            let new_backend = match self.settings.compositor_backend {
                playa_events::CompositorBackend::Cpu => CompositorType::Cpu(CpuCompositor),
                playa_events::CompositorBackend::Gpu => {
                    CompositorType::Wgpu(WgpuCompositor::new(device, queue))
                }
            };
            self.project.set_compositor(new_backend);
        }
    }

    /// Apply cache strategy changes from settings.
    pub fn apply_cache_strategy_if_changed(&mut self) {
        let desired = self.settings.cache.cache_strategy;
        if desired == self.applied_cache_strategy {
            return;
        }

        log::info!("Cache strategy changed to: {:?}", desired);
        if let Some(ref global_cache) = self.project.global_cache {
            global_cache.set_strategy(desired);
        }
        self.applied_cache_strategy = desired;
    }

    /// Full-window grabs use [`egui::ViewportCommand::Screenshot`] (decoded in
    /// [`PlayaApp::consume_egui_screenshots`]); raw-pixel grabs stay CPU-only (`capture_raw_frame`).
    fn handle_pending_screenshots(&mut self, ctx: &egui::Context) {
        if self.pending_screenshots.is_empty() {
            return;
        }

        let all_waiters: Vec<_> = std::mem::take(&mut self.pending_screenshots);

        let mut window_waiters = Vec::new();
        let mut frame_waiters = Vec::new();
        for (viewport_only, sender) in all_waiters {
            if viewport_only {
                window_waiters.push(sender);
            } else {
                frame_waiters.push(sender);
            }
        }

        log::info!(
            "Screenshot: {} window + {} frame waiters",
            window_waiters.len(),
            frame_waiters.len()
        );

        let frame_result = if frame_waiters.is_empty() {
            None
        } else {
            Some(self.capture_raw_frame())
        };
        if let Some(ref result) = frame_result {
            for waiter in frame_waiters {
                let _ = waiter.send(result.clone());
            }
        }

        if !window_waiters.is_empty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
                WindowScreenshotWaiters(window_waiters),
            )));
            ctx.request_repaint();
        }
    }
}
