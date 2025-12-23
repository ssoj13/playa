//! Layout management for PlayaApp.
//!
//! Contains methods for:
//! - Saving/loading layouts to project attrs
//! - Named layouts (stored in AppSettings)
//! - Dock panel visibility sync

use super::{DockTab, PlayaApp};
use crate::dialogs::prefs::prefs::Layout;
use crate::entities::AttrValue;
use crate::widgets::timeline::TimelineViewMode;
use crate::widgets::viewport::ViewportMode;

impl PlayaApp {
    /// Sync dock tabs visibility with show_* flags.
    pub fn sync_dock_tabs_visibility(&mut self) {
        // Check which optional tabs should be visible
        let show_project = self.show_playlist;
        let show_attributes = self.show_attributes_editor;

        // Get current visibility state
        let current_tabs: Vec<DockTab> = self.dock_state
            .iter_all_tabs()
            .map(|(_, tab)| tab.clone())
            .collect();

        let current_has_project = current_tabs.contains(&DockTab::Project);
        let current_has_attributes = current_tabs.contains(&DockTab::Attributes);

        // If visibility state changed, rebuild dock structure with saved position
        if show_project != current_has_project || show_attributes != current_has_attributes {
            self.dock_state = Self::build_dock_state(
                show_project,
                show_attributes,
                self.attributes_state.project_attributes_split,
            );
        }
    }

    /// Save current split position (call after DockArea rendering).
    pub fn save_dock_split_positions(&mut self) {
        if let Some(pos) = self.extract_project_attributes_split() {
            self.attributes_state.project_attributes_split = pos;
        }
    }

    /// Extract the current split position between Project and Attributes panels.
    pub fn extract_project_attributes_split(&self) -> Option<f32> {
        // In our dock layout:
        // - First vertical split = Viewport/Timeline (0.65)
        // - Second vertical split = Project/Attributes (user's position)
        // So we need to find the SECOND vertical split, not the first
        use egui_dock::Node;

        let surface = self.dock_state.main_surface();
        let mut vertical_count = 0;

        for node in surface.iter() {
            if let Node::Vertical(split_node) = node {
                vertical_count += 1;
                // Return the second vertical split we find
                if vertical_count == 2 {
                    return Some(split_node.fraction);
                }
            }
        }
        None
    }

    /// Save current layout to project attrs.
    /// dock_state is serialized as JSON, timeline/viewport as individual fields.
    pub fn save_layout_to_attrs(&mut self) {
        // Serialize dock_state as JSON
        if let Ok(dock_json) = serde_json::to_string(&self.dock_state) {
            self.project.attrs.set("layout.dock_state", AttrValue::Str(dock_json));
        }

        // Timeline state - individual fields
        self.project.attrs.set("layout.timeline.zoom", AttrValue::Float(self.timeline_state.zoom));
        self.project.attrs.set("layout.timeline.pan_offset", AttrValue::Float(self.timeline_state.pan_offset));
        self.project.attrs.set("layout.timeline.outline_width", AttrValue::Float(self.timeline_state.outline_width));
        let view_mode_str = match self.timeline_state.view_mode {
            TimelineViewMode::Split => "Split",
            TimelineViewMode::CanvasOnly => "CanvasOnly",
            TimelineViewMode::OutlineOnly => "OutlineOnly",
        };
        self.project.attrs.set("layout.timeline.view_mode", AttrValue::Str(view_mode_str.to_string()));

        // Viewport state - individual fields
        self.project.attrs.set("layout.viewport.zoom", AttrValue::Float(self.viewport_state.zoom));
        self.project.attrs.set("layout.viewport.pan_x", AttrValue::Float(self.viewport_state.pan.x));
        self.project.attrs.set("layout.viewport.pan_y", AttrValue::Float(self.viewport_state.pan.y));
        let mode_str = match self.viewport_state.mode {
            ViewportMode::Manual => "Manual",
            ViewportMode::AutoFit => "AutoFit",
            ViewportMode::Auto100 => "Auto100",
        };
        self.project.attrs.set("layout.viewport.mode", AttrValue::Str(mode_str.to_string()));

        log::debug!("Layout saved to project attrs");
    }

    /// Load layout from project attrs.
    /// Restores dock_state, timeline_state, viewport_state from saved values.
    pub fn load_layout_from_attrs(&mut self) {
        // Load dock_state from JSON
        if let Some(dock_json) = self.project.attrs.get_str("layout.dock_state") {
            if let Ok(dock) = serde_json::from_str(dock_json) {
                self.dock_state = dock;
                log::debug!("Dock state loaded from attrs");
            }
        }

        // Load timeline state
        if let Some(zoom) = self.project.attrs.get_float("layout.timeline.zoom") {
            self.timeline_state.zoom = zoom;
        }
        if let Some(pan) = self.project.attrs.get_float("layout.timeline.pan_offset") {
            self.timeline_state.pan_offset = pan;
        }
        if let Some(width) = self.project.attrs.get_float("layout.timeline.outline_width") {
            self.timeline_state.outline_width = width;
        }
        if let Some(mode_str) = self.project.attrs.get_str("layout.timeline.view_mode") {
            self.timeline_state.view_mode = match mode_str {
                "CanvasOnly" => TimelineViewMode::CanvasOnly,
                "OutlineOnly" => TimelineViewMode::OutlineOnly,
                _ => TimelineViewMode::Split,
            };
        }

        // Load viewport state
        if let Some(zoom) = self.project.attrs.get_float("layout.viewport.zoom") {
            self.viewport_state.zoom = zoom;
        }
        if let Some(pan_x) = self.project.attrs.get_float("layout.viewport.pan_x") {
            self.viewport_state.pan.x = pan_x;
        }
        if let Some(pan_y) = self.project.attrs.get_float("layout.viewport.pan_y") {
            self.viewport_state.pan.y = pan_y;
        }
        if let Some(mode_str) = self.project.attrs.get_str("layout.viewport.mode") {
            self.viewport_state.mode = match mode_str {
                "Manual" => ViewportMode::Manual,
                "Auto100" => ViewportMode::Auto100,
                _ => ViewportMode::AutoFit,
            };
        }

        log::debug!("Layout loaded from project attrs");
    }

    /// Reset layout to defaults.
    pub fn reset_layout(&mut self) {
        self.dock_state = Self::default_dock_state();
        self.timeline_state = Default::default();
        self.viewport_state = Default::default();

        // Clear saved layout from attrs
        self.project.attrs.remove("layout.dock_state");
        self.project.attrs.remove("layout.timeline.zoom");
        self.project.attrs.remove("layout.timeline.pan_offset");
        self.project.attrs.remove("layout.timeline.outline_width");
        self.project.attrs.remove("layout.timeline.view_mode");
        self.project.attrs.remove("layout.viewport.zoom");
        self.project.attrs.remove("layout.viewport.pan_x");
        self.project.attrs.remove("layout.viewport.pan_y");
        self.project.attrs.remove("layout.viewport.mode");

        log::info!("Layout reset to defaults");
    }

    // === Named Layouts (stored in AppSettings) ===

    /// Capture current UI state into a Layout struct.
    pub fn capture_current_layout(&self) -> Layout {
        let dock_json = serde_json::to_string(&self.dock_state).unwrap_or_default();
        let view_mode_str = match self.timeline_state.view_mode {
            TimelineViewMode::Split => "Split",
            TimelineViewMode::CanvasOnly => "CanvasOnly",
            TimelineViewMode::OutlineOnly => "OutlineOnly",
        };
        let mode_str = match self.viewport_state.mode {
            ViewportMode::Manual => "Manual",
            ViewportMode::AutoFit => "AutoFit",
            ViewportMode::Auto100 => "Auto100",
        };
        Layout {
            dock_state_json: dock_json,
            timeline_zoom: self.timeline_state.zoom,
            timeline_pan_offset: self.timeline_state.pan_offset,
            timeline_outline_width: self.timeline_state.outline_width,
            timeline_view_mode: view_mode_str.to_string(),
            viewport_zoom: self.viewport_state.zoom,
            viewport_pan: [self.viewport_state.pan.x, self.viewport_state.pan.y],
            viewport_mode: mode_str.to_string(),
        }
    }

    /// Apply a Layout struct to current UI state.
    pub fn apply_layout(&mut self, layout: &Layout) {
        // Restore dock_state
        if let Ok(dock) = serde_json::from_str(&layout.dock_state_json) {
            self.dock_state = dock;
        }
        // Restore timeline state
        self.timeline_state.zoom = layout.timeline_zoom;
        self.timeline_state.pan_offset = layout.timeline_pan_offset;
        self.timeline_state.outline_width = layout.timeline_outline_width;
        self.timeline_state.view_mode = match layout.timeline_view_mode.as_str() {
            "CanvasOnly" => TimelineViewMode::CanvasOnly,
            "OutlineOnly" => TimelineViewMode::OutlineOnly,
            _ => TimelineViewMode::Split,
        };
        // Restore viewport state
        self.viewport_state.zoom = layout.viewport_zoom;
        self.viewport_state.pan.x = layout.viewport_pan[0];
        self.viewport_state.pan.y = layout.viewport_pan[1];
        self.viewport_state.mode = match layout.viewport_mode.as_str() {
            "Manual" => ViewportMode::Manual,
            "Auto100" => ViewportMode::Auto100,
            _ => ViewportMode::AutoFit,
        };
    }

    /// Select a named layout from settings and apply it.
    pub fn select_layout(&mut self, name: &str) {
        if let Some(layout) = self.settings.layouts.get(name).cloned() {
            self.apply_layout(&layout);
            self.settings.current_layout = name.to_string();
            log::info!("Selected layout: {}", name);
        } else {
            log::warn!("Layout not found: {}", name);
        }
    }

    /// Create a new named layout from current UI state.
    pub fn create_layout(&mut self, name: Option<String>) {
        let name = name.unwrap_or_else(|| {
            // Auto-generate name: "Layout 1", "Layout 2", etc.
            let mut n = 1;
            loop {
                let candidate = format!("Layout {}", n);
                if !self.settings.layouts.contains_key(&candidate) {
                    break candidate;
                }
                n += 1;
            }
        });
        let layout = self.capture_current_layout();
        self.settings.layouts.insert(name.clone(), layout);
        self.settings.current_layout = name.clone();
        log::info!("Created layout: {}", name);
    }

    /// Delete a named layout from settings.
    /// Clears current_layout if the deleted layout was selected.
    pub fn delete_layout(&mut self, name: &str) {
        if self.settings.layouts.remove(name).is_some() {
            if self.settings.current_layout == name {
                self.settings.current_layout.clear();
            }
            log::info!("Deleted layout: {}", name);
        }
    }

    /// Rename a layout in settings.
    /// Updates current_layout if the renamed layout was selected.
    /// Does nothing if old_name doesn't exist or new_name already exists.
    pub fn rename_layout(&mut self, old_name: &str, new_name: &str) {
        // Don't rename to an existing name
        if self.settings.layouts.contains_key(new_name) {
            log::warn!("Cannot rename layout: '{}' already exists", new_name);
            return;
        }
        
        // Remove old and insert with new name
        if let Some(layout) = self.settings.layouts.remove(old_name) {
            self.settings.layouts.insert(new_name.to_string(), layout);
            
            // Update current_layout if it was the renamed one
            if self.settings.current_layout == old_name {
                self.settings.current_layout = new_name.to_string();
            }
            
            log::info!("Renamed layout: '{}' -> '{}'", old_name, new_name);
        }
    }

    /// Update the current layout with current UI state.
    pub fn update_current_layout(&mut self) {
        if self.settings.current_layout.is_empty() {
            return; // No layout selected, nothing to update
        }
        let name = self.settings.current_layout.clone();
        if self.settings.layouts.contains_key(&name) {
            let layout = self.capture_current_layout();
            self.settings.layouts.insert(name.clone(), layout);
            log::trace!("Updated layout: {}", name);
        }
    }
}
