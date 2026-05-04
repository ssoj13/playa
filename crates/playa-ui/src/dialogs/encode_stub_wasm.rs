//! Encode dialog omitted on Wasm (no FFmpeg / EXR exporters). Persisted dialog settings degrade
//! to an empty object — unknown JSON fields deserialize losslessly via serde defaults.

use eframe::egui;
use playa_engine::entities::{Comp as CompNode, Project};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EncodeDialogSettings {}

#[derive(Clone, Debug)]
pub struct EncodeDialog;

impl EncodeDialog {
    pub fn load_from_settings(_s: &EncodeDialogSettings) -> Self {
        Self
    }

    pub fn save_to_settings(&self) -> EncodeDialogSettings {
        EncodeDialogSettings::default()
    }

    pub fn render(
        &mut self,
        _ctx: &egui::Context,
        _project: &Project,
        _active_comp: Option<&CompNode>,
    ) -> bool {
        false
    }

    pub fn stop_encoding(&mut self) {}
}
