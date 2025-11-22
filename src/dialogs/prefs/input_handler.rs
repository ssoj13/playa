//! Hotkey system - keyboard shortcuts management

use crate::events::{AppEvent, HotkeyWindow};
use eframe::egui;
use std::collections::HashMap;

/// Hotkey handler for managing keyboard shortcuts
pub struct HotkeyHandler {
    bindings: HashMap<(HotkeyWindow, String), AppEvent>,
    focused_window: HotkeyWindow,
}

impl HotkeyHandler {
    /// Create new hotkey handler with default bindings
    pub fn new() -> Self {
        Self {
            bindings: HashMap::new(),
            focused_window: HotkeyWindow::Global,
        }
    }

    /// Handle key press
    pub fn handle_key(&self, key: &str) -> Option<AppEvent> {
        self.bindings
            .get(&(self.focused_window.clone(), key.to_string()))
            .cloned()
    }

    /// Handle key with modifiers
    pub fn handle_key_with_modifiers(
        &self,
        key: &str,
        ctrl: bool,
        shift: bool,
        alt: bool,
    ) -> Option<AppEvent> {
        let mut key_combo = String::new();
        if ctrl {
            key_combo.push_str("Ctrl+");
        }
        if shift {
            key_combo.push_str("Shift+");
        }
        if alt {
            key_combo.push_str("Alt+");
        }
        key_combo.push_str(key);

        self.handle_key(&key_combo)
    }

    /// Set focused window context
    pub fn set_focused_window(&mut self, window: HotkeyWindow) {
        self.focused_window = window;
    }

    /// Add hotkey binding
    pub fn add_binding(&mut self, window: HotkeyWindow, key: String, event: AppEvent) {
        self.bindings.insert((window, key), event);
    }

    /// Remove hotkey binding
    pub fn remove_binding(&mut self, window: HotkeyWindow, key: &str) {
        self.bindings.remove(&(window, key.to_string()));
    }

    /// Setup default hotkey bindings
    pub fn setup_default_bindings(&mut self) {
        use AppEvent::*;
        use HotkeyWindow::*;

        // Global hotkeys (работают везде)
        self.add_binding(Global, "F1".to_string(), ToggleHelp);
        self.add_binding(Global, "F2".to_string(), TogglePlaylist);
        self.add_binding(Global, "Space".to_string(), TogglePlayPause);
        self.add_binding(Global, "K".to_string(), Stop);
        self.add_binding(Global, ".".to_string(), Stop);

        // Timeline-specific hotkeys
        self.add_binding(Timeline, "Delete".to_string(), RemoveSelectedLayer);
        self.add_binding(Timeline, "F".to_string(), TimelineFit);
        self.add_binding(Timeline, "A".to_string(), TimelineResetZoom);

        // Viewport-specific hotkeys
        self.add_binding(Viewport, "F".to_string(), FitViewport);
        self.add_binding(Viewport, "A".to_string(), Viewport100);
        self.add_binding(Viewport, "H".to_string(), Viewport100);

        // TODO: добавить остальные hotkeys по мере необходимости
    }

    /// Handle keyboard input from egui with current focused window
    pub fn handle_input(&self, input: &egui::InputState) -> Option<AppEvent> {
        // Check all events (key_pressed, not keys_down to avoid repeats)
        for event in &input.events {
            if let egui::Event::Key { key, pressed: true, modifiers, .. } = event {
                let key_str = format!("{:?}", key);

                // Check with modifiers
                if let Some(event) = self.handle_key_with_modifiers(
                    &key_str,
                    modifiers.ctrl,
                    modifiers.shift,
                    modifiers.alt,
                ) {
                    return Some(event);
                }

                // Check without modifiers
                if !modifiers.any() {
                    if let Some(event) = self.handle_key(&key_str) {
                        return Some(event);
                    }
                }
            }
        }

        None
    }
}
