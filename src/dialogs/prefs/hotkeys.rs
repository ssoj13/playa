//! Hotkey management and handling
//!
//! Maps keyboard inputs to AppEvent messages with window-specific contexts

use std::collections::HashMap;
use crate::events::{AppEvent, HotkeyWindow};

/// Hotkey handler - manages key bindings per window
pub struct HotkeyHandler {
    /// Bindings: (window, key) -> AppEvent
    bindings: HashMap<(HotkeyWindow, String), AppEvent>,

    /// Currently focused window
    pub focused_window: HotkeyWindow,
}

impl Default for HotkeyHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl HotkeyHandler {
    /// Create new hotkey handler with default bindings
    pub fn new() -> Self {
        let mut bindings = HashMap::new();

        // ===== Global Hotkeys =====
        let global = HotkeyWindow::Global;

        // Playback
        bindings.insert((global.clone(), "Space".into()), AppEvent::TogglePlayPause);
        bindings.insert((global.clone(), "K".into()), AppEvent::Stop);
        bindings.insert((global.clone(), ".".into()), AppEvent::Stop);
        bindings.insert((global.clone(), "J".into()), AppEvent::StepBackward);
        bindings.insert((global.clone(), ",".into()), AppEvent::StepBackward);
        bindings.insert((global.clone(), "L".into()), AppEvent::StepForward);
        bindings.insert((global.clone(), "/".into()), AppEvent::StepForward);

        // Frame navigation
        bindings.insert((global.clone(), "ArrowLeft".into()), AppEvent::StepBackward);
        bindings.insert((global.clone(), "ArrowRight".into()), AppEvent::StepForward);
        bindings.insert((global.clone(), "PageUp".into()), AppEvent::StepBackward);
        bindings.insert((global.clone(), "PageDown".into()), AppEvent::StepForward);
        bindings.insert((global.clone(), "Home".into()), AppEvent::JumpToStart);
        bindings.insert((global.clone(), "End".into()), AppEvent::JumpToEnd);
        bindings.insert((global.clone(), "1".into()), AppEvent::JumpToStart);
        bindings.insert((global.clone(), "2".into()), AppEvent::JumpToEnd);
        bindings.insert((global.clone(), "[".into()), AppEvent::PreviousClip);
        bindings.insert((global.clone(), "]".into()), AppEvent::NextClip);

        // UI toggles
        bindings.insert((global.clone(), "F1".into()), AppEvent::ToggleHelp);
        bindings.insert((global.clone(), "F2".into()), AppEvent::TogglePlaylist);
        bindings.insert((global.clone(), "F3".into()), AppEvent::ToggleSettings);
        bindings.insert((global.clone(), "Z".into()), AppEvent::ToggleFullscreen);
        bindings.insert((global.clone(), "Escape".into()), AppEvent::ToggleFullscreen);
        bindings.insert((global.clone(), "'".into()), AppEvent::ToggleLoop);
        bindings.insert((global.clone(), "`".into()), AppEvent::ToggleLoop);
        bindings.insert((global.clone(), "Backspace".into()), AppEvent::ToggleFrameNumbers);

        // Play range
        bindings.insert((global.clone(), "B".into()), AppEvent::SetPlayRangeStart);
        bindings.insert((global.clone(), "N".into()), AppEvent::SetPlayRangeEnd);

        // View
        bindings.insert((global.clone(), "A".into()), AppEvent::ResetViewport);
        bindings.insert((global.clone(), "H".into()), AppEvent::ResetViewport);
        bindings.insert((global.clone(), "F".into()), AppEvent::FitViewport);

        // FPS
        bindings.insert((global.clone(), "-".into()), AppEvent::DecreaseFPS);
        bindings.insert((global.clone(), "=".into()), AppEvent::IncreaseFPS);
        bindings.insert((global.clone(), "+".into()), AppEvent::IncreaseFPS);

        // ===== Timeline-specific Hotkeys =====
        let timeline = HotkeyWindow::Timeline;
        bindings.insert((timeline.clone(), "Delete".into()), AppEvent::RemoveSelectedLayer);
        bindings.insert((timeline.clone(), "Backspace".into()), AppEvent::RemoveSelectedLayer);

        Self {
            bindings,
            focused_window: HotkeyWindow::Global,
        }
    }

    /// Handle key press - returns event if key is bound
    pub fn handle_key(&self, key: &str) -> Option<AppEvent> {
        // Try window-specific binding first
        self.bindings
            .get(&(self.focused_window.clone(), key.to_string()))
            .cloned()
            // Fallback to global binding
            .or_else(|| {
                self.bindings
                    .get(&(HotkeyWindow::Global, key.to_string()))
                    .cloned()
            })
    }

    /// Handle key with modifiers
    pub fn handle_key_with_modifiers(
        &self,
        key: &str,
        shift: bool,
        ctrl: bool,
        alt: bool,
    ) -> Option<AppEvent> {
        // Special handling for modified keys
        if shift && !ctrl && !alt {
            match key {
                "ArrowLeft" | "PageUp" => return Some(AppEvent::StepBackwardLarge),
                "ArrowRight" | "PageDown" => return Some(AppEvent::StepForwardLarge),
                _ => {}
            }
        }

        if ctrl && !shift && !alt {
            match key {
                "ArrowLeft" | "PageUp" => return Some(AppEvent::JumpToStart),
                "ArrowRight" | "PageDown" => return Some(AppEvent::JumpToEnd),
                "B" => return Some(AppEvent::ResetPlayRange),
                "R" => {
                    // TODO: Reset settings - needs implementation
                    return None;
                }
                _ => {}
            }
        }

        // No modifier or not handled - try default bindings
        if !shift && !ctrl && !alt {
            return self.handle_key(key);
        }

        None
    }

    /// Set focused window
    pub fn set_focused_window(&mut self, window: HotkeyWindow) {
        self.focused_window = window;
    }

    /// Add custom key binding
    pub fn add_binding(&mut self, window: HotkeyWindow, key: String, event: AppEvent) {
        self.bindings.insert((window, key), event);
    }

    /// Remove key binding
    pub fn remove_binding(&mut self, window: HotkeyWindow, key: &str) {
        self.bindings.remove(&(window, key.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_global_hotkeys() {
        let handler = HotkeyHandler::new();

        assert!(matches!(
            handler.handle_key("Space"),
            Some(AppEvent::TogglePlayPause)
        ));
        assert!(matches!(handler.handle_key("K"), Some(AppEvent::Stop)));
        assert!(matches!(
            handler.handle_key("ArrowLeft"),
            Some(AppEvent::StepBackward)
        ));
    }

    #[test]
    fn test_timeline_specific() {
        let mut handler = HotkeyHandler::new();
        handler.set_focused_window(HotkeyWindow::Timeline);

        assert!(matches!(
            handler.handle_key("Delete"),
            Some(AppEvent::RemoveSelectedLayer)
        ));

        // Global fallback still works
        assert!(matches!(
            handler.handle_key("Space"),
            Some(AppEvent::TogglePlayPause)
        ));
    }

    #[test]
    fn test_modifiers() {
        let handler = HotkeyHandler::new();

        // Shift + ArrowRight -> StepForwardLarge
        assert!(matches!(
            handler.handle_key_with_modifiers("ArrowRight", true, false, false),
            Some(AppEvent::StepForwardLarge)
        ));

        // Ctrl + ArrowRight -> JumpToEnd
        assert!(matches!(
            handler.handle_key_with_modifiers("ArrowRight", false, true, false),
            Some(AppEvent::JumpToEnd)
        ));

        // Ctrl + B -> ResetPlayRange
        assert!(matches!(
            handler.handle_key_with_modifiers("B", false, true, false),
            Some(AppEvent::ResetPlayRange)
        ));
    }

    #[test]
    fn test_custom_binding() {
        let mut handler = HotkeyHandler::new();

        handler.add_binding(
            HotkeyWindow::Viewport,
            "G".into(),
            AppEvent::ToggleHelp,
        );

        handler.set_focused_window(HotkeyWindow::Viewport);
        assert!(matches!(
            handler.handle_key("G"),
            Some(AppEvent::ToggleHelp)
        ));
    }
}
