//! Hotkey system - keyboard shortcuts management
//!
//! TODO: To be implemented in Phase 3

use std::collections::HashMap;
use crate::events::{AppEvent, HotkeyWindow};

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
}
