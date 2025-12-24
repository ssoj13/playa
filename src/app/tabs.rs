//! Tab rendering methods for PlayaApp.
//!
//! Contains render_*_tab methods for each dock panel:
//! - Viewport: frame display + gizmos
//! - Timeline: transport controls + layer outline
//! - Project: file browser + sequences
//! - Attributes: property editor
//! - NodeEditor: visual composition graph
//!
//! Also includes DockTabs wrapper for egui_dock TabViewer.

use eframe::egui;
use egui_dock::TabViewer;

use crate::app::{DockTab, PlayaApp};
use crate::entities::node::Node;
use crate::widgets::node_editor::render_node_editor;
use crate::widgets::viewport::ViewportRefreshEvent;
use crate::ui;
use crate::widgets;

impl PlayaApp {
    /// Render project browser tab.
    /// Dispatches project actions (file open, sequence select) to event bus.
    pub fn render_project_tab(&mut self, ui: &mut egui::Ui) {
        let project_actions = widgets::project::render(ui, &mut self.player, &self.project);

        // Store hover state for input routing
        self.project_hovered = project_actions.hovered;

        // Dispatch all events from Project UI - handling is in main_events.rs
        for evt in project_actions.events {
            self.event_bus.emit_boxed(evt);
        }
    }

    /// Render timeline tab with transport controls and layer outline.
    /// Syncs timeline toggles from settings, handles shader changes.
    pub fn render_timeline_tab(&mut self, ui: &mut egui::Ui) {
        // Sync timeline toggles from settings
        self.timeline_state.snap_enabled = self.settings.timeline_snap_enabled;
        self.timeline_state.lock_work_area = self.settings.timeline_lock_work_area;

        // Collect layout names for ComboBox
        let layout_names: Vec<String> = self.settings.layouts.keys().cloned().collect();

        // Render timeline panel with transport controls
        let (shader_changed, timeline_actions) = ui::render_timeline_panel(
            ui,
            &mut self.player,
            &self.project,
            &mut self.shader_manager,
            &mut self.timeline_state,
            &self.event_bus,
            self.settings.show_tooltips,
            self.settings.timeline_layer_height,
            self.settings.timeline_name_column_width,
            self.settings.timeline_outline_top_offset,
            &layout_names,
            &self.settings.current_layout,
            self.settings.timeline_hover_highlight,
        );

        // Store hover state for input routing
        self.timeline_hovered = timeline_actions.hovered;

        if shader_changed {
            let mut renderer = self.viewport_renderer.lock().unwrap();
            renderer.update_shader(&self.shader_manager);
            log::info!("Shader changed to: {}", self.shader_manager.current_shader);
        }
    }

    /// Render viewport tab with epoch-based refresh detection.
    ///
    /// Texture re-upload triggers:
    /// 1. Cache epoch changed (attributes modified via AttrsChangedEvent)
    /// 2. Frame number changed (scrubbing/playback)
    /// 3. Current frame still loading (poll for completion)
    pub fn render_viewport_tab(&mut self, ui: &mut egui::Ui) {
        let current_epoch = self.cache_manager.current_epoch();
        let current_frame = self.player.current_frame(&self.project);

        let epoch_changed = self.viewport_state.last_rendered_epoch != current_epoch;
        let frame_changed = self.viewport_state.last_rendered_frame != Some(current_frame);
        // Check if frame is not fully ready (needs refresh when worker finishes)
        let frame_not_ready = self
            .frame
            .as_ref()
            .map(|f| f.status() != crate::entities::frame::FrameStatus::Loaded)
            .unwrap_or(true);
        // Also re-fetch if we have no frame yet (workers may have cached it)
        let no_frame = self.frame.is_none();
        let texture_needs_upload = epoch_changed || frame_changed || frame_not_ready || no_frame;

        // If refresh needed, get frame from cache/compositor
        if texture_needs_upload {
            self.frame = self.player.get_current_frame(&self.project);
            // Update tracking only when NEW frame is fully loaded
            let new_frame_loaded = self
                .frame
                .as_ref()
                .map(|f| f.status() == crate::entities::frame::FrameStatus::Loaded)
                .unwrap_or(false);
            if new_frame_loaded {
                self.viewport_state.last_rendered_epoch = current_epoch;
                self.viewport_state.last_rendered_frame = Some(current_frame);
            }
        }

        // Display frame directly - Expired frames show valid pixels while recomputing
        let display_frame = self.frame.as_ref();

        let (viewport_actions, render_time) = widgets::viewport::render(
            ui,
            display_frame,
            self.error_msg.as_ref(),
            &mut self.player,
            &mut self.project,
            &mut self.viewport_state,
            &self.viewport_renderer,
            &mut self.shader_manager,
            &mut self.gizmo_state,
            self.show_help,
            self.is_fullscreen,
            texture_needs_upload,
            self.settings.viewport_hover_highlight,
            self.settings.tools_selection_highlight,
            self.settings.hover_stroke_width,
            self.settings.hover_corner_length,
            self.settings.hover_opacity,
        );
        self.last_render_time_ms = render_time;

        // Store hover state for input routing
        self.viewport_hovered = viewport_actions.hovered;

        // Dispatch all events from Viewport UI
        for evt in viewport_actions.events {
            self.event_bus.emit_boxed(evt);
        }

        // Persist timeline options back to settings
        self.settings.timeline_snap_enabled = self.timeline_state.snap_enabled;
        self.settings.timeline_lock_work_area = self.timeline_state.lock_work_area;
    }

    /// Render node editor tab (composition as node graph).
    ///
    /// Uses egui-snarl for visual node/wire representation of comp hierarchy.
    /// Source nodes (children) connect to Output node (current comp).
    pub fn render_node_editor_tab(&mut self, ui: &mut egui::Ui) {
        let Some(comp_uuid) = self.player.active_comp() else {
            self.node_editor_hovered = false;
            ui.centered_and_justified(|ui: &mut egui::Ui| {
                ui.label("No composition selected");
            });
            return;
        };

        // Render node editor - pass comp_uuid, let it handle locking internally
        // IMPORTANT: Don't use with_comp here! render_node_editor calls modify_comp
        // which needs write lock, causing deadlock if we hold read lock from with_comp
        let emitter = self.event_bus.emitter();
        let hovered = render_node_editor(
            ui,
            &mut self.node_editor_state,
            &self.project,
            comp_uuid,
            |evt| emitter.emit_boxed(evt),
        );

        // Hover tracking for input routing
        self.node_editor_hovered = hovered;
    }

    /// Render attributes tab with property editor.
    ///
    /// Handles both layer attributes (multi-select with mixed values)
    /// and node attributes (File, Comp, Camera, Text nodes).
    /// Effects UI rendered for single layer selection.
    pub fn render_attributes_tab(&mut self, ui: &mut egui::Ui) {
        let ae_focus = self.ae_focus.clone();
        let active = self.player.active_comp();

        // If ae_focus is empty, fallback to active comp attrs
        if ae_focus.is_empty() {
            if let Some(comp_uuid) = active {
                self.project.modify_comp(comp_uuid, |comp| {
                    let comp_name = comp.name().to_string();
                    if crate::widgets::ae::render(
                        ui,
                        &mut comp.attrs,
                        &mut self.attributes_state,
                        &comp_name,
                    ) {
                        comp.emit_attrs_changed();
                    }
                });
            }
            return;
        }

        // Check if ae_focus contains layers in active comp
        let is_layer_focus = active
            .map(|comp_uuid| {
                self.project
                    .with_comp(comp_uuid, |comp| {
                        ae_focus
                            .iter()
                            .any(|uuid| comp.layers.iter().any(|l| l.uuid() == *uuid))
                    })
                    .unwrap_or(false)
            })
            .unwrap_or(false);

        if is_layer_focus {
            self.render_layer_attributes(ui, active.unwrap(), &ae_focus);
        } else {
            self.render_node_attributes(ui, &ae_focus);
        }
    }

    /// Render layer attributes (single or multi-select).
    fn render_layer_attributes(
        &mut self,
        ui: &mut egui::Ui,
        comp_uuid: uuid::Uuid,
        ae_focus: &[uuid::Uuid],
    ) {
        use crate::entities::comp_events::SetLayerAttrsEvent;

        let render_data = self
            .project
            .with_comp(comp_uuid, |comp| {
                if ae_focus.len() > 1 {
                    // Multi-select: compute intersection of keys
                    use std::collections::{BTreeSet, HashSet};
                    let mut common_keys: BTreeSet<String> = BTreeSet::new();
                    let mut first = true;
                    for uuid in ae_focus {
                        if let Some(attrs) = comp.layers_attrs_get(uuid) {
                            let keys: BTreeSet<String> =
                                attrs.iter().map(|(k, _)| k.clone()).collect();
                            if first {
                                common_keys = keys;
                                first = false;
                            } else {
                                common_keys = common_keys.intersection(&keys).cloned().collect();
                            }
                        }
                    }
                    if common_keys.is_empty() {
                        return None;
                    }

                    let mut merged = crate::entities::Attrs::new();
                    let mut mixed_keys: HashSet<String> = HashSet::new();

                    if let Some(first_uuid) = ae_focus.first()
                        && let Some(attrs) = comp.layers_attrs_get(first_uuid)
                    {
                        for key in &common_keys {
                            if let Some(v) = attrs.get(key) {
                                merged.set(key.clone(), v.clone());
                            }
                        }
                    }
                    for key in &common_keys {
                        if let Some(base) = merged.get(key) {
                            for uuid in ae_focus {
                                if let Some(attrs) = comp.layers_attrs_get(uuid)
                                    && let Some(other) = attrs.get(key)
                                    && other != base
                                {
                                    mixed_keys.insert(key.clone());
                                    break;
                                }
                            }
                        }
                    }
                    Some((merged, mixed_keys, "Multiple layers".to_string()))
                } else if let Some(layer_uuid) = ae_focus.first() {
                    let layer_idx = comp.uuid_to_idx(*layer_uuid).unwrap_or(0);
                    if let Some(attrs) = comp.layers_attrs_get(layer_uuid) {
                        let name = attrs
                            .get_str("name")
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| format!("Layer {}", layer_idx));
                        Some((attrs.clone(), std::collections::HashSet::new(), name))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .flatten();

        if let Some((mut attrs, mixed_keys, display_name)) = render_data {
            let mut changed: Vec<(String, crate::entities::AttrValue)> = Vec::new();
            crate::widgets::ae::render_with_mixed(
                ui,
                &mut attrs,
                &mut self.attributes_state,
                &display_name,
                &mixed_keys,
                &mut changed,
            );
            if !changed.is_empty() {
                self.event_bus.emit_boxed(Box::new(SetLayerAttrsEvent {
                    comp_uuid,
                    layer_uuids: ae_focus.to_vec(),
                    attrs: changed,
                }));
            }

            // === Effects UI (single layer only) ===
            if ae_focus.len() == 1 {
                let layer_uuid = ae_focus[0];

                // Get effects clone for UI rendering (read-only pass)
                let effects_opt = self
                    .project
                    .with_comp(comp_uuid, |comp| {
                        comp.get_layer(layer_uuid).map(|l| l.effects.clone())
                    })
                    .flatten();

                if let Some(mut effects) = effects_opt {
                    let effect_actions =
                        crate::widgets::ae::render_effects(ui, &mut effects, &mut self.attributes_state);

                    // Handle effect actions
                    if !effect_actions.is_empty() {
                        self.handle_effect_actions(comp_uuid, layer_uuid, effect_actions);
                    }
                }
            }
        }
    }

    /// Render node attributes (File, Comp, Camera, Text nodes).
    fn render_node_attributes(&mut self, ui: &mut egui::Ui, ae_focus: &[uuid::Uuid]) {

        if ae_focus.len() == 1 {
            // Single node - edit directly
            let node_uuid = ae_focus[0];
            let mut node_changed = false;
            self.project.modify_node(node_uuid, |node| {
                let name = node.name().to_string();
                if crate::widgets::ae::render(ui, node.attrs_mut(), &mut self.attributes_state, &name)
                {
                    node_changed = true;
                }
            });

            // === Cache invalidation for source node attribute changes ===
            //
            // When a source node (FileNode, TextNode, CameraNode) changes:
            // 1. The source's own cached frames are stale
            // 2. Any CompNode using this source via layers is also stale
            //
            // We use "dehydrate" (clear_comp with true) instead of full clear:
            // - Dehydrate marks frames as Expired but KEEPS pixel data
            // - Viewport continues showing old pixels while new ones compute
            // - This prevents black flash during re-render
            if node_changed {
                self.invalidate_and_refresh(ae_focus);
            }
        } else {
            // Multi-select nodes: compute intersection of attrs
            self.render_multi_node_attributes(ui, ae_focus);
        }
    }

    /// Render attributes for multiple selected nodes.
    fn render_multi_node_attributes(&mut self, ui: &mut egui::Ui, ae_focus: &[uuid::Uuid]) {
        use std::collections::{BTreeSet, HashSet};

        let mut common_keys: BTreeSet<String> = BTreeSet::new();
        let mut first = true;
        let mut all_attrs: Vec<crate::entities::Attrs> = Vec::new();

        for uuid in ae_focus {
            if let Some(attrs) = self.project.with_node(*uuid, |n| n.attrs().clone()) {
                let keys: BTreeSet<String> = attrs.iter().map(|(k, _)| k.clone()).collect();
                if first {
                    common_keys = keys;
                    first = false;
                } else {
                    common_keys = common_keys.intersection(&keys).cloned().collect();
                }
                all_attrs.push(attrs);
            }
        }

        if common_keys.is_empty() || all_attrs.is_empty() {
            ui.label("No common attributes");
            return;
        }

        let mut merged = crate::entities::Attrs::new();
        let mut mixed_keys: HashSet<String> = HashSet::new();

        // Copy first node's attrs for common keys
        for key in &common_keys {
            if let Some(v) = all_attrs[0].get(key) {
                merged.set(key.clone(), v.clone());
            }
        }
        // Find mixed values
        for key in &common_keys {
            if let Some(base) = merged.get(key) {
                for attrs in &all_attrs[1..] {
                    if let Some(other) = attrs.get(key)
                        && other != base
                    {
                        mixed_keys.insert(key.clone());
                        break;
                    }
                }
            }
        }

        let mut changed: Vec<(String, crate::entities::AttrValue)> = Vec::new();
        crate::widgets::ae::render_with_mixed(
            ui,
            &mut merged,
            &mut self.attributes_state,
            "Multiple nodes",
            &mixed_keys,
            &mut changed,
        );

        // Apply changed attrs to all selected nodes
        if !changed.is_empty() {
            for uuid in ae_focus {
                self.project.modify_node(*uuid, |node| {
                    for (key, value) in &changed {
                        node.attrs_mut().set(key.clone(), value.clone());
                    }
                });
            }

            // Invalidate all modified sources + their dependents
            self.invalidate_and_refresh(ae_focus);
        }
    }

    /// Invalidate cache for modified nodes and trigger refresh.
    fn invalidate_and_refresh(&mut self, uuids: &[uuid::Uuid]) {
        // Cancel all pending preload jobs (they'd load stale data)
        if let Some(manager) = self.project.cache_manager() {
            manager.increment_epoch();
        }

        // Invalidate sources + all dependent comps recursively
        for uuid in uuids {
            self.project.invalidate_with_dependents(*uuid, true);
        }

        // Trigger recompute: current frame immediately, full preload after delay
        self.enqueue_current_frame_only();
        if let Some(comp_uuid) = self.player.active_comp() {
            self.debounced_preloader.schedule(comp_uuid);
        }
        self.event_bus.emit(ViewportRefreshEvent);
    }
}

// === DockTabs wrapper for egui_dock ===

/// Wrapper struct for egui_dock TabViewer implementation.
/// Holds mutable reference to PlayaApp for rendering tabs.
pub struct DockTabs<'a> {
    pub app: &'a mut PlayaApp,
}

impl<'a> TabViewer for DockTabs<'a> {
    type Tab = DockTab;

    fn title(&mut self, tab: &mut DockTab) -> egui::WidgetText {
        match tab {
            DockTab::Viewport => "Viewport".into(),
            DockTab::Timeline => "Timeline".into(),
            DockTab::Project => "Project".into(),
            DockTab::Attributes => "Attributes".into(),
            DockTab::NodeEditor => "Node Editor".into(),
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut DockTab) {
        // Track which tab is active for hotkey routing
        // Note: Don't reset node_editor_hovered here - it's reset at frame start
        // and set by render_node_editor_tab. Resetting here would break when
        // multiple tabs are rendered in same frame (dock splits).
        if matches!(tab, DockTab::NodeEditor) {
            self.app.node_editor_tab_active = true;
        }
        match tab {
            DockTab::Viewport => self.app.render_viewport_tab(ui),
            DockTab::Timeline => self.app.render_timeline_tab(ui),
            DockTab::Project => self.app.render_project_tab(ui),
            DockTab::Attributes => self.app.render_attributes_tab(ui),
            DockTab::NodeEditor => self.app.render_node_editor_tab(ui),
        }
    }
}
