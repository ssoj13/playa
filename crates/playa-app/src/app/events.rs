//! Event handling for PlayaApp.
//!
//! Contains handlers for:
//! - Event bus events (handle_events)
//! - Effect actions (handle_effect_actions)  
//! - Keyboard input (handle_keyboard_input)
//! - Focus detection (determine_focused_window)

use super::PlayaApp;
use crate::main_events::{self, AppEventContext};
use playa_engine::core::event_bus::downcast_event;
use playa_engine::entities::comp_events::*;
use playa_engine::entities::node::Node;
use playa_ui::dialogs::prefs::prefs_events::HotkeyWindow;
use playa_ui::widgets::ae::EffectAction;
use playa_ui::widgets::project::project_events::ClearCacheEvent;
use playa_ui::widgets::viewport::ViewportRefreshEvent;

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
        let mut deferred_generate_ainode: Option<uuid::Uuid> = None;
        let mut deferred_iterate_generation: Option<(uuid::Uuid, uuid::Uuid)> = None;

        // Poll all events from the bus
        let events = self.event_bus.poll();
        for event in events {
            // === Comp events (high priority, internal) ===
            if let Some(e) = downcast_event::<CurrentFrameChangedEvent>(&event) {
                trace!(
                    "Comp {} frame changed: {} → {}",
                    e.comp_uuid, e.old_frame, e.new_frame
                );
                self.enqueue_frame_loads_around_playhead(self.settings.playback.preload_radius);
                continue;
            }
            if let Some(e) = downcast_event::<LayersChangedEvent>(&event) {
                trace!(
                    "Comp {} layers changed (range: {:?})",
                    e.comp_uuid, e.affected_range
                );
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
                self.handle_attrs_changed(e.0);
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
            // Layout events - reset/select/create/delete/update/rename UI layout
            if downcast_event::<playa_engine::core::layout_events::ResetLayoutEvent>(&event)
                .is_some()
            {
                self.reset_layout();
                continue;
            }
            // Named layout events
            if let Some(evt) =
                downcast_event::<playa_engine::core::layout_events::LayoutSelectedEvent>(&event)
            {
                self.select_layout(&evt.0);
                continue;
            }
            if let Some(evt) =
                downcast_event::<playa_engine::core::layout_events::LayoutCreatedEvent>(&event)
            {
                self.create_layout(evt.0.clone());
                continue;
            }
            if let Some(evt) =
                downcast_event::<playa_engine::core::layout_events::LayoutDeletedEvent>(&event)
            {
                self.delete_layout(&evt.0);
                continue;
            }
            if downcast_event::<playa_engine::core::layout_events::LayoutUpdatedEvent>(&event)
                .is_some()
            {
                self.update_current_layout();
                continue;
            }
            if let Some(evt) =
                downcast_event::<playa_engine::core::layout_events::LayoutRenamedEvent>(&event)
            {
                self.rename_layout(&evt.0, &evt.1);
                continue;
            }
            // === App events - delegate to main_events module ===
            // log::trace!("[HANDLE] checking event type_id={:?}", (*event).type_id());
            if let Some(result) = main_events::handle_app_event(
                &event,
                &mut AppEventContext {
                    player: &mut self.player,
                    project: &mut self.project,
                    timeline_state: &mut self.timeline_state,
                    node_editor_state: &mut self.node_editor_state,
                    viewport_state: &mut self.viewport_state,
                    settings: &mut self.settings,
                    show_help: &mut self.show_help,
                    show_playlist: &mut self.show_playlist,
                    show_settings: &mut self.show_settings,
                    show_encode_dialog: &mut self.show_encode_dialog,
                    show_attributes_editor: &mut self.show_attributes_editor,
                    encode_dialog: &mut self.encode_dialog,
                    is_fullscreen: &mut self.is_fullscreen,
                    fullscreen_dirty: &mut self.fullscreen_dirty,
                    reset_settings_pending: &mut self.reset_settings_pending,
                },
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
                    deferred_load_sequences
                        .get_or_insert_with(Vec::new)
                        .extend(paths);
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
                if let Some(uuid) = result.generate_ainode {
                    deferred_generate_ainode = Some(uuid);
                }
                if let Some(pair) = result.iterate_generation {
                    deferred_iterate_generation = Some(pair);
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
            trace!(
                "[DERIVED] iteration={}, events={}",
                iteration,
                derived.len()
            );
            for event in derived {
                if let Some(e) = downcast_event::<AttrsChangedEvent>(&event) {
                    trace!("[DERIVED] AttrsChangedEvent comp={}", e.0);
                    self.handle_attrs_changed(e.0);
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
            let uuid = self
                .project
                .create_comp(&name, fps, self.comp_event_emitter.clone());
            self.player.set_active_comp(Some(uuid), &mut self.project);
            self.node_editor_state.set_comp(uuid);
            info!("Created new comp: {}", uuid);
        }
        if let Some(name) = deferred_new_camera {
            use playa_engine::entities::CameraNode;
            let camera = CameraNode::new(&name);
            let uuid = camera.uuid();
            self.project.add_node(camera.into());
            info!("Created new camera: {}", uuid);
        }
        if let Some((name, text)) = deferred_new_text {
            use playa_engine::entities::TextNode;
            let text_node = TextNode::new(&name, &text);
            let uuid = text_node.uuid();
            self.project.add_node(text_node.into());
            info!("Created new text: {}", uuid);
        }
        if deferred_enqueue_frames {
            self.enqueue_frame_loads_around_playhead(self.settings.playback.preload_radius);
        }
        if deferred_quick_save {
            self.quick_save();
        }
        if deferred_show_open {
            self.show_open_project_dialog();
        }
        #[cfg(feature = "jobs")]
        if let Some(uuid) = deferred_generate_ainode {
            self.generate_ainode(uuid);
        }
        #[cfg(feature = "jobs")]
        if let Some((ainode_uuid, parent_gen_uuid)) = deferred_iterate_generation {
            self.iterate_generation(ainode_uuid, parent_gen_uuid);
        }
        #[cfg(not(feature = "jobs"))]
        let _ = (deferred_generate_ainode, deferred_iterate_generation);
    }

    /// Iterate from a past Generation: copy its params verbatim, swap
    /// the seed for a fresh random u64, set `parent_gen_uuid` to the
    /// source's uuid, and submit. Produces a reproducibility-linked
    /// variation in one click.
    #[cfg(feature = "jobs")]
    pub fn iterate_generation(
        &mut self,
        ainode_uuid: uuid::Uuid,
        parent_gen_uuid: uuid::Uuid,
    ) {
        use playa_engine::entities::Generation;

        let Some(queue) = self.job_queue.as_ref().map(std::sync::Arc::clone) else {
            log::warn!("iterate_generation: JobQueue not initialised");
            return;
        };

        // Snapshot the source Generation we're iterating from.
        let parent = self.project.with_node(ainode_uuid, |node| {
            node.as_ai().and_then(|ai| {
                ai.generations()
                    .into_iter()
                    .find(|g| g.uuid == parent_gen_uuid)
            })
        });
        let Some(Some(parent)) = parent else {
            log::warn!("iterate_generation: parent gen {parent_gen_uuid} not found");
            return;
        };

        // Copy params verbatim, swap seed + clean linkage fields (the
        // submit path re-injects them with the NEW gen uuid).
        let mut params = match parent.params.clone() {
            serde_json::Value::Object(m) => m,
            _ => serde_json::Map::new(),
        };
        let fresh_seed = rand::random::<u64>();
        params.insert("seed".into(), serde_json::Value::from(fresh_seed));
        params.remove("ainode_uuid");
        params.remove("gen_uuid");

        let gen_uuid = uuid::Uuid::new_v4();
        let timestamp_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        params.insert(
            "ainode_uuid".into(),
            serde_json::Value::String(ainode_uuid.to_string()),
        );
        params.insert(
            "gen_uuid".into(),
            serde_json::Value::String(gen_uuid.to_string()),
        );
        let params_value = serde_json::Value::Object(params);

        match queue.submit(parent.provider.clone(), params_value.clone()) {
            Ok(job_id) => {
                let generation = Generation {
                    uuid: gen_uuid,
                    timestamp_secs,
                    provider: parent.provider.clone(),
                    provider_version: parent.provider_version.clone(),
                    params: params_value,
                    // Inherit the source's input snapshots — these
                    // captured the inputs at the parent's submit time
                    // and are the right anchor for "what was the
                    // parent based on" comparisons in regen review.
                    input_snapshots: parent.input_snapshots.clone(),
                    job_id: job_id.0,
                    request_id: None,
                    result_path: std::path::PathBuf::new(),
                    cost_usd: None,
                    parent_gen_uuid: Some(parent_gen_uuid),
                };
                let added = self.project.modify_node(ainode_uuid, |node| {
                    if let Some(ai) = node.as_ai_mut() {
                        ai.add_generation(generation);
                    }
                });
                if !added {
                    log::warn!("iterate_generation: AINode {ainode_uuid} disappeared mid-submit");
                }
                log::info!(
                    "Iterated AINode {ainode_uuid}: parent={parent_gen_uuid} → new={gen_uuid} as job {} (seed={fresh_seed})",
                    job_id.0
                );
            }
            Err(e) => {
                log::warn!("iterate_generation submit failed: {e}");
            }
        }
    }

    /// Submit a fresh `Generation` for the AINode with `ainode_uuid`.
    /// Reads the current attrs (provider, params_template, input_refs),
    /// resolves the seed to a concrete `u64` (random if absent), builds
    /// a `Generation` record with input_snapshots, dispatches the
    /// linked `JobQueue::submit`, and pushes the record onto the
    /// AINode's history (making it active).
    ///
    /// The submit's params verbatim include the params_template plus
    /// `ainode_uuid` and `gen_uuid` strings — the on-Completed listener
    /// (registered in `register_ainode_completion_listener`) reads
    /// those back to update the Generation's `result_path` when the
    /// provider finishes.
    #[cfg(feature = "jobs")]
    pub fn generate_ainode(&mut self, ainode_uuid: uuid::Uuid) {
        use playa_engine::entities::{Generation, sha256_hex};

        let Some(queue) = self.job_queue.as_ref().map(std::sync::Arc::clone) else {
            log::warn!("generate_ainode: JobQueue not initialised");
            return;
        };

        // Snapshot the AINode's attrs (provider + params + refs).
        let snapshot = self.project.with_node(ainode_uuid, |node| {
            node.as_ai().map(|ai| {
                (
                    ai.provider(),
                    ai.params_template(),
                    ai.input_refs(),
                )
            })
        });
        let Some(Some((provider, params_template, input_refs))) = snapshot else {
            log::warn!("generate_ainode: {ainode_uuid} not found or not an AINode");
            return;
        };

        // Resolve auto-fields. Right now `seed` is the only one — if
        // params_template has it set use it verbatim, otherwise pick
        // a fresh u64. Result is stored in the Generation's params so
        // "Regenerate exact" reproduces the same bytes.
        let mut params = match params_template {
            serde_json::Value::Object(map) => map,
            _ => serde_json::Map::new(),
        };
        let resolved_seed = params
            .get("seed")
            .and_then(|v| v.as_u64())
            .unwrap_or_else(rand::random::<u64>);
        params.insert("seed".into(), serde_json::Value::from(resolved_seed));

        // Build input snapshots. For each RefNode in input_refs:
        // - Resolve target uuid + channel.
        // - If target is a FileNode → hash the file content at the
        //   active frame (frame 0 for single-frame stills, the
        //   sequence start otherwise). This is the canonical
        //   reproducibility signal: regen with the same file bytes
        //   yields the same output.
        // - Other target kinds (CompNode, AINode, RefNode-chain) fall
        //   back to hashing the empty string. v9.2 can do a render +
        //   pixel-buffer hash here when justified.
        let mut snapshots = Vec::with_capacity(input_refs.len());
        for &ref_uuid in &input_refs {
            let resolved = self.project.with_node(ref_uuid, |node| {
                node.as_ref_node().map(|r| {
                    (
                        r.target().unwrap_or_else(uuid::Uuid::nil),
                        r.channel(),
                    )
                })
            });
            if let Some(Some((target_uuid, channel))) = resolved {
                let hash = self
                    .project
                    .with_node(target_uuid, |node| {
                        if let Some(file) = node.as_file()
                            && let Some(path) = file.resolve_frame_path(0)
                            && let Ok(bytes) = std::fs::read(&path)
                        {
                            return sha256_hex(&bytes);
                        }
                        sha256_hex(b"")
                    })
                    .unwrap_or_else(|| sha256_hex(b""));
                snapshots.push(playa_engine::entities::RefSnapshot {
                    ref_uuid,
                    target_uuid,
                    target_content_hash: hash,
                    channel,
                });
            }
        }

        let gen_uuid = uuid::Uuid::new_v4();
        let timestamp_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Inject linkage so the on-Completed listener finds us.
        params.insert(
            "ainode_uuid".into(),
            serde_json::Value::String(ainode_uuid.to_string()),
        );
        params.insert(
            "gen_uuid".into(),
            serde_json::Value::String(gen_uuid.to_string()),
        );
        let params_value = serde_json::Value::Object(params);

        let submit_params = params_value.clone();
        match queue.submit(provider.clone(), submit_params) {
            Ok(job_id) => {
                let generation = Generation {
                    uuid: gen_uuid,
                    timestamp_secs,
                    provider: provider.clone(),
                    provider_version: None,
                    params: params_value,
                    input_snapshots: snapshots,
                    job_id: job_id.0,
                    request_id: None,
                    result_path: std::path::PathBuf::new(),
                    cost_usd: None,
                    parent_gen_uuid: None,
                };
                let added = self.project.modify_node(ainode_uuid, |node| {
                    if let Some(ai) = node.as_ai_mut() {
                        ai.add_generation(generation);
                    }
                });
                if !added {
                    log::warn!(
                        "generate_ainode: AINode {ainode_uuid} disappeared between snapshot and add_generation"
                    );
                }
                log::info!(
                    "Submitted AINode {ainode_uuid} gen {gen_uuid} as job {} (provider={provider})",
                    job_id.0
                );
            }
            Err(e) => {
                log::warn!("generate_ainode submit failed: {e}");
            }
        }
    }

    /// Handle AttrsChangedEvent: invalidate cache, schedule preload, request refresh.
    ///
    /// Shared by the main event loop and the derived-events loop.
    fn handle_attrs_changed(&mut self, comp_uuid: uuid::Uuid) {
        trace!(
            "Comp {} attrs changed - triggering cascade invalidation",
            comp_uuid
        );
        // 1. Increment epoch to cancel pending worker tasks (stale data prevention)
        if let Some(manager) = self.project.cache_manager() {
            manager.increment_epoch();
        }
        // 2. Clear all cached frames - any attribute could affect rendering
        if let Some(ref cache) = self.project.global_cache {
            cache.clear_comp(comp_uuid, true, None);
        }
        // 3. Debounced preload: current frame immediately, full preload after delay
        //    This prevents flooding cache with requests during rapid slider scrubbing
        self.enqueue_current_frame_only();
        self.debounced_preloader.schedule(comp_uuid);
        // 4. Request viewport refresh
        self.event_bus.emit(ViewportRefreshEvent);
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
        use playa_engine::entities::effects::Effect;

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
                            if let Some(effect) =
                                layer.effects.iter_mut().find(|e| e.uuid == effect_uuid)
                            {
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
                            if let Some(effect) =
                                layer.effects.iter_mut().find(|e| e.uuid == effect_uuid)
                            {
                                effect.collapsed = !effect.collapsed;
                            }
                        }
                    });
                }
                EffectAction::AttrChanged(effect_uuid, key, value) => {
                    self.project.modify_comp(comp_uuid, |comp| {
                        if let Some(layer) = comp.get_layer_mut(layer_uuid) {
                            if let Some(effect) =
                                layer.effects.iter_mut().find(|e| e.uuid == effect_uuid)
                            {
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
                            if let Some(idx) =
                                layer.effects.iter().position(|e| e.uuid == effect_uuid)
                            {
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
                            if let Some(idx) =
                                layer.effects.iter().position(|e| e.uuid == effect_uuid)
                            {
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
        self.hotkey_handler.set_focused_window(focused_window);

        // Try hotkey handler first (for context-aware hotkeys)
        if let Some(event) = self.hotkey_handler.handle_input(&input) {
            use playa_engine::entities::comp_events::{
                AlignLayersEndEvent, AlignLayersStartEvent, ClearLayerSelectionEvent,
                CopyLayersEvent, DuplicateLayersEvent, PasteLayersEvent, ResetTrimsEvent,
                SelectAllLayersEvent, TrimLayersEndEvent, TrimLayersStartEvent,
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
                    self.event_bus.emit(DuplicateLayersEvent {
                        comp_uuid: active_comp_uuid,
                    });
                    return;
                }
                if downcast_event::<CopyLayersEvent>(&event).is_some() {
                    log::trace!("Hotkey: Ctrl-C -> CopyLayersEvent");
                    self.event_bus.emit(CopyLayersEvent {
                        comp_uuid: active_comp_uuid,
                    });
                    return;
                }
                if downcast_event::<PasteLayersEvent>(&event).is_some() {
                    // Get current playhead position for paste target
                    let target_frame = self
                        .project
                        .with_comp(active_comp_uuid, |c| c.frame())
                        .unwrap_or(0);
                    log::trace!(
                        "Hotkey: Ctrl-V -> PasteLayersEvent at frame {}",
                        target_frame
                    );
                    self.event_bus.emit(PasteLayersEvent {
                        comp_uuid: active_comp_uuid,
                        target_frame,
                    });
                    return;
                }
                // Selection operations
                if downcast_event::<SelectAllLayersEvent>(&event).is_some() {
                    log::trace!("Hotkey: Ctrl-A -> SelectAllLayersEvent");
                    self.event_bus.emit(SelectAllLayersEvent {
                        comp_uuid: active_comp_uuid,
                    });
                    return;
                }
                if downcast_event::<ClearLayerSelectionEvent>(&event).is_some() {
                    log::trace!("Hotkey: F2 -> ClearLayerSelectionEvent");
                    self.event_bus.emit(ClearLayerSelectionEvent {
                        comp_uuid: active_comp_uuid,
                    });
                    return;
                }
                // Trim operations
                if downcast_event::<ResetTrimsEvent>(&event).is_some() {
                    log::trace!("Hotkey: Ctrl-R -> ResetTrimsEvent");
                    self.event_bus.emit(ResetTrimsEvent {
                        comp_uuid: active_comp_uuid,
                    });
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
