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

impl Default for HotkeyHandler {
    fn default() -> Self {
        Self::new()
    }
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
        self.bind(Global, "F12", ToggleSettingsEvent);
        self.bind(Global, "Space", TogglePlayPauseEvent);
        self.bind(Global, "Insert", TogglePlayPauseEvent);  // KP_Ins / Insert
        self.bind(Global, "ArrowUp", TogglePlayPauseEvent);
        self.bind(Global, "K", StopEvent);
        self.bind(Global, "Slash", StopEvent);        // / = K (stop)
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
        // FPS control: both regular and numpad +/-
        self.bind(Global, "Minus", DecreaseFPSBaseEvent);
        self.bind(Global, "Equals", IncreaseFPSBaseEvent);
        self.bind(Global, "Plus", IncreaseFPSBaseEvent);
        self.bind(Global, "Shift+ArrowLeft", StepBackwardLargeEvent);
        self.bind(Global, "Shift+ArrowRight", StepForwardLargeEvent);
        self.bind(Global, "ArrowLeft", StepBackwardEvent);
        self.bind(Global, "ArrowRight", StepForwardEvent);
        self.bind(Global, "ArrowDown", StopEvent);
        // J/K/L style: < = J, / = K, > = L
        self.bind(Global, "J", JogBackwardEvent);
        self.bind(Global, "Comma", JogBackwardEvent);  // , or < = J (jog back)
        self.bind(Global, "L", JogForwardEvent);
        self.bind(Global, "Period", JogForwardEvent);  // . or > = L (jog forward)
        self.bind(Global, "Semicolon", JumpToPrevEdgeEvent);
        self.bind(Global, "Quote", JumpToNextEdgeEvent);
        self.bind(Global, "Backtick", ToggleLoopEvent);
        self.bind(Global, "Backspace", ToggleFrameNumbersEvent);
        self.bind(Global, "B", SetPlayRangeStartEvent);
        self.bind(Global, "N", SetPlayRangeEndEvent);
        self.bind(Global, "Ctrl+B", ResetPlayRangeEvent);
        self.bind(Global, "Ctrl+ArrowLeft", JumpToStartEvent);
        self.bind(Global, "Ctrl+ArrowRight", JumpToEndEvent);
        // Ctrl+R is now ResetTrimsEvent in Timeline context (see below)
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
        // Layer clipboard operations
        self.bind(Timeline, "Ctrl+D", DuplicateLayersEvent { comp_uuid: Uuid::nil() });
        self.bind(Timeline, "Ctrl+C", CopyLayersEvent { comp_uuid: Uuid::nil() });
        self.bind(Timeline, "Ctrl+V", PasteLayersEvent { comp_uuid: Uuid::nil(), target_frame: 0 });
        // Selection operations
        self.bind(Timeline, "Ctrl+A", SelectAllLayersEvent { comp_uuid: Uuid::nil() });
        self.bind(Timeline, "F2", ClearLayerSelectionEvent { comp_uuid: Uuid::nil() }); // Overrides global F2 in timeline
        // Trim operations
        self.bind(Timeline, "Ctrl+R", ResetTrimsEvent { comp_uuid: Uuid::nil() });

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
            // Handle egui's semantic Copy/Cut/Paste events (Ctrl+C/X/V are converted to these)
            match event {
                egui::Event::Copy => {
                    log::debug!("Event::Copy (window={:?})", self.focused_window);
                    if let Some(ev) = self.handle_key("Ctrl+C") {
                        return Some(ev);
                    }
                }
                egui::Event::Cut => {
                    log::debug!("Event::Cut (window={:?})", self.focused_window);
                    if let Some(ev) = self.handle_key("Ctrl+X") {
                        return Some(ev);
                    }
                }
                egui::Event::Paste(_) => {
                    log::debug!("Event::Paste (window={:?})", self.focused_window);
                    if let Some(ev) = self.handle_key("Ctrl+V") {
                        return Some(ev);
                    }
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    let key_str = format!("{:?}", key);

                    // Build combo string for debug
                    let mut combo = String::new();
                    if modifiers.ctrl { combo.push_str("Ctrl+"); }
                    if modifiers.shift { combo.push_str("Shift+"); }
                    if modifiers.alt { combo.push_str("Alt+"); }
                    combo.push_str(&key_str);

                    if let Some(ev) = self.handle_key_with_modifiers(
                        &key_str,
                        modifiers.ctrl,
                        modifiers.shift,
                        modifiers.alt,
                    ) {
                        log::debug!("Hotkey matched: {} (window={:?})", combo, self.focused_window);
                        return Some(ev);
                    }
                    if !modifiers.any() {
                        if let Some(ev) = self.handle_key(&key_str) {
                            log::debug!("Hotkey matched (no mod): {} (window={:?})", key_str, self.focused_window);
                            return Some(ev);
                        }
                    }
                }
                _ => {}
            }
        }
        None
    }
}
