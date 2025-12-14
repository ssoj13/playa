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
    NodeEditor,
}

/// Centralized focus/hover tracking for hotkey routing.
/// Replaces scattered *_hovered flags with a single source of truth.
#[derive(Debug, Clone, Default)]
pub struct FocusTracker {
    /// Mouse is over viewport content area
    pub viewport_hovered: bool,
    /// Mouse is over timeline content area
    pub timeline_hovered: bool,
    /// Mouse is over project panel content area
    pub project_hovered: bool,
    /// Mouse is over node editor content area
    pub node_editor_hovered: bool,
    /// Node editor tab is currently active/visible (not necessarily hovered)
    pub node_editor_tab_active: bool,
}

impl FocusTracker {
    /// Reset all flags at start of frame
    pub fn reset_frame(&mut self) {
        self.node_editor_hovered = false;
        self.node_editor_tab_active = false;
        // Other hover flags are set per-widget, no need to reset
    }

    /// Determine which window should receive hotkeys based on current hover/focus state.
    /// Priority order (higher = checked first):
    /// 1. Viewport hover
    /// 2. Node editor (tab active OR hover)
    /// 3. Timeline hover
    /// 4. Project hover
    /// 5. Global fallback
    pub fn focused_window(&self, has_active_comp: bool) -> HotkeyWindow {
        // Priority 1: Explicit viewport hover
        if self.viewport_hovered {
            return HotkeyWindow::Viewport;
        }

        // Priority 2: Node editor - active tab OR hover
        if self.node_editor_tab_active || self.node_editor_hovered {
            return HotkeyWindow::NodeEditor;
        }

        // Priority 3: Explicit timeline hover
        if self.timeline_hovered {
            return HotkeyWindow::Timeline;
        }

        // Priority 4: Project hover
        if self.project_hovered {
            return HotkeyWindow::Project;
        }

        // Priority 5: Default to Timeline if comp is active (common case)
        if has_active_comp {
            return HotkeyWindow::Timeline;
        }

        HotkeyWindow::Global
    }

    /// Debug string for logging
    pub fn debug_str(&self) -> String {
        format!(
            "vp={} tl={} pj={} ne_hover={} ne_tab={}",
            self.viewport_hovered,
            self.timeline_hovered,
            self.project_hovered,
            self.node_editor_hovered,
            self.node_editor_tab_active
        )
    }
}
