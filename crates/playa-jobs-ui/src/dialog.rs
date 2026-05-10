//! [`SubmitDialog`] — modal for composing one Seedance generation request
//! (text-to-video or image-to-video) with live cost preview.

use egui::{Context, Window};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SubmitEndpoint {
    #[default]
    TextToVideo,
    ImageToVideo,
}

impl SubmitEndpoint {
    /// fal.ai per-second pricing (standard tier, 720p reference).
    pub fn cost_per_sec_usd(self) -> f64 {
        match self {
            Self::ImageToVideo => 0.3024,
            Self::TextToVideo => 0.3034,
        }
    }

    pub fn kind(self) -> &'static str {
        match self {
            Self::ImageToVideo => "seedance.image_to_video",
            Self::TextToVideo => "seedance.text_to_video",
        }
    }
}

/// Stateful modal dialog. Caller constructs once, calls
/// [`SubmitDialog::open`] to make it visible, then [`SubmitDialog::show`]
/// every frame to render and harvest user actions via
/// [`SubmitDialogResult`].
#[derive(Debug, Clone)]
pub struct SubmitDialog {
    pub open: bool,
    pub endpoint: SubmitEndpoint,
    pub prompt: String,
    pub image_url: String,
    pub resolution: String,
    pub duration_secs: u8,
    pub aspect_ratio: String,
    pub generate_audio: bool,
    pub seed_text: String,
    pub auto_attach: bool,
}

impl Default for SubmitDialog {
    fn default() -> Self {
        Self {
            open: false,
            endpoint: SubmitEndpoint::TextToVideo,
            prompt: String::new(),
            image_url: String::new(),
            resolution: "480p".to_string(),
            duration_secs: 4,
            aspect_ratio: "auto".to_string(),
            generate_audio: false,
            seed_text: String::new(),
            auto_attach: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SubmitDialogResult {
    /// No interaction this frame.
    None,
    /// User clicked Submit. Body suitable to pass to
    /// [`playa_jobs_core::JobQueue::submit`].
    Submit {
        kind: &'static str,
        params: Value,
        auto_attach: bool,
    },
    /// User clicked Cancel or `[x]`.
    Cancelled,
}

impl SubmitDialog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open(&mut self) {
        self.open = true;
    }

    /// Check if input is ready for Submit (caller can use this to gray out
    /// the Submit button server-side).
    pub fn is_valid(&self) -> bool {
        if self.prompt.trim().is_empty() {
            return false;
        }
        if self.endpoint == SubmitEndpoint::ImageToVideo && self.image_url.trim().is_empty() {
            return false;
        }
        if !(4..=15).contains(&self.duration_secs) {
            return false;
        }
        true
    }

    pub fn cost_estimate_usd(&self) -> f64 {
        self.endpoint.cost_per_sec_usd() * self.duration_secs as f64
    }

    /// Build the JSON params body for the chosen endpoint. Caller passes
    /// this to `JobQueue::submit(self.endpoint.kind(), params)`.
    pub fn build_params(&self) -> Value {
        let mut obj = serde_json::Map::new();
        obj.insert("prompt".into(), Value::String(self.prompt.clone()));
        obj.insert("resolution".into(), Value::String(self.resolution.clone()));
        match self.endpoint {
            SubmitEndpoint::ImageToVideo => {
                obj.insert("image_url".into(), Value::String(self.image_url.clone()));
                // i2v wants integer duration on the wire.
                obj.insert("duration".into(), Value::from(self.duration_secs));
            }
            SubmitEndpoint::TextToVideo => {
                // t2v wants string duration (per fal docs).
                obj.insert("duration".into(), Value::String(self.duration_secs.to_string()));
            }
        }
        if self.aspect_ratio != "auto" {
            obj.insert("aspect_ratio".into(), Value::String(self.aspect_ratio.clone()));
        }
        if self.generate_audio {
            obj.insert("generate_audio".into(), Value::Bool(true));
        }
        if let Ok(seed) = self.seed_text.trim().parse::<i64>() {
            obj.insert("seed".into(), Value::from(seed));
        }
        Value::Object(obj)
    }

    /// Render and harvest user action.
    pub fn show(&mut self, ctx: &Context) -> SubmitDialogResult {
        if !self.open {
            return SubmitDialogResult::None;
        }
        let mut result = SubmitDialogResult::None;
        let mut window_open = self.open;

        Window::new("Generate via Seedance")
            .open(&mut window_open)
            .resizable(true)
            .default_size([520.0, 460.0])
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Endpoint:");
                    ui.radio_value(
                        &mut self.endpoint,
                        SubmitEndpoint::TextToVideo,
                        "Text-to-Video",
                    );
                    ui.radio_value(
                        &mut self.endpoint,
                        SubmitEndpoint::ImageToVideo,
                        "Image-to-Video",
                    );
                });
                ui.add_space(8.0);

                ui.label("Prompt:");
                egui::ScrollArea::vertical()
                    .max_height(80.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.prompt)
                                .desired_rows(3)
                                .desired_width(f32::INFINITY),
                        );
                    });
                ui.add_space(4.0);

                if self.endpoint == SubmitEndpoint::ImageToVideo {
                    ui.horizontal(|ui| {
                        ui.label("Image URL:");
                        ui.text_edit_singleline(&mut self.image_url);
                    });
                    ui.add_space(4.0);
                }

                ui.horizontal(|ui| {
                    ui.label("Resolution:");
                    ui.radio_value(&mut self.resolution, "480p".into(), "480p");
                    ui.radio_value(&mut self.resolution, "720p".into(), "720p");
                    if self.endpoint == SubmitEndpoint::ImageToVideo {
                        ui.radio_value(&mut self.resolution, "1080p".into(), "1080p");
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("Duration:");
                    ui.add(egui::DragValue::new(&mut self.duration_secs).range(4..=15).suffix(" s"));
                    ui.label("Aspect:");
                    egui::ComboBox::from_id_salt("aspect_ratio")
                        .selected_text(&self.aspect_ratio)
                        .show_ui(ui, |ui| {
                            for ar in ["auto", "21:9", "16:9", "4:3", "1:1", "3:4", "9:16"] {
                                ui.selectable_value(&mut self.aspect_ratio, ar.to_string(), ar);
                            }
                        });
                });

                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.generate_audio, "Generate audio");
                    ui.label("Seed:");
                    ui.add(egui::TextEdit::singleline(&mut self.seed_text).desired_width(80.0));
                });

                ui.checkbox(&mut self.auto_attach, "Auto-attach mp4 to active comp on completion");
                ui.add_space(8.0);

                ui.separator();
                ui.label(format!(
                    "Estimated cost: ${:.2} USD ({} × {} s, standard tier)",
                    self.cost_estimate_usd(),
                    self.resolution,
                    self.duration_secs,
                ));

                ui.add_space(8.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let valid = self.is_valid();
                    if ui.add_enabled(valid, egui::Button::new("Submit")).clicked() {
                        result = SubmitDialogResult::Submit {
                            kind: self.endpoint.kind(),
                            params: self.build_params(),
                            auto_attach: self.auto_attach,
                        };
                    }
                    if ui.button("Cancel").clicked() {
                        result = SubmitDialogResult::Cancelled;
                    }
                });
            });

        if !window_open || matches!(result, SubmitDialogResult::Cancelled) {
            // [x] = Cancel.
            if matches!(result, SubmitDialogResult::None) {
                result = SubmitDialogResult::Cancelled;
            }
            self.open = false;
        }
        if matches!(result, SubmitDialogResult::Submit { .. }) {
            self.open = false;
        }
        result
    }
}

// =============================================================================
// Tests — focus on validation + params building (no egui rendering).
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_reasonable() {
        let d = SubmitDialog::default();
        assert!(!d.open);
        assert_eq!(d.endpoint, SubmitEndpoint::TextToVideo);
        assert_eq!(d.duration_secs, 4);
        assert_eq!(d.resolution, "480p");
        assert!(d.auto_attach);
    }

    #[test]
    fn invalid_when_prompt_empty() {
        let mut d = SubmitDialog::default();
        d.prompt = String::new();
        assert!(!d.is_valid());
        d.prompt = "  \n\t".into();
        assert!(!d.is_valid());
    }

    #[test]
    fn invalid_when_image_to_video_without_url() {
        let mut d = SubmitDialog::default();
        d.endpoint = SubmitEndpoint::ImageToVideo;
        d.prompt = "drift".into();
        d.image_url = "".into();
        assert!(!d.is_valid());
        d.image_url = "https://x/y.png".into();
        assert!(d.is_valid());
    }

    #[test]
    fn invalid_when_duration_out_of_range() {
        let mut d = SubmitDialog::default();
        d.prompt = "drift".into();
        d.duration_secs = 3;
        assert!(!d.is_valid());
        d.duration_secs = 16;
        assert!(!d.is_valid());
        d.duration_secs = 4;
        assert!(d.is_valid());
        d.duration_secs = 15;
        assert!(d.is_valid());
    }

    #[test]
    fn cost_estimate_matches_per_sec_pricing() {
        let mut d = SubmitDialog::default();
        d.endpoint = SubmitEndpoint::TextToVideo;
        d.duration_secs = 4;
        // 0.3034 * 4 ≈ 1.2136
        assert!((d.cost_estimate_usd() - 1.2136).abs() < 1e-9);

        d.endpoint = SubmitEndpoint::ImageToVideo;
        d.duration_secs = 10;
        // 0.3024 * 10 = 3.024
        assert!((d.cost_estimate_usd() - 3.024).abs() < 1e-9);
    }

    #[test]
    fn build_params_text_to_video_uses_string_duration() {
        let mut d = SubmitDialog::default();
        d.prompt = "cyberpunk".into();
        d.duration_secs = 7;
        let params = d.build_params();
        assert_eq!(params["prompt"], "cyberpunk");
        assert_eq!(params["resolution"], "480p");
        assert_eq!(params["duration"], "7"); // STRING per fal t2v spec
        assert!(params.get("image_url").is_none());
    }

    #[test]
    fn build_params_image_to_video_uses_integer_duration() {
        let mut d = SubmitDialog::default();
        d.endpoint = SubmitEndpoint::ImageToVideo;
        d.prompt = "drift".into();
        d.image_url = "https://x/y.png".into();
        d.duration_secs = 5;
        let params = d.build_params();
        assert_eq!(params["image_url"], "https://x/y.png");
        assert_eq!(params["duration"], 5); // INTEGER per fal i2v spec
    }

    #[test]
    fn build_params_omits_aspect_when_auto() {
        let mut d = SubmitDialog::default();
        d.prompt = "x".into();
        d.aspect_ratio = "auto".into();
        let params = d.build_params();
        assert!(params.get("aspect_ratio").is_none());

        d.aspect_ratio = "16:9".into();
        let params = d.build_params();
        assert_eq!(params["aspect_ratio"], "16:9");
    }

    #[test]
    fn build_params_includes_seed_only_when_parseable() {
        let mut d = SubmitDialog::default();
        d.prompt = "x".into();
        d.seed_text = "".into();
        assert!(d.build_params().get("seed").is_none());
        d.seed_text = "not a number".into();
        assert!(d.build_params().get("seed").is_none());
        d.seed_text = "42".into();
        assert_eq!(d.build_params()["seed"], 42);
    }

    #[test]
    fn build_params_omits_audio_when_off() {
        let mut d = SubmitDialog::default();
        d.prompt = "x".into();
        d.generate_audio = false;
        assert!(d.build_params().get("generate_audio").is_none());
        d.generate_audio = true;
        assert_eq!(d.build_params()["generate_audio"], true);
    }

    #[test]
    fn endpoint_kind_strings() {
        assert_eq!(SubmitEndpoint::ImageToVideo.kind(), "seedance.image_to_video");
        assert_eq!(SubmitEndpoint::TextToVideo.kind(), "seedance.text_to_video");
    }
}
