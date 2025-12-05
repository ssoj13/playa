//! Preferences/settings events.

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

/// Hotkey window context for context-aware shortcuts
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HotkeyWindow {
    Global,
    Viewport,
    Timeline,
    Project,
}
