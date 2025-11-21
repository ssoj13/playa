//! Encoding dialog UI
//!
//! Provides dialog for configuring and running video encoding.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::thread::JoinHandle;

use eframe::egui;
use log::info;

use crate::dialogs::encode::{
    CodecSettings, Container, EncodeError, EncodeProgress, EncodeStage, EncoderSettings,
    ProResProfile, VideoCodec,
};
use crate::widgets::status::progress_bar::ProgressBar;

/// Encoding dialog state
pub struct EncodeDialog {
    /// Output path and container settings
    pub output_path: PathBuf,
    pub container: Container,
    pub fps: f32,

    /// Currently selected codec tab
    pub selected_codec: VideoCodec,

    /// Per-codec settings
    pub codec_settings: CodecSettings,

    /// Whether encoding is currently in progress
    pub is_encoding: bool,

    /// Current encoding progress (if encoding)
    pub progress: Option<EncodeProgress>,

    /// Cancel flag shared with encoder thread
    pub cancel_flag: Arc<AtomicBool>,

    /// Channel receiver for progress updates
    progress_rx: Option<Receiver<EncodeProgress>>,

    /// Encoder thread handle
    encode_thread: Option<JoinHandle<Result<(), EncodeError>>>,

    /// Progress bar widget
    progress_bar: ProgressBar,

    /// Last encoder name (for display)
    encoder_name: String,

    /// Tonemapping mode for HDR→LDR conversion
    pub tonemap_mode: crate::entities::frame::TonemapMode,
}

impl EncodeDialog {
    /// Increment the last number in filename
    /// Examples: aaa001.mp4 -> aaa002.mp4, test999.mp4 -> test1000.mp4
    fn increment_filename(&mut self) {
        let file_stem = self.output_path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("output");

        let extension = self.output_path.extension()
            .and_then(|s| s.to_str())
            .unwrap_or("mp4");

        // Find last number in filename using regex-like approach
        let mut last_num_start = None;
        let mut last_num_end = None;
        let mut in_number = false;

        for (i, c) in file_stem.chars().enumerate() {
            if c.is_ascii_digit() {
                if !in_number {
                    last_num_start = Some(i);
                    in_number = true;
                }
                last_num_end = Some(i + 1);
            } else {
                in_number = false;
            }
        }

        let new_stem = if let (Some(start), Some(end)) = (last_num_start, last_num_end) {
            let prefix = &file_stem[..start];
            let num_str = &file_stem[start..end];
            let suffix = &file_stem[end..];

            // Parse number and increment
            if let Ok(num) = num_str.parse::<u32>() {
                let new_num = num + 1;
                let old_width = num_str.len();

                // Calculate how many digits the new number has
                let new_num_digits = if new_num == 0 {
                    1
                } else {
                    ((new_num as f64).log10().floor() as usize) + 1
                };

                // Use original width if new number fits, otherwise use natural width
                let width = old_width.max(new_num_digits);

                format!("{}{:0width$}{}", prefix, new_num, suffix, width = width)
            } else {
                // If parse fails, just append 001
                format!("{}001", file_stem)
            }
        } else {
            // No number found, append 001
            format!("{}001", file_stem)
        };

        // Update path with new filename
        if let Some(parent) = self.output_path.parent() {
            self.output_path = parent.join(format!("{}.{}", new_stem, extension));
        } else {
            self.output_path = PathBuf::from(format!("{}.{}", new_stem, extension));
        }
    }

    /// Load dialog state from AppSettings (called when opening dialog)
    pub fn load_from_settings(settings: &crate::dialogs::encode::EncodeDialogSettings) -> Self {
        log::debug!("========== LOADING ENCODE DIALOG SETTINGS ==========");
        log::debug!("  Output: {}", settings.output_path.display());
        log::debug!("  Container: {:?}, FPS: {}, Codec: {:?}", settings.container, settings.fps, settings.selected_codec);
        log::debug!("  H.264: impl={:?}, mode={:?}, value={}, preset={}, profile={}",
            settings.codec_settings.h264.encoder_impl,
            settings.codec_settings.h264.quality_mode,
            settings.codec_settings.h264.quality_value,
            settings.codec_settings.h264.preset,
            settings.codec_settings.h264.profile
        );
        log::debug!("  H.265: impl={:?}, mode={:?}, value={}, preset={}, profile={}",
            settings.codec_settings.h265.encoder_impl,
            settings.codec_settings.h265.quality_mode,
            settings.codec_settings.h265.quality_value,
            settings.codec_settings.h265.preset,
            settings.codec_settings.h265.profile
        );
        log::debug!("  ProRes: profile={:?}", settings.codec_settings.prores.profile);
        log::debug!("  AV1: impl={:?}, mode={:?}, value={}, preset={}",
            settings.codec_settings.av1.encoder_impl,
            settings.codec_settings.av1.quality_mode,
            settings.codec_settings.av1.quality_value,
            settings.codec_settings.av1.preset
        );
        log::debug!("  Tonemap: {:?}", settings.tonemap_mode);

        Self {
            output_path: settings.output_path.clone(),
            container: settings.container,
            fps: settings.fps,
            selected_codec: settings.selected_codec,
            codec_settings: settings.codec_settings.clone(),
            is_encoding: false,
            progress: None,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            progress_rx: None,
            encode_thread: None,
            progress_bar: ProgressBar::new(400.0, 20.0),
            encoder_name: String::new(),
            tonemap_mode: settings.tonemap_mode,
        }
    }

    /// Save current dialog state to AppSettings (called when closing dialog or starting encode)
    pub fn save_to_settings(&self) -> crate::dialogs::encode::EncodeDialogSettings {
        log::debug!("========== SAVING ENCODE DIALOG SETTINGS ==========");
        log::debug!("  Output: {}", self.output_path.display());
        log::debug!("  Container: {:?}, FPS: {}, Codec: {:?}", self.container, self.fps, self.selected_codec);
        log::debug!("  H.264: impl={:?}, mode={:?}, value={}, preset={}, profile={}",
            self.codec_settings.h264.encoder_impl,
            self.codec_settings.h264.quality_mode,
            self.codec_settings.h264.quality_value,
            self.codec_settings.h264.preset,
            self.codec_settings.h264.profile
        );
        log::debug!("  H.265: impl={:?}, mode={:?}, value={}, preset={}, profile={}",
            self.codec_settings.h265.encoder_impl,
            self.codec_settings.h265.quality_mode,
            self.codec_settings.h265.quality_value,
            self.codec_settings.h265.preset,
            self.codec_settings.h265.profile
        );
        log::debug!("  ProRes: profile={:?}", self.codec_settings.prores.profile);
        log::debug!("  AV1: impl={:?}, mode={:?}, value={}, preset={}",
            self.codec_settings.av1.encoder_impl,
            self.codec_settings.av1.quality_mode,
            self.codec_settings.av1.quality_value,
            self.codec_settings.av1.preset
        );
        log::debug!("  Tonemap: {:?}", self.tonemap_mode);

        crate::dialogs::encode::EncodeDialogSettings {
            output_path: self.output_path.clone(),
            container: self.container,
            fps: self.fps,
            selected_codec: self.selected_codec,
            tonemap_mode: self.tonemap_mode,
            codec_settings: self.codec_settings.clone(),
        }
    }

    /// Build EncoderSettings from current UI state
    pub fn build_encoder_settings(&self) -> EncoderSettings {
        // self.output_path is already normalized (kept in sync with container changes)
        let (encoder_impl, quality_mode, quality_value, preset, profile, prores_profile) =
            match self.selected_codec {
                VideoCodec::H264 => (
                    self.codec_settings.h264.encoder_impl,
                    self.codec_settings.h264.quality_mode,
                    self.codec_settings.h264.quality_value,
                    Some(self.codec_settings.h264.preset.clone()),
                    Some(self.codec_settings.h264.profile.clone()),
                    None,
                ),
                VideoCodec::H265 => (
                    self.codec_settings.h265.encoder_impl,
                    self.codec_settings.h265.quality_mode,
                    self.codec_settings.h265.quality_value,
                    Some(self.codec_settings.h265.preset.clone()),
                    Some(self.codec_settings.h265.profile.clone()),
                    None,
                ),
                VideoCodec::AV1 => (
                    self.codec_settings.av1.encoder_impl,
                    self.codec_settings.av1.quality_mode,
                    self.codec_settings.av1.quality_value,
                    Some(self.codec_settings.av1.preset.clone()),
                    None,
                    None,
                ),
                VideoCodec::ProRes => (
                    crate::dialogs::encode::EncoderImpl::Software,
                    crate::dialogs::encode::QualityMode::CRF,
                    0, // ProRes doesn't use quality_value
                    None,
                    None,
                    Some(self.codec_settings.prores.profile),
                ),
            };

        EncoderSettings {
            output_path: self.output_path.clone(),
            container: self.container,
            codec: self.selected_codec,
            encoder_impl,
            quality_mode,
            quality_value,
            fps: self.fps,
            preset,
            profile,
            prores_profile,
            tonemap_mode: self.tonemap_mode,
        }
    }

    /// Check if encoding is currently in progress
    pub fn is_encoding(&self) -> bool {
        self.is_encoding
    }

    /// Stop encoding (public interface for ESC key handling)
    pub fn stop_encoding(&mut self) {
        self.stop_encoding_keep_window();
    }

    /// Render the encode dialog
    ///
    /// Returns: true if dialog should remain open, false if closed
    pub fn render(&mut self, ctx: &egui::Context) -> bool {
        let mut should_close = false;

        // Poll progress updates
        if let Some(rx) = &self.progress_rx {
            while let Ok(progress) = rx.try_recv() {
                self.progress = Some(progress);
            }
        }

        // Check if encoding completed (only process once while encoding)
        if self.is_encoding
            && let Some(ref progress) = self.progress {
                match &progress.stage {
                    EncodeStage::Complete => {
                        info!("Encoding completed successfully");
                        self.reset_encoding_state();
                    }
                    EncodeStage::Error(msg) => {
                        info!("Encoding failed: {}", msg);
                        self.reset_encoding_state();
                    }
                    _ => {}
                }
            }

        egui::Window::new("Video Encoder")
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.set_width(600.0);

                // === Output Path ===
                ui.horizontal(|ui| {
                    ui.label("Output:");
                    ui.add_enabled_ui(!self.is_encoding, |ui| {
                        let path_str = self.output_path.display().to_string();
                        let mut edit_path = path_str.clone();
                        if ui.text_edit_singleline(&mut edit_path).changed() {
                            self.output_path = PathBuf::from(edit_path);
                        }

                        // Increment filename button
                        if ui.button("+")
                            .on_hover_text("Increment number in filename (e.g., file001.mp4 → file002.mp4)")
                            .clicked()
                        {
                            self.increment_filename();
                        }

                        if ui.button("Browse").clicked()
                            && let Some(path) = rfd::FileDialog::new()
                                .set_file_name("output.mp4")
                                .save_file()
                        {
                            self.output_path = path;
                        }
                    });
                });

                ui.add_space(8.0);

                // === Framerate ===
                ui.horizontal(|ui| {
                    ui.label("Framerate:");
                    ui.add_enabled_ui(!self.is_encoding, |ui| {
                        ui.add(egui::Slider::new(&mut self.fps, 1.0..=240.0).text("fps"));
                    });
                });

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(4.0);

                // === Codec Tabs ===
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(!self.is_encoding, |ui| {
                        for codec in VideoCodec::all() {
                            let is_available = codec.is_available();
                            let is_selected = self.selected_codec == *codec;

                            // Disable tab if codec not available
                            ui.add_enabled_ui(is_available, |ui| {
                                let button = egui::Button::new(codec.to_string())
                                    .selected(is_selected)
                                    .min_size(egui::vec2(100.0, 0.0));

                                if ui.add(button).clicked() {
                                    self.selected_codec = *codec;

                                    // Auto-update container and file extension based on codec
                                    let preferred_container = codec.preferred_container();
                                    self.container = preferred_container;
                                    self.output_path.set_extension(preferred_container.extension());
                                }
                            });

                            if !is_available {
                                ui.label("✗")
                                    .on_hover_text(format!("{} encoder not available", codec));
                            }
                        }
                    });
                });

                ui.separator();
                ui.add_space(8.0);

                // === Per-Codec Settings ===
                ui.add_enabled_ui(!self.is_encoding, |ui| match self.selected_codec {
                    VideoCodec::H264 => self.render_h264_settings(ui),
                    VideoCodec::H265 => self.render_h265_settings(ui),
                    VideoCodec::AV1 => self.render_av1_settings(ui),
                    VideoCodec::ProRes => self.render_prores_settings(ui),
                });

                ui.add_space(12.0);

                // === Frame Range Info ===
                  ui.label("Frame Range: (use active Comp)");

                ui.add_space(12.0);

                // === Progress (always visible to prevent dialog size jumping) ===
                ui.separator();
                ui.heading("Progress");

                if self.is_encoding {
                    if let Some(ref progress) = self.progress {
                        // Stage description
                        let stage_text = match &progress.stage {
                            EncodeStage::Validating => "Validating frame sizes...",
                            EncodeStage::Opening => "Opening encoder...",
                            EncodeStage::Encoding => "Encoding frames...",
                            EncodeStage::Flushing => "Flushing encoder...",
                            EncodeStage::Complete => "Complete!",
                            EncodeStage::Error(msg) => msg.as_str(),
                        };
                        ui.label(stage_text);

                        // Progress bar
                        self.progress_bar
                            .set_progress(progress.current_frame.max(0) as usize, progress.total_frames.max(0) as usize);
                        self.progress_bar.render(ui);

                        // Encoder name
                        if !self.encoder_name.is_empty() {
                            ui.label(format!("Encoder: {}", self.encoder_name));
                        }
                    }
                } else {
                    // Not encoding: show empty progress bar to maintain dialog size
                    ui.label("Ready to encode");
                    self.progress_bar.set_progress(0, 100);
                    self.progress_bar.render(ui);
                    ui.label(""); // Empty label for encoder name spacing
                }

                ui.add_space(8.0);

                ui.separator();

                  // === Readiness check ===
                  let ready_to_encode = true;

                if !ready_to_encode {
                    ui.colored_label(
                        egui::Color32::from_rgb(200, 150, 0),
                        "Frames are still loading...",
                    );
                }

                // === Buttons ===
                ui.horizontal(|ui| {
                    // Close button (stops encoding if running, then closes window)
                    if ui.button("Close").clicked() {
                        if self.is_encoding {
                            self.stop_encoding_and_close();
                        }
                        should_close = true;
                    }

                    // Encode/Stop button (toggles between Encode and Stop)
                    if self.is_encoding {
                        // During encoding: show "Stop" button
                        if ui.button("Stop").clicked() {
                            self.stop_encoding_keep_window();
                        }
                    } else {
                        // Not encoding: show "Encode" button
                        ui.add_enabled_ui(ready_to_encode, |ui| {
                            let mut button = ui.button("Encode");
                            if !ready_to_encode {
                                button = button.on_disabled_hover_text("Wait for all frames to load");
                            }
                              if button.clicked() {
                                  self.start_encoding();
                            }
                        });
                    }
                });
            });

        // Return true if window should stay open
        !should_close
    }

    /// Start encoding process
    fn start_encoding(&mut self) {
        let settings = self.build_encoder_settings();
        info!("========== STARTING ENCODING ==========");
        info!("Codec: {:?}, Container: {:?}", settings.codec, settings.container);
        info!("Settings: {:?}", settings);

        // Reset state for new encoding
        self.cancel_flag.store(false, Ordering::Relaxed);
        self.progress = None; // Clear old progress

        // Create progress channel
        let (tx, rx) = channel();
        self.progress_rx = Some(rx);

          // Clone data for thread
        let settings_clone = self.build_encoder_settings();
        let cancel_flag_clone = Arc::clone(&self.cancel_flag);

          // Spawn encoder thread (Comp-based)
            use crate::dialogs::encode::encode_comp;
        use std::thread;

            let handle = thread::spawn(move || {
                info!("Encoder thread started");

                // TODO: Get real comp and project from UI state
                // For now we build minimal empty comp and project
                let comp = crate::entities::comp::Comp::new("Comp", 0, 0, settings_clone.fps);
                let project = crate::entities::project::Project::new();

                info!("Calling encode_comp()...");
                encode_comp(&comp, &project, &settings_clone, tx, cancel_flag_clone)
            });

        self.encode_thread = Some(handle);
        self.is_encoding = true;
    }

    /// Stop encoding and close window
    fn stop_encoding_and_close(&mut self) {
        info!("Stopping encoding (closing window)");
        self.stop_encoding_internal();
    }

    /// Stop encoding but keep window open
    fn stop_encoding_keep_window(&mut self) {
        info!("Stopping encoding (keeping window open)");
        self.stop_encoding_internal();
    }

    /// Internal: Stop encoding thread with timeout
    fn stop_encoding_internal(&mut self) {
        self.cancel_flag.store(true, Ordering::Relaxed);

        // Wait for thread with timeout
        if let Some(handle) = self.encode_thread.take() {
            use std::time::{Duration, Instant};

            // Try to join with 2 second timeout
            let timeout = Duration::from_secs(2);
            let start = Instant::now();

            loop {
                if handle.is_finished() {
                    match handle.join() {
                        Ok(Ok(())) => info!("Encode thread stopped cleanly"),
                        Ok(Err(e)) => {
                            info!("Encode thread stopped with error: {}", e);
                        }
                        Err(_) => info!("Encode thread panicked"),
                    }
                    break;
                }

                if start.elapsed() > timeout {
                    info!("Encode thread didn't stop within timeout - forcefully resetting UI");
                    // Thread is stuck, but we reset UI anyway
                    // The thread will be dropped and remain orphaned
                    break;
                }

                std::thread::sleep(Duration::from_millis(100));
            }
        }

        // Force reset to clean state
        self.reset_encoding_state();
        self.progress = None;
        self.cancel_flag = Arc::new(AtomicBool::new(false));
    }

    /// Stop encoding (cleanup after completion or error)
    fn reset_encoding_state(&mut self) {
        self.is_encoding = false;
        self.progress_rx = None;

        // CRITICAL: Wait for encoder thread to actually finish
        if let Some(handle) = self.encode_thread.take() {
            // Thread should already be finished (we're here because of Complete/Error)
            // But we still need to join() to clean up properly
            if handle.is_finished() {
                let _ = handle.join(); // Ignore result, we already know it completed
            } else {
                // Thread still running (shouldn't happen) - log warning
                info!("Warning: encoder thread still running during reset_encoding_state");
                let _ = handle.join(); // Wait for it anyway
            }
        }
    }

    /// Render H.264 settings
    fn render_h264_settings(&mut self, ui: &mut egui::Ui) {
        use crate::dialogs::encode::{EncoderImpl, QualityMode};

        // Encoder implementation
        ui.label("Encoder:");
        ui.horizontal(|ui| {
            for impl_type in EncoderImpl::all() {
                ui.radio_value(
                    &mut self.codec_settings.h264.encoder_impl,
                    *impl_type,
                    impl_type.to_string(),
                );
            }
        });

        ui.add_space(4.0);

        // Quality mode
        ui.label("Quality Mode:");
        ui.horizontal(|ui| {
            for mode in QualityMode::all() {
                ui.radio_value(
                    &mut self.codec_settings.h264.quality_mode,
                    *mode,
                    mode.to_string(),
                );
            }
        });

        // Quality value
        ui.horizontal(|ui| {
            ui.label("Value:");
            let hint = match self.codec_settings.h264.quality_mode {
                QualityMode::CRF => "18=best, 23=default, 28=fast",
                QualityMode::Bitrate => "kbps",
            };
            ui.add(
                egui::Slider::new(&mut self.codec_settings.h264.quality_value, 1..=10000)
                    .text(hint),
            );
        });

        ui.add_space(4.0);

        // Preset
        ui.horizontal(|ui| {
            ui.label("Preset:");

            // Presets for H.264 encoders
            let presets = match self.codec_settings.h264.encoder_impl {
                EncoderImpl::Hardware => {
                    // NVENC/QSV/AMF
                    vec!["default", "slow", "medium", "fast", "p1", "p2", "p3", "p4", "p5", "p6", "p7"]
                }
                EncoderImpl::Software | EncoderImpl::Auto => {
                    // libx264
                    vec!["ultrafast", "superfast", "veryfast", "faster", "fast", "medium", "slow", "slower", "veryslow", "placebo"]
                }
            };

            egui::ComboBox::from_id_salt("h264_preset")
                .selected_text(&self.codec_settings.h264.preset)
                .show_ui(ui, |ui| {
                    for preset in presets {
                        ui.selectable_value(
                            &mut self.codec_settings.h264.preset,
                            preset.to_string(),
                            preset,
                        );
                    }
                });
        });

        // Profile (libx264 only)
        ui.horizontal(|ui| {
            ui.label("Profile:");

            let profiles = vec!["baseline", "main", "high", "high10", "high422", "high444"];

            egui::ComboBox::from_id_salt("h264_profile")
                .selected_text(&self.codec_settings.h264.profile)
                .show_ui(ui, |ui| {
                    for profile in profiles {
                        ui.selectable_value(
                            &mut self.codec_settings.h264.profile,
                            profile.to_string(),
                            profile,
                        );
                    }
                });
        });

        ui.add_space(4.0);
        ui.label(""); // Empty line for vertical alignment
    }

    /// Render H.265 settings
    fn render_h265_settings(&mut self, ui: &mut egui::Ui) {
        use crate::dialogs::encode::{EncoderImpl, QualityMode};

        // Encoder implementation
        ui.label("Encoder:");
        ui.horizontal(|ui| {
            for impl_type in EncoderImpl::all() {
                ui.radio_value(
                    &mut self.codec_settings.h265.encoder_impl,
                    *impl_type,
                    impl_type.to_string(),
                );
            }
        });

        ui.add_space(4.0);

        // Quality mode
        ui.label("Quality Mode:");
        ui.horizontal(|ui| {
            for mode in QualityMode::all() {
                ui.radio_value(
                    &mut self.codec_settings.h265.quality_mode,
                    *mode,
                    mode.to_string(),
                );
            }
        });

        // Quality value
        ui.horizontal(|ui| {
            ui.label("Value:");
            let hint = match self.codec_settings.h265.quality_mode {
                QualityMode::CRF => "28=default (higher than H.264)",
                QualityMode::Bitrate => "kbps",
            };
            ui.add(
                egui::Slider::new(&mut self.codec_settings.h265.quality_value, 1..=10000)
                    .text(hint),
            );
        });

        ui.add_space(4.0);

        // Preset
        ui.horizontal(|ui| {
            ui.label("Preset:");

            // Presets for H.265 encoders (same as H.264)
            let presets = match self.codec_settings.h265.encoder_impl {
                EncoderImpl::Hardware => {
                    // NVENC/QSV/AMF
                    vec!["default", "slow", "medium", "fast", "p1", "p2", "p3", "p4", "p5", "p6", "p7"]
                }
                EncoderImpl::Software | EncoderImpl::Auto => {
                    // libx265
                    vec!["ultrafast", "superfast", "veryfast", "faster", "fast", "medium", "slow", "slower", "veryslow", "placebo"]
                }
            };

            egui::ComboBox::from_id_salt("h265_preset")
                .selected_text(&self.codec_settings.h265.preset)
                .show_ui(ui, |ui| {
                    for preset in presets {
                        ui.selectable_value(
                            &mut self.codec_settings.h265.preset,
                            preset.to_string(),
                            preset,
                        );
                    }
                });
        });

        ui.add_space(4.0);

        // Profile (main or main10)
        ui.horizontal(|ui| {
            ui.label("Profile:");

            let profiles = vec!["main", "main10"];

            egui::ComboBox::from_id_salt("h265_profile")
                .selected_text(&self.codec_settings.h265.profile)
                .show_ui(ui, |ui| {
                    for profile in profiles {
                        ui.selectable_value(
                            &mut self.codec_settings.h265.profile,
                            profile.to_string(),
                            profile,
                        );
                    }
                });
        });

        // Empty lines for vertical alignment with H264 tab
        ui.add_space(4.0);
        ui.label("");
    }

    /// Render ProRes settings
    fn render_prores_settings(&mut self, ui: &mut egui::Ui) {
        ui.label("Profile:");
        ui.horizontal(|ui| {
            for profile in ProResProfile::all() {
                ui.radio_value(
                    &mut self.codec_settings.prores.profile,
                    *profile,
                    profile.to_string(),
                );
            }
        });

        ui.add_space(4.0);
        ui.label("ProRes is always software-encoded (prores_ks)");

        // Empty lines for vertical alignment with H264 tab
        ui.add_space(4.0);
        ui.label("");
        ui.add_space(4.0);
        ui.label("");
        ui.add_space(4.0);
        ui.label("");
        ui.add_space(4.0);
        ui.label("");
        ui.add_space(4.0);
        ui.label("");
    }

    /// Render AV1 settings
    fn render_av1_settings(&mut self, ui: &mut egui::Ui) {
        use crate::dialogs::encode::{EncoderImpl, QualityMode};

        ui.label("Encoder:");
        ui.horizontal(|ui| {
            for impl_type in EncoderImpl::all() {
                ui.radio_value(
                    &mut self.codec_settings.av1.encoder_impl,
                    *impl_type,
                    impl_type.to_string(),
                );
            }
        });

        ui.label("Quality Mode:");
        ui.horizontal(|ui| {
            for mode in QualityMode::all() {
                ui.radio_value(
                    &mut self.codec_settings.av1.quality_mode,
                    *mode,
                    mode.to_string(),
                );
            }
        });

        ui.horizontal(|ui| {
            ui.label("Value:");
            let hint = match self.codec_settings.av1.quality_mode {
                QualityMode::CRF => "CRF (0-63, lower=better)",
                QualityMode::Bitrate => "kbps",
            };
            ui.add(
                egui::Slider::new(&mut self.codec_settings.av1.quality_value, 0..=10000).text(hint),
            );
        });

        ui.horizontal(|ui| {
            ui.label("Preset:");

            // Determine available presets based on encoder
            let (presets, descriptions): (Vec<&str>, Vec<&str>) = match self.codec_settings.av1.encoder_impl {
                EncoderImpl::Hardware => {
                    // NVENC/QSV/AMF: p1-p7 + named presets
                    (
                        vec!["p1", "p2", "p3", "p4", "p5", "p6", "p7", "default", "slow", "medium", "fast"],
                        vec![
                            "P1 (fastest, lowest quality)",
                            "P2 (faster, lower quality)",
                            "P3 (fast, low quality)",
                            "P4 (medium, default)",
                            "P5 (slow, good quality)",
                            "P6 (slower, better quality)",
                            "P7 (slowest, best quality)",
                            "Default",
                            "Slow (HQ 2 passes)",
                            "Medium (HQ 1 pass)",
                            "Fast (HP 1 pass)",
                        ],
                    )
                }
                EncoderImpl::Software | EncoderImpl::Auto => {
                    // SVT-AV1/libaom: numeric 0-13 presets
                    (
                        vec!["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13"],
                        vec![
                            "0 (slowest, best)",
                            "1",
                            "2",
                            "3",
                            "4",
                            "5",
                            "6 (balanced)",
                            "7",
                            "8",
                            "9",
                            "10",
                            "11",
                            "12",
                            "13 (fastest)",
                        ],
                    )
                }
            };

            egui::ComboBox::from_id_salt("av1_preset")
                .selected_text(&self.codec_settings.av1.preset)
                .show_ui(ui, |ui| {
                    for (preset, desc) in presets.iter().zip(descriptions.iter()) {
                        ui.selectable_value(
                            &mut self.codec_settings.av1.preset,
                            preset.to_string(),
                            format!("{} - {}", preset, desc),
                        );
                    }
                });
        });

        ui.add_space(4.0);
        ui.label("💡 AV1: Best compression, slower encoding. HW: RTX 40xx/Arc/RDNA 3");

        // Empty line for vertical alignment with H264 tab
        ui.add_space(4.0);
        ui.label("");
    }
}

