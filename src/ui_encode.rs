//! Encoding dialog UI
//!
//! Provides dialog for configuring and running video encoding.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::thread::JoinHandle;

use eframe::egui;
use log::info;

use crate::cache::Cache;
use crate::encode::{
    Container, EncodeError, EncodeProgress, EncodeStage, EncoderImpl, EncoderSettings,
    QualityMode, VideoCodec,
};
use crate::progress_bar::ProgressBar;

/// Encoding dialog state
pub struct EncodeDialog {
    /// Current encoder settings (editable)
    pub settings: EncoderSettings,

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
}

impl EncodeDialog {
    /// Create new encode dialog with settings from AppSettings
    pub fn new(settings: EncoderSettings) -> Self {
        Self {
            settings,
            is_encoding: false,
            progress: None,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            progress_rx: None,
            encode_thread: None,
            progress_bar: ProgressBar::new(400.0, 20.0),
            encoder_name: String::new(),
        }
    }

    /// Render the encode dialog
    ///
    /// Returns: true if dialog should remain open, false if closed
    pub fn render(&mut self, ctx: &egui::Context, cache: &Cache) -> bool {
        let mut should_close = false;

        // Poll progress updates
        if let Some(rx) = &self.progress_rx {
            while let Ok(progress) = rx.try_recv() {
                self.progress = Some(progress);
            }
        }

        // Check if encoding completed
        if let Some(ref progress) = self.progress {
            match &progress.stage {
                EncodeStage::Complete => {
                    info!("Encoding completed successfully");
                    self.stop_encoding();
                }
                EncodeStage::Error(msg) => {
                    info!("Encoding failed: {}", msg);
                    self.stop_encoding();
                }
                _ => {}
            }
        }

        egui::Window::new("Video Encoder")
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.set_width(500.0);

                // === Output Path ===
                ui.horizontal(|ui| {
                    ui.label("Output:");
                    ui.add_enabled_ui(!self.is_encoding, |ui| {
                        let path_str = self.settings.output_path.display().to_string();
                        let mut edit_path = path_str.clone();
                        if ui.text_edit_singleline(&mut edit_path).changed() {
                            self.settings.output_path = PathBuf::from(edit_path);
                        }

                        if ui.button("Browse").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .set_file_name("output.mp4")
                                .save_file()
                            {
                                self.settings.output_path = path;
                            }
                        }
                    });
                });

                ui.add_space(8.0);

                // === Container ===
                ui.label("Container:");
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(!self.is_encoding, |ui| {
                        for container in Container::all() {
                            if ui
                                .radio_value(&mut self.settings.container, *container, container.to_string())
                                .changed()
                            {
                                // Update file extension when container changes
                                self.settings
                                    .output_path
                                    .set_extension(container.extension());
                            }
                        }
                    });
                });

                ui.add_space(8.0);

                // === Codec ===
                ui.label("Codec:");
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(!self.is_encoding, |ui| {
                        for codec in VideoCodec::all() {
                            ui.radio_value(&mut self.settings.codec, *codec, codec.to_string());
                        }
                    });
                });

                ui.add_space(8.0);

                // === Encoder Implementation ===
                ui.label("Encoder:");
                ui.add_enabled_ui(!self.is_encoding, |ui| {
                    for impl_type in EncoderImpl::all() {
                        ui.radio_value(
                            &mut self.settings.encoder_impl,
                            *impl_type,
                            impl_type.to_string(),
                        );
                    }
                });

                ui.add_space(8.0);

                // === Quality Mode ===
                ui.label("Quality:");
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(!self.is_encoding, |ui| {
                        for mode in QualityMode::all() {
                            ui.radio_value(&mut self.settings.quality_mode, *mode, mode.to_string());
                        }
                    });
                });

                ui.horizontal(|ui| {
                    ui.label("Value:");
                    ui.add_enabled_ui(!self.is_encoding, |ui| {
                        let hint = match self.settings.quality_mode {
                            QualityMode::CRF => "18=best, 23=default, 28=fast",
                            QualityMode::Bitrate => "kbps",
                        };
                        ui.add(
                            egui::Slider::new(&mut self.settings.quality_value, 1..=10000)
                                .text(hint),
                        );
                    });
                });

                ui.horizontal(|ui| {
                    ui.label("Framerate:");
                    ui.add_enabled_ui(!self.is_encoding, |ui| {
                        ui.add(
                            egui::Slider::new(&mut self.settings.fps, 1.0..=240.0)
                                .text("fps"),
                        );
                    });
                });

                ui.add_space(8.0);

                // === Frame Range Info ===
                let (start, end) = cache.get_play_range();
                let frame_count = end.saturating_sub(start) + 1;
                ui.label(format!(
                    "Frame Range: {} - {} ({} frames)",
                    start, end, frame_count
                ));

                ui.add_space(12.0);

                // === Progress Section (only when encoding) ===
                if self.is_encoding {
                    ui.separator();
                    ui.heading("Progress");

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
                            .set_progress(progress.current_frame, progress.total_frames);
                        self.progress_bar.render(ui);

                        // Encoder name
                        if !self.encoder_name.is_empty() {
                            ui.label(format!("Encoder: {}", self.encoder_name));
                        }
                    }

                    ui.add_space(8.0);
                }

                ui.separator();

                // === Buttons ===
                ui.horizontal(|ui| {
                    // Close/Cancel button
                    let close_text = if self.is_encoding { "Cancel" } else { "Close" };
                    if ui.button(close_text).clicked() {
                        if self.is_encoding {
                            self.cancel_encoding();
                        }
                        should_close = true;
                    }

                    // Encode button (disabled during encoding)
                    ui.add_enabled_ui(!self.is_encoding, |ui| {
                        if ui.button("Encode").clicked() {
                            self.start_encoding(cache);
                        }
                    });
                });
            });

        // Return true if window should stay open
        !should_close
    }

    /// Start encoding process
    fn start_encoding(&mut self, cache: &Cache) {
        info!("Starting encoding: {:?}", self.settings);

        // Reset cancel flag
        self.cancel_flag.store(false, Ordering::Relaxed);

        // Create progress channel
        let (tx, rx) = channel();
        self.progress_rx = Some(rx);

        // Clone data for thread (including play_range)
        let cache_clone = cache.sequences().iter()
            .map(|s| s.clone())
            .collect::<Vec<_>>();
        let play_range = cache.get_play_range();
        let settings_clone = self.settings.clone();
        let cancel_flag_clone = Arc::clone(&self.cancel_flag);

        // Spawn encoder thread
        use crate::encode::encode_sequence;
        use std::thread;

        let handle = thread::spawn(move || {
            // Create temporary cache with cloned sequences
            let (mut temp_cache, _rx) = Cache::new(0.75, None);
            for seq in cache_clone {
                temp_cache.append_seq(seq);
            }

            // Set play range from original cache (append_seq sets full range by default)
            temp_cache.set_play_range(play_range.0, play_range.1);

            // Run encoding
            encode_sequence(&mut temp_cache, &settings_clone, tx, cancel_flag_clone)
        });

        self.encode_thread = Some(handle);
        self.is_encoding = true;
    }

    /// Cancel encoding
    fn cancel_encoding(&mut self) {
        info!("Cancelling encoding");
        self.cancel_flag.store(true, Ordering::Relaxed);
    }

    /// Stop encoding (cleanup after completion or error)
    fn stop_encoding(&mut self) {
        self.is_encoding = false;
        self.progress_rx = None;
        self.encode_thread = None;
    }
}
