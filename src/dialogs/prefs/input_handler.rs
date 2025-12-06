//! Hotkey system - keyboard shortcuts management

use crate::dialogs::prefs::prefs_events::HotkeyWindow;
use crate::core::event_bus::BoxedEvent;
use crate::core::player_events::*;
use crate::core::project_events::*;
use crate::entities::comp_events::*;
use crate::widgets::timeline::timeline_events::*;
use crate::widgets::viewport::viewport_events::*;
use crate::dialogs::prefs::prefs_events::*;
use eframe::egui;
use std::collections::HashMap;
use uuid::Uuid;

/// Factory function type for creating events
type EventFactory = Box<dyn Fn() -> BoxedEvent + Send + Sync>;

/// Hotkey handler for managing keyboard shortcuts
pub struct HotkeyHandler {
    bindings: HashMap<(HotkeyWindow, String), EventFactory>,
    focused_window: HotkeyWindow,
}

impl HotkeyHandler {
    /// Create new hotkey handler
    pub fn new() -> Self {
        Self {
            bindings: HashMap::new(),
            focused_window: HotkeyWindow::Global,
        }
    }

    /// Handle key press, returns cloned event
    pub fn handle_key(&self, key: &str) -> Option<BoxedEvent> {
        // Try current focused window first
        if let Some(factory) = self.bindings.get(&(self.focused_window, key.to_string())) {
            return Some(factory());
        }
        // Fallback: try Global
        if self.focused_window != HotkeyWindow::Global {
            if let Some(factory) = self.bindings.get(&(HotkeyWindow::Global, key.to_string())) {
                return Some(factory());
            }
        }
        None
    }

    /// Handle key with modifiers
    pub fn handle_key_with_modifiers(
        &self,
        key: &str,
        ctrl: bool,
        shift: bool,
        alt: bool,
    ) -> Option<BoxedEvent> {
        let mut key_combo = String::new();
        if ctrl { key_combo.push_str("Ctrl+"); }
        if shift { key_combo.push_str("Shift+"); }
        if alt { key_combo.push_str("Alt+"); }
        key_combo.push_str(key);
        self.handle_key(&key_combo)
    }

    /// Set focused window context
    pub fn set_focused_window(&mut self, window: HotkeyWindow) {
        self.focused_window = window;
    }

    /// Add hotkey binding with factory
    fn bind<E: Clone + Send + Sync + 'static>(&mut self, window: HotkeyWindow, key: &str, event: E) {
        let factory: EventFactory = Box::new(move || Box::new(event.clone()));
        self.bindings.insert((window, key.to_string()), factory);
    }

    /// Setup default hotkey bindings
    pub fn setup_default_bindings(&mut self) {
        use HotkeyWindow::*;

        // Global hotkeys
        self.bind(Global, "F1", ToggleHelpEvent);
        self.bind(Global, "F2", TogglePlaylistEvent);
        self.bind(Global, "F3", ToggleAttributeEditorEvent);
        self.bind(Global, "F4", ToggleEncodeDialogEvent);
        self.bind(Global, "F5", ToggleSettingsEvent);
        self.bind(Global, "Space", TogglePlayPauseEvent);
        self.bind(Global, "ArrowUp", TogglePlayPauseEvent);
        self.bind(Global, "K", StopEvent);
        self.bind(Global, "Period", StopEvent);
        self.bind(Global, "Num1", JumpToStartEvent);
        self.bind(Global, "Home", JumpToStartEvent);
        self.bind(Global, "Num2", JumpToEndEvent);
        self.bind(Global, "End", JumpToEndEvent);
        self.bind(Global, "PageDown", StepForwardEvent);
        self.bind(Global, "Shift+PageDown", StepForwardLargeEvent);
        self.bind(Global, "PageUp", StepBackwardEvent);
        self.bind(Global, "Shift+PageUp", StepBackwardLargeEvent);
        self.bind(Global, "Ctrl+PageDown", JumpToEndEvent);
        self.bind(Global, "Ctrl+PageUp", JumpToStartEvent);
        self.bind(Global, "-", DecreaseFPSBaseEvent);
        self.bind(Global, "Equals", IncreaseFPSBaseEvent);
        self.bind(Global, "Plus", IncreaseFPSBaseEvent);
        self.bind(Global, "Shift+ArrowLeft", StepBackwardLargeEvent);
        self.bind(Global, "Shift+ArrowRight", StepForwardLargeEvent);
        self.bind(Global, "ArrowLeft", StepBackwardEvent);
        self.bind(Global, "ArrowRight", StepForwardEvent);
        self.bind(Global, "ArrowDown", StopEvent);
        self.bind(Global, "J", JogBackwardEvent);
        self.bind(Global, "Comma", JogBackwardEvent);
        self.bind(Global, "L", JogForwardEvent);
        self.bind(Global, "Slash", JogForwardEvent);
        self.bind(Global, "Semicolon", JumpToPrevEdgeEvent);
        self.bind(Global, "Quote", JumpToNextEdgeEvent);
        self.bind(Global, "Backtick", ToggleLoopEvent);
        self.bind(Global, "Backspace", ToggleFrameNumbersEvent);
        self.bind(Global, "B", SetPlayRangeStartEvent);
        self.bind(Global, "N", SetPlayRangeEndEvent);
        self.bind(Global, "Ctrl+B", ResetPlayRangeEvent);
        self.bind(Global, "Ctrl+ArrowLeft", JumpToStartEvent);
        self.bind(Global, "Ctrl+ArrowRight", JumpToEndEvent);
        self.bind(Global, "Ctrl+R", ResetSettingsEvent);
        self.bind(Global, "Ctrl+S", QuickSaveEvent);
        self.bind(Global, "Ctrl+O", OpenProjectDialogEvent);
        self.bind(Global, "Z", ToggleFullscreenEvent);
        self.bind(Global, "U", ProjectPreviousCompEvent);
        self.bind(Global, "F", FitViewportEvent);
        self.bind(Global, "A", Viewport100Event);
        self.bind(Global, "H", Viewport100Event);

        // Timeline-specific
        self.bind(Timeline, "Delete", RemoveSelectedLayerEvent);
        self.bind(Timeline, "F", TimelineFitEvent);
        self.bind(Timeline, "A", TimelineResetZoomEvent);
        self.bind(Timeline, "OpenBracket", AlignLayersStartEvent(Uuid::nil()));
        self.bind(Timeline, "CloseBracket", AlignLayersEndEvent(Uuid::nil()));
        self.bind(Timeline, "Alt+OpenBracket", TrimLayersStartEvent(Uuid::nil()));
        self.bind(Timeline, "Alt+CloseBracket", TrimLayersEndEvent(Uuid::nil()));

        // Project-specific
        self.bind(Project, "Delete", RemoveSelectedMediaEvent);

        // Viewport-specific
        self.bind(Viewport, "F", FitViewportEvent);
        self.bind(Viewport, "A", Viewport100Event);
        self.bind(Viewport, "H", Viewport100Event);
    }

    /// Handle keyboard input
    pub fn handle_input(&self, input: &egui::InputState) -> Option<BoxedEvent> {
        for event in &input.events {
            if let egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = event
            {
                let key_str = format!("{:?}", key);
                if let Some(ev) = self.handle_key_with_modifiers(
                    &key_str,
                    modifiers.ctrl,
                    modifiers.shift,
                    modifiers.alt,
                ) {
                    return Some(ev);
                }
                if !modifiers.any() {
                    if let Some(ev) = self.handle_key(&key_str) {
                        return Some(ev);
                    }
                }
            }
        }
        None
    }
}
