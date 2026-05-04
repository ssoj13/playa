//! Preferences / settings payload + events.
//!
//! [`GizmoPrefs`] is stored inside project JSON; defined here so UI and engine
//! share one type without coupling widgets to `entities::project` internals.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GizmoPrefs {
    pub pref_manip_size: f32,
    pub pref_manip_stroke_width: f32,
    pub pref_manip_inactive_alpha: f32,
    pub pref_manip_highlight_alpha: f32,
}

impl Default for GizmoPrefs {
    fn default() -> Self {
        Self {
            pref_manip_size: 128.0,
            pref_manip_stroke_width: 5.0,
            pref_manip_inactive_alpha: 0.7,
            pref_manip_highlight_alpha: 1.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ResetSettingsEvent;

#[derive(Clone, Debug)]
pub struct TogglePlaylistEvent;

#[derive(Clone, Debug)]
pub struct ToggleHelpEvent;

#[derive(Clone, Debug)]
pub struct ToggleAttributeEditorEvent;

#[derive(Clone, Debug)]
pub struct ToggleSettingsEvent;

#[derive(Clone, Debug)]
pub struct ToggleEncodeDialogEvent;

#[derive(Clone, Debug)]
pub struct ToggleFullscreenEvent;

#[derive(Clone, Debug)]
pub struct ToggleFrameNumbersEvent;

#[derive(Clone, Debug)]
pub struct SetGizmoPrefsEvent(pub GizmoPrefs);

/// CPU vs GPU compositor backend (persisted via AppSettings).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, Default,
)]
pub enum CompositorBackend {
    #[default]
    Cpu,
    Gpu,
}

/// Backend selection changed from Settings UI — applied on next tick in `run.rs`.
#[derive(Debug, Clone)]
pub struct CompositorBackendChangedEvent {
    pub backend: CompositorBackend,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HotkeyWindow {
    Global,
    Viewport,
    Timeline,
    Project,
    NodeEditor,
}
