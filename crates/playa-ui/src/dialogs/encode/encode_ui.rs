//! Encode dialog UI
//!
//! Configures and runs video / image-sequence encoding. The *settings* UI is the
//! generic, data-driven [`egui_encode_dialog`] widget: this module builds an
//! [`EncodeSchema`] from playa's codec/format tables, shows the widget, and maps
//! the returned [`WidgetSettings`] back onto playa's model. Everything below the
//! settings panel — the encode worker thread, the ffmpeg/EXR pipeline, the
//! progress channel + progress bar, and the EXR metadata round-trip — stays in
//! `encode.rs` and is kicked unchanged from [`EncodeDialog::start_encoding`].
//!
//! Data flow each frame (when idle):
//!   model (this struct) -> `build_schema` + `model_to_widget` -> widget.show()
//!   -> `apply_widget` -> model.  On Start the model feeds `build_encoder_settings`
//!   / `sequence_settings` into the existing worker.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::thread::JoinHandle;

use eframe::egui;
use log::info;

use crate::dialogs::encode::{
    ChannelMode, CodecSettings, Container, EncodeError, EncodeProgress, EncodeStage, EncoderImpl,
    EncoderSettings, ExportMode, ExrCompression, ExrEncodeMode, OutputBitDepth, ProResProfile,
    QualityMode, SequenceFormat, SequenceSettings, TiffBitDepth, TiffCompression, VideoCodec,
};
use egui_encode_dialog::{
    Codec, EncodeDialog as EncodeWidget, EncodeDialogResult, EncodeOption, EncodeSchema,
    EncodeSettings as WidgetSettings, Format, ShowConfig,
};
use egui_progressbar::ProgressBar;
use playa_engine::entities::frame::TonemapMode;
use playa_engine::entities::{Comp, Project};

/// Encoding dialog state.
///
/// Holds the persisted *model* (output path / codec / per-codec + sequence
/// settings) plus the live encode-worker state (cancel flag, progress channel,
/// thread handles). The settings widget is rebuilt from this model each frame,
/// so this struct remains the single source of truth that `load_from_settings`
/// / `save_to_settings` persist.
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

    /// Orphaned thread handles (timed out but not joined)
    orphan_handles: Vec<JoinHandle<Result<(), EncodeError>>>,

    /// Progress bar widget
    progress_bar: ProgressBar,

    /// Tonemapping mode for HDR→LDR conversion (video path)
    pub tonemap_mode: playa_engine::entities::frame::TonemapMode,

    /// Export mode (Video or Sequence)
    pub export_mode: ExportMode,

    /// Image sequence settings
    pub sequence_settings: SequenceSettings,
}

impl EncodeDialog {
    /// Load dialog state from AppSettings (called when opening dialog)
    pub fn load_from_settings(settings: &crate::dialogs::encode::EncodeDialogSettings) -> Self {
        log::trace!("========== LOADING ENCODE DIALOG SETTINGS ==========");
        log::trace!("  Output: {}", settings.output_path.display());
        log::trace!(
            "  Container: {:?}, FPS: {}, Codec: {:?}",
            settings.container,
            settings.fps,
            settings.selected_codec
        );
        log::trace!(
            "  H.264: impl={:?}, mode={:?}, value={}, preset={}, profile={}",
            settings.codec_settings.h264.encoder_impl,
            settings.codec_settings.h264.quality_mode,
            settings.codec_settings.h264.quality_value,
            settings.codec_settings.h264.preset,
            settings.codec_settings.h264.profile
        );
        log::trace!(
            "  H.265: impl={:?}, mode={:?}, value={}, preset={}, profile={}",
            settings.codec_settings.h265.encoder_impl,
            settings.codec_settings.h265.quality_mode,
            settings.codec_settings.h265.quality_value,
            settings.codec_settings.h265.preset,
            settings.codec_settings.h265.profile
        );
        log::trace!(
            "  ProRes: profile={:?}",
            settings.codec_settings.prores.profile
        );
        log::trace!(
            "  AV1: impl={:?}, mode={:?}, value={}, preset={}",
            settings.codec_settings.av1.encoder_impl,
            settings.codec_settings.av1.quality_mode,
            settings.codec_settings.av1.quality_value,
            settings.codec_settings.av1.preset
        );
        log::trace!("  Tonemap: {:?}", settings.tonemap_mode);
        log::trace!("  ExportMode: {:?}", settings.export_mode);
        log::trace!(
            "  Sequence: format={:?}, channels={:?}, depth={:?}",
            settings.sequence_settings.format,
            settings.sequence_settings.channels,
            settings.sequence_settings.bit_depth
        );

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
            orphan_handles: Vec::new(),
            progress_bar: ProgressBar::new(400.0, 20.0),
            tonemap_mode: settings.tonemap_mode,
            export_mode: settings.export_mode,
            sequence_settings: settings.sequence_settings.clone(),
        }
    }

    /// Save current dialog state to AppSettings (called when closing dialog or starting encode)
    pub fn save_to_settings(&self) -> crate::dialogs::encode::EncodeDialogSettings {
        log::trace!("========== SAVING ENCODE DIALOG SETTINGS ==========");
        log::trace!("  Output: {}", self.output_path.display());
        log::trace!(
            "  Container: {:?}, FPS: {}, Codec: {:?}",
            self.container,
            self.fps,
            self.selected_codec
        );
        log::trace!(
            "  H.264: impl={:?}, mode={:?}, value={}, preset={}, profile={}",
            self.codec_settings.h264.encoder_impl,
            self.codec_settings.h264.quality_mode,
            self.codec_settings.h264.quality_value,
            self.codec_settings.h264.preset,
            self.codec_settings.h264.profile
        );
        log::trace!(
            "  H.265: impl={:?}, mode={:?}, value={}, preset={}, profile={}",
            self.codec_settings.h265.encoder_impl,
            self.codec_settings.h265.quality_mode,
            self.codec_settings.h265.quality_value,
            self.codec_settings.h265.preset,
            self.codec_settings.h265.profile
        );
        log::trace!("  ProRes: profile={:?}", self.codec_settings.prores.profile);
        log::trace!(
            "  AV1: impl={:?}, mode={:?}, value={}, preset={}",
            self.codec_settings.av1.encoder_impl,
            self.codec_settings.av1.quality_mode,
            self.codec_settings.av1.quality_value,
            self.codec_settings.av1.preset
        );
        log::trace!("  Tonemap: {:?}", self.tonemap_mode);
        log::trace!("  ExportMode: {:?}", self.export_mode);
        log::trace!(
            "  Sequence: format={:?}, channels={:?}, depth={:?}",
            self.sequence_settings.format,
            self.sequence_settings.channels,
            self.sequence_settings.bit_depth
        );

        crate::dialogs::encode::EncodeDialogSettings {
            output_path: self.output_path.clone(),
            container: self.container,
            fps: self.fps,
            selected_codec: self.selected_codec,
            tonemap_mode: self.tonemap_mode,
            codec_settings: self.codec_settings.clone(),
            export_mode: self.export_mode,
            sequence_settings: self.sequence_settings.clone(),
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

    /// Render the encode dialog.
    ///
    /// While idle, shows the generic settings widget (built from the model each
    /// frame). While encoding, shows playa's own progress window instead.
    ///
    /// Returns: true if dialog should remain open, false if closed
    pub fn render(
        &mut self,
        ctx: &egui::Context,
        project: &Project,
        active_comp: Option<&Comp>,
    ) -> bool {
        // Poll progress updates
        if let Some(rx) = &self.progress_rx {
            while let Ok(progress) = rx.try_recv() {
                self.progress = Some(progress);
            }
        }

        // Request continuous repaint while encoding (progress bar updates)
        if self.is_encoding {
            ctx.request_repaint();
        }

        // Check if encoding completed (only process once while encoding)
        if self.is_encoding
            && let Some(ref progress) = self.progress
        {
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

        let mut should_close = false;

        if self.is_encoding {
            // Progress / worker-status UI (kept as before, in its own window since the
            // settings widget owns its modal and does not render progress).
            self.render_progress_window(ctx, &mut should_close);
        } else {
            // Settings UI: build the schema + working settings from the model, show
            // the generic widget, then mirror its result back into the model.
            let schema = self.build_schema();
            let mut widget = EncodeWidget::with_settings(schema, self.model_to_widget());
            let cfg = ShowConfig {
                title: "Export".to_string(),
                width: 600.0,
                show_browse: true,
                // Frame range stays "use active Comp" (the worker derives it from the
                // comp's play_range), so we hide the widget's range row.
                show_frame_range: false,
                start_label: "Encode".to_string(),
            };
            let result = widget.show(ctx, &cfg);

            // Host-owned Browse: the widget only signals intent; we run rfd ourselves.
            if widget.take_browse_request() {
                let mut fd = rfd::FileDialog::new();
                if let Some(name) = self.output_path.file_name().and_then(|s| s.to_str()) {
                    fd = fd.set_file_name(name);
                }
                if let Some(path) = fd.save_file() {
                    widget.set_output_path(path.display().to_string());
                }
            }

            // Mirror the widget's working state into the model so persistence and the
            // encode worker (build_encoder_settings / sequence_settings) see edits.
            self.apply_widget(widget.settings());

            match result {
                EncodeDialogResult::Open => {}
                EncodeDialogResult::Cancelled => should_close = true,
                EncodeDialogResult::Start(settings) => {
                    self.apply_widget(&settings);
                    if let Some(comp) = active_comp {
                        self.start_encoding(comp, project);
                    } else {
                        info!("Encode requested but there is no active comp to encode");
                    }
                }
            }
        }

        // Return true if window should stay open
        !should_close
    }

    /// Progress window shown while encoding (the widget renders no progress).
    fn render_progress_window(&mut self, ctx: &egui::Context, should_close: &mut bool) {
        let window_title = match self.export_mode {
            ExportMode::Video => "Video Encoder",
            ExportMode::Sequence => "Image Sequence Export",
        };
        egui::Window::new(window_title)
            .id(egui::Id::new("encode_progress"))
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.set_width(420.0);
                ui.heading("Progress");

                if let Some(ref progress) = self.progress {
                    let stage_text = match &progress.stage {
                        EncodeStage::Validating => "Validating frame sizes...",
                        EncodeStage::Opening => "Opening encoder...",
                        EncodeStage::Encoding => "Encoding frames...",
                        EncodeStage::Flushing => "Flushing encoder...",
                        EncodeStage::Complete => "Complete!",
                        EncodeStage::Error(msg) => msg.as_str(),
                    };
                    ui.label(stage_text);
                    self.progress_bar.set_progress(
                        progress.current_frame.max(0) as usize,
                        progress.total_frames.max(0) as usize,
                    );
                    self.progress_bar.render(ui);
                } else {
                    ui.label("Starting...");
                    self.progress_bar.render(ui);
                }

                ui.add_space(8.0);
                ui.separator();

                ui.horizontal(|ui| {
                    // Close: stop the encode and close the dialog.
                    if ui.button("Close").clicked() {
                        self.stop_encoding_and_close();
                        *should_close = true;
                    }
                    // Stop: cancel the encode but keep the dialog open.
                    if ui.button("Stop").clicked() {
                        self.stop_encoding_keep_window();
                    }
                });
            });
    }

    // ===================================================================
    // Schema construction (model -> widget) and result mapping (widget -> model)
    // ===================================================================

    /// The widget format/codec ids for the currently selected model target.
    fn current_ids(&self) -> (&'static str, &'static str) {
        match self.export_mode {
            ExportMode::Video => match self.selected_codec {
                VideoCodec::H264 => ("mp4", "h264"),
                VideoCodec::H265 => ("mp4", "h265"),
                VideoCodec::AV1 => ("mp4", "av1"),
                VideoCodec::ProRes => ("mov", "prores"),
            },
            ExportMode::Sequence => match self.sequence_settings.format {
                SequenceFormat::Exr => ("exr", "exr"),
                SequenceFormat::Png => ("png", "png"),
                SequenceFormat::Jpeg => ("jpeg", "jpeg"),
                SequenceFormat::Tiff => ("tiff", "tiff"),
                SequenceFormat::Tga => ("tga", "tga"),
            },
        }
    }

    /// Working settings handed to the widget. Only ids + path are set; the widget's
    /// `with_settings` repair seeds every option value from the schema defaults,
    /// which we build from the model — so the displayed values always equal the model.
    fn model_to_widget(&self) -> WidgetSettings {
        let (format_id, codec_id) = self.current_ids();
        WidgetSettings {
            format_id: format_id.to_string(),
            codec_id: codec_id.to_string(),
            output_path: self.output_path.display().to_string(),
            ..Default::default()
        }
    }

    /// Common image-sequence options (channels / bit depth / tonemapping), shared
    /// by every sequence format. Choice lists are format-specific (alpha + depth
    /// support), mirroring the source dialog's per-format validation.
    fn seq_common_options(&self, fmt: SequenceFormat) -> Vec<EncodeOption> {
        let seq = &self.sequence_settings;
        vec![
            EncodeOption::choice(
                "channels",
                "Channels",
                seq_channel_labels(fmt),
                seq_channel_to_idx(fmt, seq.channels),
            ),
            EncodeOption::choice(
                "bitdepth",
                "Bit Depth",
                seq_depth_labels(fmt),
                seq_depth_to_idx(fmt, seq.bit_depth),
            ),
            EncodeOption::boolean("tonemap", "Tonemapping", seq.apply_tonemap),
            EncodeOption::choice(
                "tonemap_mode",
                "Tonemap mode",
                TONEMAP_LABELS,
                tonemap_to_idx(seq.tonemap_mode),
            ),
        ]
    }

    /// Build the encode schema mirroring playa's codec/format tables 1:1, seeding
    /// every option default from the current model and codec availability from the
    /// ffmpeg encoder probe.
    fn build_schema(&self) -> EncodeSchema {
        let cs = &self.codec_settings;

        // --- Video: MP4 (H.264 / H.265 / AV1) ---
        let mp4 = Format::new(
            "mp4",
            "MP4",
            "mp4",
            [
                Codec::new(
                    "h264",
                    "H.264",
                    [
                        EncodeOption::float("fps", "Framerate", self.fps as f64, 1.0, 960.0),
                        EncodeOption::choice(
                            "impl",
                            "Encoder",
                            ENC_IMPL_LABELS,
                            enc_impl_to_idx(cs.h264.encoder_impl),
                        ),
                        EncodeOption::choice(
                            "qmode",
                            "Quality Mode",
                            QMODE_LABELS,
                            qmode_to_idx(cs.h264.quality_mode),
                        ),
                        EncodeOption::int("value", "Value", cs.h264.quality_value as i64, 1, 10000),
                        EncodeOption::choice(
                            "preset",
                            "Preset",
                            H26X_PRESETS,
                            list_idx(&H26X_PRESETS, &cs.h264.preset, 5),
                        ),
                        EncodeOption::choice(
                            "profile",
                            "Profile",
                            H264_PROFILES,
                            list_idx(&H264_PROFILES, &cs.h264.profile, 2),
                        ),
                    ],
                )
                .available(VideoCodec::H264.is_available())
                .hint("18=best, 23=default, 28=fast"),
                Codec::new(
                    "h265",
                    "H.265 (HEVC)",
                    [
                        EncodeOption::float("fps", "Framerate", self.fps as f64, 1.0, 960.0),
                        EncodeOption::choice(
                            "impl",
                            "Encoder",
                            ENC_IMPL_LABELS,
                            enc_impl_to_idx(cs.h265.encoder_impl),
                        ),
                        EncodeOption::choice(
                            "qmode",
                            "Quality Mode",
                            QMODE_LABELS,
                            qmode_to_idx(cs.h265.quality_mode),
                        ),
                        EncodeOption::int("value", "Value", cs.h265.quality_value as i64, 1, 10000),
                        EncodeOption::choice(
                            "preset",
                            "Preset",
                            H26X_PRESETS,
                            list_idx(&H26X_PRESETS, &cs.h265.preset, 5),
                        ),
                        EncodeOption::choice(
                            "profile",
                            "Profile",
                            H265_PROFILES,
                            list_idx(&H265_PROFILES, &cs.h265.profile, 0),
                        ),
                    ],
                )
                .available(VideoCodec::H265.is_available())
                .hint("28=default (higher than H.264)"),
                Codec::new(
                    "av1",
                    "AV1",
                    [
                        EncodeOption::float("fps", "Framerate", self.fps as f64, 1.0, 960.0),
                        EncodeOption::choice(
                            "impl",
                            "Encoder",
                            ENC_IMPL_LABELS,
                            enc_impl_to_idx(cs.av1.encoder_impl),
                        ),
                        EncodeOption::choice(
                            "qmode",
                            "Quality Mode",
                            QMODE_LABELS,
                            qmode_to_idx(cs.av1.quality_mode),
                        ),
                        EncodeOption::int("value", "Value", cs.av1.quality_value as i64, 0, 10000),
                        EncodeOption::choice(
                            "preset",
                            "Preset",
                            AV1_PRESETS,
                            list_idx(&AV1_PRESETS, &cs.av1.preset, 17),
                        ),
                    ],
                )
                .available(VideoCodec::AV1.is_available())
                .hint("AV1: Best compression, slower encoding. HW: RTX 40xx/Arc/RDNA 3"),
            ],
        );

        // --- Video: MOV (ProRes) ---
        let mov = Format::new(
            "mov",
            "MOV",
            "mov",
            [Codec::new(
                "prores",
                "ProRes",
                [
                    EncodeOption::float("fps", "Framerate", self.fps as f64, 1.0, 960.0),
                    EncodeOption::choice(
                        "profile",
                        "Profile",
                        prores_labels(),
                        prores_idx(cs.prores.profile),
                    ),
                ],
            )
            .available(VideoCodec::ProRes.is_available())
            .hint("ProRes is always software-encoded (prores_ks)")],
        );

        // --- Image sequence formats (each its own widget Format/extension) ---
        let seq = &self.sequence_settings;

        let exr = Format::new(
            "exr",
            "EXR",
            "exr",
            [Codec::new("exr", "EXR", {
                let mut o = self.seq_common_options(SequenceFormat::Exr);
                o.extend([
                    EncodeOption::choice(
                        "mode",
                        "Mode",
                        EXR_MODE_LABELS,
                        exr_mode_idx(seq.format_settings.exr.mode),
                    ),
                    EncodeOption::choice(
                        "compression",
                        "Compression",
                        exr_comp_labels(),
                        exr_comp_idx(seq.format_settings.exr.compression),
                    ),
                    EncodeOption::float(
                        "dwa",
                        "DWA loss level",
                        seq.format_settings.exr.dwa_quality as f64,
                        0.0,
                        200.0,
                    ),
                ]);
                o
            })
            .hint("EXR: HDR format, preserves full dynamic range")],
        );

        let png = Format::new(
            "png",
            "PNG",
            "png",
            [Codec::new("png", "PNG", {
                let mut o = self.seq_common_options(SequenceFormat::Png);
                o.push(EncodeOption::int(
                    "compression",
                    "Compression",
                    seq.format_settings.png.compression as i64,
                    0,
                    9,
                ));
                o
            })
            .hint("PNG: Lossless, good for compositing")],
        );

        let jpeg = Format::new(
            "jpeg",
            "JPEG",
            "jpg",
            [Codec::new("jpeg", "JPEG", {
                let mut o = self.seq_common_options(SequenceFormat::Jpeg);
                o.push(EncodeOption::int(
                    "quality",
                    "Quality",
                    seq.format_settings.jpeg.quality as i64,
                    1,
                    100,
                ));
                o
            })
            .hint("JPEG: Lossy, small files, no alpha")],
        );

        let tiff = Format::new(
            "tiff",
            "TIFF",
            "tiff",
            [Codec::new("tiff", "TIFF", {
                let mut o = self.seq_common_options(SequenceFormat::Tiff);
                o.push(EncodeOption::choice(
                    "compression",
                    "Compression",
                    tiff_comp_labels(),
                    tiff_comp_idx(seq.format_settings.tiff.compression),
                ));
                o
            })
            .hint("TIFF: Industry standard, lossless")],
        );

        let tga = Format::new(
            "tga",
            "TGA",
            "tga",
            [Codec::new("tga", "TGA", {
                let mut o = self.seq_common_options(SequenceFormat::Tga);
                o.push(EncodeOption::boolean(
                    "rle",
                    "RLE Compression",
                    seq.format_settings.tga.rle_compression,
                ));
                o
            })
            .hint("TGA: Legacy format, game industry")],
        );

        EncodeSchema::new([mp4, mov, exr, png, jpeg, tiff, tga])
    }

    /// Map the widget's chosen settings back onto the model. Each option id is the
    /// inverse of `build_schema`.
    fn apply_widget(&mut self, s: &WidgetSettings) {
        self.output_path = PathBuf::from(&s.output_path);
        // Choice index helper (defaults to 0 if absent / wrong type).
        let ci = |id: &str| s.get_choice(id).unwrap_or(0);

        match s.codec_id.as_str() {
            "h264" => {
                self.export_mode = ExportMode::Video;
                self.selected_codec = VideoCodec::H264;
                self.container = Container::MP4;
                self.fps = s.get_float("fps").unwrap_or(24.0) as f32;
                let c = &mut self.codec_settings.h264;
                c.encoder_impl = idx_to_enc_impl(ci("impl"));
                c.quality_mode = idx_to_qmode(ci("qmode"));
                c.quality_value = s.get_int("value").unwrap_or(23).max(0) as u32;
                c.preset = H26X_PRESETS.get(ci("preset")).copied().unwrap_or("medium").to_string();
                c.profile = H264_PROFILES.get(ci("profile")).copied().unwrap_or("high").to_string();
            }
            "h265" => {
                self.export_mode = ExportMode::Video;
                self.selected_codec = VideoCodec::H265;
                self.container = Container::MP4;
                self.fps = s.get_float("fps").unwrap_or(24.0) as f32;
                let c = &mut self.codec_settings.h265;
                c.encoder_impl = idx_to_enc_impl(ci("impl"));
                c.quality_mode = idx_to_qmode(ci("qmode"));
                c.quality_value = s.get_int("value").unwrap_or(28).max(0) as u32;
                c.preset = H26X_PRESETS.get(ci("preset")).copied().unwrap_or("medium").to_string();
                c.profile = H265_PROFILES.get(ci("profile")).copied().unwrap_or("main").to_string();
            }
            "av1" => {
                self.export_mode = ExportMode::Video;
                self.selected_codec = VideoCodec::AV1;
                self.container = Container::MP4;
                self.fps = s.get_float("fps").unwrap_or(24.0) as f32;
                let c = &mut self.codec_settings.av1;
                c.encoder_impl = idx_to_enc_impl(ci("impl"));
                c.quality_mode = idx_to_qmode(ci("qmode"));
                c.quality_value = s.get_int("value").unwrap_or(30).max(0) as u32;
                c.preset = AV1_PRESETS.get(ci("preset")).copied().unwrap_or("p4").to_string();
            }
            "prores" => {
                self.export_mode = ExportMode::Video;
                self.selected_codec = VideoCodec::ProRes;
                self.container = Container::MOV;
                self.fps = s.get_float("fps").unwrap_or(24.0) as f32;
                self.codec_settings.prores.profile = ProResProfile::all()
                    .get(ci("profile"))
                    .copied()
                    .unwrap_or(ProResProfile::Standard);
            }
            "exr" => {
                self.export_mode = ExportMode::Sequence;
                self.apply_seq_common(s, SequenceFormat::Exr);
                let e = &mut self.sequence_settings.format_settings.exr;
                e.mode = idx_to_exr_mode(ci("mode"));
                e.compression = ExrCompression::all()
                    .get(ci("compression"))
                    .copied()
                    .unwrap_or(ExrCompression::Zip);
                e.dwa_quality = s.get_float("dwa").unwrap_or(45.0) as f32;
                self.sequence_settings.format = SequenceFormat::Exr;
                self.sequence_settings.validate();
            }
            "png" => {
                self.export_mode = ExportMode::Sequence;
                self.apply_seq_common(s, SequenceFormat::Png);
                self.sequence_settings.format_settings.png.compression =
                    s.get_int("compression").unwrap_or(6).clamp(0, 9) as u8;
                self.sequence_settings.format = SequenceFormat::Png;
                self.sequence_settings.validate();
            }
            "jpeg" => {
                self.export_mode = ExportMode::Sequence;
                self.apply_seq_common(s, SequenceFormat::Jpeg);
                self.sequence_settings.format_settings.jpeg.quality =
                    s.get_int("quality").unwrap_or(90).clamp(1, 100) as u8;
                self.sequence_settings.format = SequenceFormat::Jpeg;
                self.sequence_settings.validate();
            }
            "tiff" => {
                self.export_mode = ExportMode::Sequence;
                self.apply_seq_common(s, SequenceFormat::Tiff);
                let depth = self.sequence_settings.bit_depth;
                let t = &mut self.sequence_settings.format_settings.tiff;
                t.compression = TiffCompression::all()
                    .get(ci("compression"))
                    .copied()
                    .unwrap_or(TiffCompression::Lzw);
                t.bit_depth = if matches!(depth, OutputBitDepth::U16) {
                    TiffBitDepth::Sixteen
                } else {
                    TiffBitDepth::Eight
                };
                self.sequence_settings.format = SequenceFormat::Tiff;
                self.sequence_settings.validate();
            }
            "tga" => {
                self.export_mode = ExportMode::Sequence;
                self.apply_seq_common(s, SequenceFormat::Tga);
                self.sequence_settings.format_settings.tga.rle_compression =
                    s.get_bool("rle").unwrap_or(true);
                self.sequence_settings.format = SequenceFormat::Tga;
                self.sequence_settings.validate();
            }
            _ => {}
        }
    }

    /// Apply the shared sequence options (channels / depth / tonemapping) for a format.
    fn apply_seq_common(&mut self, s: &WidgetSettings, fmt: SequenceFormat) {
        let seq = &mut self.sequence_settings;
        seq.channels = idx_to_channel(fmt, s.get_choice("channels").unwrap_or(0));
        seq.bit_depth = idx_to_seq_depth(fmt, s.get_choice("bitdepth").unwrap_or(0));
        seq.apply_tonemap = s.get_bool("tonemap").unwrap_or(false);
        seq.tonemap_mode = idx_to_tonemap(s.get_choice("tonemap_mode").unwrap_or(0));
    }

    // ===================================================================
    // Worker control (unchanged: spawns the existing encode workers)
    // ===================================================================

    /// Start encoding process
    fn start_encoding(&mut self, comp: &Comp, project: &Project) {
        info!("========== STARTING ENCODING ==========");
        info!("Export mode: {:?}", self.export_mode);

        // Reset state for new encoding
        self.cancel_flag.store(false, Ordering::Relaxed);
        self.progress = None; // Clear old progress

        // Create progress channel
        let (tx, rx) = channel();
        self.progress_rx = Some(rx);

        let cancel_flag_clone = Arc::clone(&self.cancel_flag);
        let comp_clone = comp.clone();
        let project_clone = project.clone();

        use std::thread;

        let handle = match self.export_mode {
            ExportMode::Video => {
                // Video encoding
                let settings = self.build_encoder_settings();
                info!(
                    "Codec: {:?}, Container: {:?}",
                    settings.codec, settings.container
                );
                info!("Settings: {:?}", settings);

                use crate::dialogs::encode::encode_comp;
                let settings_clone = settings;

                thread::spawn(move || {
                    info!("Video encoder thread started");
                    encode_comp(
                        &comp_clone,
                        &project_clone,
                        &settings_clone,
                        tx,
                        cancel_flag_clone,
                    )
                })
            }
            ExportMode::Sequence => {
                // Image sequence export
                let settings = self.sequence_settings.clone();
                let output_path = self.output_path.clone();
                info!(
                    "Format: {:?}, Channels: {:?}",
                    settings.format, settings.channels
                );
                info!("Output: {}", output_path.display());

                use crate::dialogs::encode::encode_image_sequence;

                thread::spawn(move || {
                    info!("Image sequence export thread started");
                    encode_image_sequence(
                        &comp_clone,
                        &project_clone,
                        &output_path,
                        &settings,
                        tx,
                        cancel_flag_clone,
                    )
                })
            }
        };

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

    /// Internal: Stop encoding — non-blocking, no UI freeze.
    fn stop_encoding_internal(&mut self) {
        self.cancel_flag.store(true, Ordering::Relaxed);

        // Clean up any previously orphaned threads that have finished
        self.cleanup_orphan_handles();

        // Don't block the UI thread waiting for the encode thread to stop.
        // The cancel_flag is already set; push the handle to orphans so
        // cleanup_orphan_handles() will reap it on the next UI tick.
        if let Some(handle) = self.encode_thread.take() {
            self.orphan_handles.push(handle);
        }

        // Force reset to clean state
        self.reset_encoding_state();
        self.progress = None;
        self.cancel_flag = Arc::new(AtomicBool::new(false));
    }

    /// Clean up finished orphan thread handles
    fn cleanup_orphan_handles(&mut self) {
        // Retain only handles that are still running
        let mut finished_count = 0;
        self.orphan_handles.retain(|handle| {
            if handle.is_finished() {
                finished_count += 1;
                false // Remove from vec, will be dropped and joined
            } else {
                true // Keep in vec
            }
        });
        if finished_count > 0 {
            info!("Cleaned up {} orphaned encode thread(s)", finished_count);
        }
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
}

impl Drop for EncodeDialog {
    fn drop(&mut self) {
        // Join any orphaned encode threads on dialog close
        for handle in self.orphan_handles.drain(..) {
            if let Err(e) = handle.join() {
                info!("Orphaned encode thread panicked during cleanup: {:?}", e);
            }
        }
        // Also join the active thread if any
        if let Some(handle) = self.encode_thread.take()
            && let Err(e) = handle.join()
        {
            info!("Encode thread panicked during dialog close: {:?}", e);
        }
    }
}

// ========================================================================
// Schema option label tables + index mappers (model <-> widget choice index)
// ========================================================================

/// Encoder-impl labels, indexed to match [`EncoderImpl`] order.
const ENC_IMPL_LABELS: [&str; 3] = ["Auto (HW → CPU)", "Hardware only", "Software (CPU)"];
/// Quality-mode labels, indexed to match [`QualityMode`] order.
const QMODE_LABELS: [&str; 2] = ["CRF (Quality)", "Bitrate (kbps)"];
/// Tonemap labels (order is fixed here, mapped explicitly — not enum order).
const TONEMAP_LABELS: [&str; 3] = ["ACES", "Reinhard", "Clamp"];
/// H.264/H.265 preset union (libx26x ladder + NVENC/QSV/AMF presets). Single list
/// because the widget can't vary a choice list by another option; the chosen
/// string is what the encoder consumes.
const H26X_PRESETS: [&str; 18] = [
    "ultrafast", "superfast", "veryfast", "faster", "fast", "medium", "slow", "slower", "veryslow",
    "placebo", "default", "p1", "p2", "p3", "p4", "p5", "p6", "p7",
];
const H264_PROFILES: [&str; 6] = ["baseline", "main", "high", "high10", "high422", "high444"];
const H265_PROFILES: [&str; 2] = ["main", "main10"];
/// AV1 preset union (SVT-AV1/libaom 0..13 + NVENC/QSV/AMF p1..p7 + named). "p4" at idx 17.
const AV1_PRESETS: [&str; 25] = [
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "p1", "p2", "p3",
    "p4", "p5", "p6", "p7", "default", "slow", "medium", "fast",
];
const EXR_MODE_LABELS: [&str; 2] = [
    "Display only (single RGBA)",
    "Pass-through (preserve all layers)",
];

fn enc_impl_to_idx(v: EncoderImpl) -> usize {
    match v {
        EncoderImpl::Auto => 0,
        EncoderImpl::Hardware => 1,
        EncoderImpl::Software => 2,
    }
}
fn idx_to_enc_impl(i: usize) -> EncoderImpl {
    match i {
        1 => EncoderImpl::Hardware,
        2 => EncoderImpl::Software,
        _ => EncoderImpl::Auto,
    }
}

fn qmode_to_idx(v: QualityMode) -> usize {
    match v {
        QualityMode::CRF => 0,
        QualityMode::Bitrate => 1,
    }
}
fn idx_to_qmode(i: usize) -> QualityMode {
    match i {
        1 => QualityMode::Bitrate,
        _ => QualityMode::CRF,
    }
}

fn tonemap_to_idx(v: TonemapMode) -> usize {
    match v {
        TonemapMode::ACES => 0,
        TonemapMode::Reinhard => 1,
        TonemapMode::Clamp => 2,
    }
}
fn idx_to_tonemap(i: usize) -> TonemapMode {
    match i {
        1 => TonemapMode::Reinhard,
        2 => TonemapMode::Clamp,
        _ => TonemapMode::ACES,
    }
}

fn exr_mode_idx(m: ExrEncodeMode) -> usize {
    match m {
        ExrEncodeMode::DisplayOnly => 0,
        ExrEncodeMode::PassThrough => 1,
    }
}
fn idx_to_exr_mode(i: usize) -> ExrEncodeMode {
    match i {
        1 => ExrEncodeMode::PassThrough,
        _ => ExrEncodeMode::DisplayOnly,
    }
}

/// Index of `val` in `list`, or `fallback` when absent.
fn list_idx(list: &[&str], val: &str, fallback: usize) -> usize {
    list.iter().position(|&s| s == val).unwrap_or(fallback)
}

fn prores_labels() -> Vec<String> {
    ProResProfile::all().iter().map(|p| p.to_string()).collect()
}
fn prores_idx(p: ProResProfile) -> usize {
    ProResProfile::all()
        .iter()
        .position(|&x| x == p)
        .unwrap_or(2)
}

fn exr_comp_labels() -> Vec<String> {
    ExrCompression::all().iter().map(|c| c.to_string()).collect()
}
fn exr_comp_idx(c: ExrCompression) -> usize {
    ExrCompression::all()
        .iter()
        .position(|&x| x == c)
        .unwrap_or(3) // ZIP
}

fn tiff_comp_labels() -> Vec<String> {
    TiffCompression::all().iter().map(|c| c.to_string()).collect()
}
fn tiff_comp_idx(c: TiffCompression) -> usize {
    TiffCompression::all()
        .iter()
        .position(|&x| x == c)
        .unwrap_or(1) // LZW
}

fn seq_channel_labels(fmt: SequenceFormat) -> Vec<&'static str> {
    if fmt.supports_alpha() {
        vec!["RGB", "RGBA"]
    } else {
        vec!["RGB"]
    }
}
fn seq_channel_to_idx(fmt: SequenceFormat, ch: ChannelMode) -> usize {
    if fmt.supports_alpha() && matches!(ch, ChannelMode::Rgba) {
        1
    } else {
        0
    }
}
fn idx_to_channel(fmt: SequenceFormat, i: usize) -> ChannelMode {
    if fmt.supports_alpha() && i == 1 {
        ChannelMode::Rgba
    } else {
        ChannelMode::Rgb
    }
}

fn seq_depth_labels(fmt: SequenceFormat) -> Vec<String> {
    fmt.capabilities()
        .supported_depths
        .iter()
        .map(|d| d.to_string())
        .collect()
}
fn seq_depth_to_idx(fmt: SequenceFormat, d: OutputBitDepth) -> usize {
    fmt.capabilities()
        .supported_depths
        .iter()
        .position(|&x| x == d)
        .unwrap_or(0)
}
fn idx_to_seq_depth(fmt: SequenceFormat, i: usize) -> OutputBitDepth {
    let ds = fmt.capabilities().supported_depths;
    ds.get(i).copied().unwrap_or(ds[0])
}
