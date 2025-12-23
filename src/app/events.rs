//! Event handling for PlayaApp.
//!
//! Contains handlers for:
//! - Event bus events (handle_events)
//! - Effect actions (handle_effect_actions)  
//! - Keyboard input (handle_keyboard_input)
//! - Focus detection (determine_focused_window)

use super::PlayaApp;
use crate::core::event_bus::downcast_event;
use crate::dialogs::prefs::prefs_events::HotkeyWindow;
use crate::entities::comp_events::*;
use crate::main_events;
use crate::widgets::ae::EffectAction;
use crate::widgets::project::project_events::ClearCacheEvent;
use crate::widgets::viewport::ViewportRefreshEvent;

use eframe::egui;
use log::{info, trace};
use uuid::Uuid;

impl PlayaApp {
    /// Handle events from event bus.
    pub fn handle_events(&mut self) {
        // Deferred actions to execute after event loop
        let mut deferred_load_project: Option<std::path::PathBuf> = None;
        let mut deferred_save_project: Option<std::path::PathBuf> = None;
        let mut deferred_load_sequences: Option<Vec<std::path::PathBuf>> = None;
        let mut deferred_new_comp: Option<(String, f32)> = None;
        let mut deferred_new_camera: Option<String> = None;
        let mut deferred_new_text: Option<(String, String)> = None;
        let mut deferred_enqueue_frames = false;
        let mut deferred_quick_save = false;
        let mut deferred_show_open = false;

        // Poll all events from the bus
        let events = self.event_bus.poll();
        for event in events {

            // === Comp events (high priority, internal) ===
            if let Some(e) = downcast_event::<CurrentFrameChangedEvent>(&event) {
                trace!("Comp {} frame changed: {} → {}", e.comp_uuid, e.old_frame, e.new_frame);
                self.enqueue_frame_loads_around_playhead(self.settings.preload_radius);
                continue;
            }
            if let Some(e) = downcast_event::<LayersChangedEvent>(&event) {
                trace!("Comp {} layers changed (range: {:?})", e.comp_uuid, e.affected_range);
                // 1. Increment epoch to cancel all pending worker tasks
                // Why: Old tasks may write stale data to cache, causing eviction loops
                if let Some(manager) = self.project.cache_manager() {
                    manager.increment_epoch();
                }
                // 2. Clear affected frames from cache (they need recomposition)
                // Preload is triggered by centralized dirty check in update()
                if let Some(ref cache) = self.project.global_cache {
                    match e.affected_range {
                        Some((start, end)) => cache.clear_range(e.comp_uuid, start, end),
                        None => cache.clear_comp(e.comp_uuid, true, None),
                    }
                }
                continue;
            }
            // AttrsChangedEvent - emitted by Comp::set_child_attr[s]() and emit_attrs_changed()
            // Handles attribute changes from: timeline outline, Attribute Editor, programmatic
            // See comp_events.rs and comp.rs for event architecture documentation
            if let Some(e) = downcast_event::<AttrsChangedEvent>(&event) {
                trace!("Comp {} attrs changed - triggering cascade invalidation", e.0);
                // 1. Increment epoch to cancel pending worker tasks (stale data prevention)
                if let Some(manager) = self.project.cache_manager() {
                    manager.increment_epoch();
                }
                // 2. Clear all cached frames - any attribute could affect rendering
                if let Some(ref cache) = self.project.global_cache {
                    cache.clear_comp(e.0, true, None);
                }
                // 3. Debounced preload: current frame immediately, full preload after delay
                //    This prevents flooding cache with requests during rapid slider scrubbing
                self.enqueue_current_frame_only();
                self.debounced_preloader.schedule(e.0);
                // 5. Request viewport refresh
                self.event_bus.emit(ViewportRefreshEvent);
                continue;
            }
            // ViewportRefreshEvent - force viewport to re-fetch current frame
            if downcast_event::<ViewportRefreshEvent>(&event).is_some() {
                trace!("ViewportRefreshEvent - forcing frame refresh");
                self.viewport_state.request_refresh();
                continue;
            }
            // ClearCacheEvent - clear all cached frames (Ctrl+Alt+Slash)
            if downcast_event::<ClearCacheEvent>(&event).is_some() {
                info!("ClearCacheEvent - clearing all cached frames");
                if let Some(manager) = self.project.cache_manager() {
                    manager.increment_epoch();
                }
                if let Some(ref cache) = self.project.global_cache {
                    cache.clear_all();
                }
                self.event_bus.emit(ViewportRefreshEvent);
                continue;
            }
            // Layout events - save/load/reset UI layout
            if downcast_event::<crate::core::layout_events::SaveLayoutEvent>(&event).is_some() {
                self.save_layout_to_attrs();
                continue;
            }
            if downcast_event::<crate::core::layout_events::LoadLayoutEvent>(&event).is_some() {
                self.load_layout_from_attrs();
                continue;
            }
            if downcast_event::<crate::core::layout_events::ResetLayoutEvent>(&event).is_some() {
                self.reset_layout();
                continue;
            }
            // Named layout events
            if let Some(evt) = downcast_event::<crate::core::layout_events::LayoutSelectedEvent>(&event) {
                self.select_layout(&evt.0);
                continue;
            }
            if let Some(evt) = downcast_event::<crate::core::layout_events::LayoutCreatedEvent>(&event) {
                self.create_layout(evt.0.clone());
                continue;
            }
            if let Some(evt) = downcast_event::<crate::core::layout_events::LayoutDeletedEvent>(&event) {
                self.delete_layout(&evt.0);
                continue;
            }
            if downcast_event::<crate::core::layout_events::LayoutUpdatedEvent>(&event).is_some() {
                self.update_current_layout();
                continue;
            }
            if let Some(evt) = downcast_event::<crate::core::layout_events::LayoutRenamedEvent>(&event) {
                self.rename_layout(&evt.0, &evt.1);
                continue;
            }
            // === App events - delegate to main_events module ===
            // log::trace!("[HANDLE] checking event type_id={:?}", (*event).type_id());
            if let Some(result) = main_events::handle_app_event(
                &event,
                &mut self.player,
                &mut self.project,
                &mut self.timeline_state,
                &mut self.node_editor_state,
                &mut self.viewport_state,
                &mut self.settings,
                &mut self.show_help,
                &mut self.show_playlist,
                &mut self.show_settings,
                &mut self.show_encode_dialog,
                &mut self.show_attributes_editor,
                &mut self.encode_dialog,
                &mut self.is_fullscreen,
                &mut self.fullscreen_dirty,
                &mut self.reset_settings_pending,
            ) {
                // log::trace!("[HANDLE] got result, ae_focus_update={:?}", result.ae_focus_update);
                // Process deferred actions from EventResult
                if let Some(path) = result.load_project {
                    deferred_load_project = Some(path);
                }
                if let Some(path) = result.save_project {
                    deferred_save_project = Some(path);
                }
                if let Some(paths) = result.load_sequences {
                    deferred_load_sequences = Some(paths);
                }
                if let Some(comp_data) = result.new_comp {
                    deferred_new_comp = Some(comp_data);
                }
                if let Some(camera_name) = result.new_camera {
                    deferred_new_camera = Some(camera_name);
                }
                if let Some(text_data) = result.new_text {
                    deferred_new_text = Some(text_data);
                }
                deferred_enqueue_frames |= result.enqueue_frames;
                if result.quick_save {
                    deferred_quick_save = true;
                }
                if result.show_open_dialog {
                    deferred_show_open = true;
                }
                // Update AE panel focus (immediate, not deferred)
                if let Some(focus) = result.ae_focus_update {
                    self.ae_focus = focus;
                }
            }
        }

        // === DERIVED EVENTS LOOP - DO NOT REMOVE! ===
        // 
        // WHY THIS EXISTS:
        // When handle_app_event() processes MoveAndReorderLayerEvent (or similar), it calls
        // modify_comp() which emits AttrsChangedEvent. But since we're INSIDE the main
        // `for event in poll()` loop, this new event goes into the queue and would only
        // be processed on the NEXT frame - causing a 1-frame delay before cache invalidation.
        // 
        // Without this loop: layer move -> render uses stale cache -> next frame clears cache
        // With this loop:    layer move -> derived events processed -> cache cleared -> fresh render
        //
        // This keeps everything through EventBus (no direct calls) while ensuring same-frame response.
        // Max iterations (10) prevents infinite loops if there's ever an event cycle.
        //
        // DO NOT REFACTOR THIS INTO THE MAIN LOOP - the main loop has already drained poll().
        // DO NOT USE DIRECT CALLS - we need EventBus for decoupling and traceability.
        for iteration in 0..10 {
            let derived = self.event_bus.poll();
            if derived.is_empty() {
                break;
            }
            trace!("[DERIVED] iteration={}, events={}", iteration, derived.len());
            for event in derived {
                if let Some(e) = downcast_event::<AttrsChangedEvent>(&event) {
                    trace!("[DERIVED] AttrsChangedEvent comp={}", e.0);
                    if let Some(manager) = self.project.cache_manager() {
                        manager.increment_epoch();
                    }
                    if let Some(ref cache) = self.project.global_cache {
                        cache.clear_comp(e.0, true, None);
                    }
                    // Debounced preload: current frame immediately, full preload after delay
                    self.enqueue_current_frame_only();
                    self.debounced_preloader.schedule(e.0);
                    self.event_bus.emit(ViewportRefreshEvent);
                    continue;
                }
                if downcast_event::<ViewportRefreshEvent>(&event).is_some() {
                    self.viewport_state.request_refresh();
                    continue;
                }
                // Other derived events are ignored (processed next frame)
            }
        }

        // Execute deferred actions outside the event loop (to avoid borrow conflicts)
        if let Some(path) = deferred_load_project {
            self.load_project(path);
        }
        if let Some(path) = deferred_save_project {
            self.save_project(path);
        }
        if let Some(paths) = deferred_load_sequences {
            let _ = self.load_sequences(paths);
        }
        if let Some((name, fps)) = deferred_new_comp {
            let uuid = self.project.create_comp(&name, fps, self.comp_event_emitter.clone());
            self.player.set_active_comp(Some(uuid), &mut self.project);
            self.node_editor_state.set_comp(uuid);
            info!("Created new comp: {}", uuid);
        }
        if let Some(name) = deferred_new_camera {
            use crate::entities::CameraNode;
            let camera = CameraNode::new(&name);
            let uuid = camera.uuid();
            self.project.add_node(camera.into());
            info!("Created new camera: {}", uuid);
        }
        if let Some((name, text)) = deferred_new_text {
            use crate::entities::TextNode;
            let text_node = TextNode::new(&name, &text);
            let uuid = text_node.uuid();
            self.project.add_node(text_node.into());
            info!("Created new text: {}", uuid);
        }
        if deferred_enqueue_frames {
            self.enqueue_frame_loads_around_playhead(self.settings.preload_radius);
        }
        if deferred_quick_save {
            self.quick_save();
        }
        if deferred_show_open {
            self.show_open_project_dialog();
        }
    }

    /// Determine which window/panel currently has focus for hotkey routing.
    pub fn determine_focused_window(&self, ctx: &egui::Context) -> HotkeyWindow {
        // Priority 1: Modal dialogs (settings, encode) - always capture input
        if self.show_settings || self.show_encode_dialog {
            return HotkeyWindow::Global;
        }

        // Priority 2: Keyboard focus (text fields) - don't process hotkeys
        if ctx.wants_keyboard_input() {
            return HotkeyWindow::Global; // Return Global but will be filtered later
        }

        // Priority 3: Explicit hover (hover takes precedence over active tab)
        if self.viewport_hovered {
            return HotkeyWindow::Viewport;
        }
        if self.node_editor_hovered {
            return HotkeyWindow::NodeEditor;
        }
        if self.timeline_hovered {
            return HotkeyWindow::Timeline;
        }
        if self.project_hovered {
            return HotkeyWindow::Project;
        }

        // Priority 4: Active tab (when nothing is explicitly hovered)
        if self.node_editor_tab_active {
            return HotkeyWindow::NodeEditor;
        }

        // Priority 5: Default to timeline when a comp is active (keyboard fallback)
        // This allows playback hotkeys (Space, arrows) to work without explicit hover
        if self.player.active_comp().is_some() {
            return HotkeyWindow::Timeline;
        }

        // Fallback to Global
        HotkeyWindow::Global
    }
    
    /// Handle effect actions from the Attribute Editor effects UI.
    /// Modifies layer effects and triggers cache invalidation.
    pub fn handle_effect_actions(
        &mut self,
        comp_uuid: Uuid,
        layer_uuid: Uuid,
        actions: Vec<EffectAction>,
    ) {
        use crate::entities::effects::Effect;
        
        let mut needs_invalidate = false;
        
        for action in actions {
            match action {
                EffectAction::Add(effect_type) => {
                    self.project.modify_comp(comp_uuid, |comp| {
                        if let Some(layer) = comp.get_layer_mut(layer_uuid) {
                            layer.effects.push(Effect::new(effect_type));
                        }
                        comp.attrs.mark_dirty(); // Effects changed → comp dirty
                    });
                    needs_invalidate = true;
                }
                EffectAction::Remove(effect_uuid) => {
                    self.project.modify_comp(comp_uuid, |comp| {
                        if let Some(layer) = comp.get_layer_mut(layer_uuid) {
                            layer.effects.retain(|e| e.uuid != effect_uuid);
                        }
                        comp.attrs.mark_dirty();
                    });
                    needs_invalidate = true;
                }
                EffectAction::ToggleEnabled(effect_uuid) => {
                    self.project.modify_comp(comp_uuid, |comp| {
                        if let Some(layer) = comp.get_layer_mut(layer_uuid) {
                            if let Some(effect) = layer.effects.iter_mut().find(|e| e.uuid == effect_uuid) {
                                effect.enabled = !effect.enabled;
                            }
                        }
                        comp.attrs.mark_dirty();
                    });
                    needs_invalidate = true;
                }
                EffectAction::ToggleCollapsed(effect_uuid) => {
                    // UI-only state, no invalidation needed
                    self.project.modify_comp(comp_uuid, |comp| {
                        if let Some(layer) = comp.get_layer_mut(layer_uuid) {
                            if let Some(effect) = layer.effects.iter_mut().find(|e| e.uuid == effect_uuid) {
                                effect.collapsed = !effect.collapsed;
                            }
                        }
                    });
                }
                EffectAction::AttrChanged(effect_uuid, key, value) => {
                    self.project.modify_comp(comp_uuid, |comp| {
                        if let Some(layer) = comp.get_layer_mut(layer_uuid) {
                            if let Some(effect) = layer.effects.iter_mut().find(|e| e.uuid == effect_uuid) {
                                effect.attrs.set(&key, value);
                            }
                        }
                        comp.attrs.mark_dirty();
                    });
                    needs_invalidate = true;
                }
                EffectAction::MoveUp(effect_uuid) => {
                    self.project.modify_comp(comp_uuid, |comp| {
                        if let Some(layer) = comp.get_layer_mut(layer_uuid) {
                            if let Some(idx) = layer.effects.iter().position(|e| e.uuid == effect_uuid) {
                                if idx > 0 {
                                    layer.effects.swap(idx, idx - 1);
                                }
                            }
                        }
                        comp.attrs.mark_dirty();
                    });
                    needs_invalidate = true;
                }
                EffectAction::MoveDown(effect_uuid) => {
                    self.project.modify_comp(comp_uuid, |comp| {
                        if let Some(layer) = comp.get_layer_mut(layer_uuid) {
                            if let Some(idx) = layer.effects.iter().position(|e| e.uuid == effect_uuid) {
                                if idx < layer.effects.len() - 1 {
                                    layer.effects.swap(idx, idx + 1);
                                }
                            }
                        }
                        comp.attrs.mark_dirty();
                    });
                    needs_invalidate = true;
                }
            }
        }
        
        if needs_invalidate {
            // Invalidate comp cache and trigger refresh
            self.project.invalidate_with_dependents(comp_uuid, true);
            self.enqueue_current_frame_only();
            self.event_bus.emit(ViewportRefreshEvent);
        }
    }

    /// Handle keyboard input and hotkeys.
    pub fn handle_keyboard_input(&mut self, ctx: &egui::Context) {
        // Don't process hotkeys when text input is active (typing in fields)
        if ctx.wants_keyboard_input() {
            return;
        }

        let input = ctx.input(|i| i.clone());

        // Determine focused window and update hotkey handler
        let focused_window = self.determine_focused_window(ctx);
        self.focused_window = focused_window;
        self.hotkey_handler
            .set_focused_window(focused_window);

        // Try hotkey handler first (for context-aware hotkeys)
        if let Some(event) = self.hotkey_handler.handle_input(&input) {
            use crate::entities::comp_events::{
                AlignLayersStartEvent, AlignLayersEndEvent, 
                TrimLayersStartEvent, TrimLayersEndEvent, 
                DuplicateLayersEvent, CopyLayersEvent, PasteLayersEvent, 
                SelectAllLayersEvent, ClearLayerSelectionEvent, ResetTrimsEvent
            };

            // Fill comp_uuid for timeline-specific events
            if let Some(active_comp_uuid) = self.player.active_comp() {
                // Check if event needs comp_uuid filled in
                if downcast_event::<AlignLayersStartEvent>(&event).is_some() {
                    self.event_bus.emit(AlignLayersStartEvent(active_comp_uuid));
                    return;
                }
                if downcast_event::<AlignLayersEndEvent>(&event).is_some() {
                    self.event_bus.emit(AlignLayersEndEvent(active_comp_uuid));
                    return;
                }
                if downcast_event::<TrimLayersStartEvent>(&event).is_some() {
                    self.event_bus.emit(TrimLayersStartEvent(active_comp_uuid));
                    return;
                }
                if downcast_event::<TrimLayersEndEvent>(&event).is_some() {
                    self.event_bus.emit(TrimLayersEndEvent(active_comp_uuid));
                    return;
                }
                // Layer clipboard operations
                if downcast_event::<DuplicateLayersEvent>(&event).is_some() {
                    log::trace!("Hotkey: Ctrl-D -> DuplicateLayersEvent");
                    self.event_bus.emit(DuplicateLayersEvent { comp_uuid: active_comp_uuid });
                    return;
                }
                if downcast_event::<CopyLayersEvent>(&event).is_some() {
                    log::trace!("Hotkey: Ctrl-C -> CopyLayersEvent");
                    self.event_bus.emit(CopyLayersEvent { comp_uuid: active_comp_uuid });
                    return;
                }
                if downcast_event::<PasteLayersEvent>(&event).is_some() {
                    // Get current playhead position for paste target
                    let target_frame = self.project.with_comp(active_comp_uuid, |c| c.frame())
                        .unwrap_or(0);
                    log::trace!("Hotkey: Ctrl-V -> PasteLayersEvent at frame {}", target_frame);
                    self.event_bus.emit(PasteLayersEvent { comp_uuid: active_comp_uuid, target_frame });
                    return;
                }
                // Selection operations
                if downcast_event::<SelectAllLayersEvent>(&event).is_some() {
                    log::trace!("Hotkey: Ctrl-A -> SelectAllLayersEvent");
                    self.event_bus.emit(SelectAllLayersEvent { comp_uuid: active_comp_uuid });
                    return;
                }
                if downcast_event::<ClearLayerSelectionEvent>(&event).is_some() {
                    log::trace!("Hotkey: F2 -> ClearLayerSelectionEvent");
                    self.event_bus.emit(ClearLayerSelectionEvent { comp_uuid: active_comp_uuid });
                    return;
                }
                // Trim operations
                if downcast_event::<ResetTrimsEvent>(&event).is_some() {
                    log::trace!("Hotkey: Ctrl-R -> ResetTrimsEvent");
                    self.event_bus.emit(ResetTrimsEvent { comp_uuid: active_comp_uuid });
                    return;
                }
            }

            self.event_bus.emit_boxed(event);
            return; // Hotkey handled, don't process manual checks
        }

        // Debug: log when F or A is pressed but no event
        if input.key_pressed(egui::Key::F) || input.key_pressed(egui::Key::A) {
            log::info!(
                "F/A pressed NO EVENT. focused={:?} vp={} tl={} ne_tab={} ne_hover={} pj={}",
                focused_window,
                self.viewport_hovered,
                self.timeline_hovered,
                self.node_editor_tab_active,
                self.node_editor_hovered,
                self.project_hovered
            );
        }

        // ESC: Priority-based handler. ESC: fullscreen -> encode dialog -> settings -> quit.
        if input.key_pressed(egui::Key::Escape) {
            // Priority 1: Fullscreen/Cinema mode (highest priority - most immersive state)
            if input.key_pressed(egui::Key::Escape) && self.is_fullscreen {
                self.set_cinema_mode(ctx, false);
            }
            // Priority 2: Encode dialog (modal dialog should be dismissed before app closes)
            else if input.key_pressed(egui::Key::Escape) && self.show_encode_dialog {
                // Close encode dialog (stop encoding if in progress)
                if let Some(ref mut dialog) = self.encode_dialog
                    && dialog.is_encoding()
                {
                    dialog.stop_encoding();
                }
                self.show_encode_dialog = false;
            }
            // Priority 3: Settings dialog (preferences window)
            else if input.key_pressed(egui::Key::Escape) && self.show_settings {
                self.show_settings = false;
            }
            // Priority 4: Quit application (default action when nothing else to dismiss)
            else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }

        // All other hotkeys (playback, viewport, etc.) are routed via EventBus (HotkeyHandler)
    }
}
