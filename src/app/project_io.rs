//! Project I/O and sequence loading for PlayaApp.
//!
//! Contains methods for:
//! - Loading sequences from files (load_sequences)
//! - Saving/loading projects (save_project, load_project, quick_save)
//! - File dialogs (show_open_project_dialog)
//! - Frame preloading (enqueue_frame_loads_around_playhead)

use super::PlayaApp;
use crate::entities::FileNode;
use crate::entities::node::Node;

use log::{error, info, trace, warn};
use std::path::PathBuf;
use std::sync::Arc;

impl PlayaApp {
    /// Attach composition event emitter to all comps in the current project.
    pub fn attach_comp_event_emitter(&mut self) {
        let emitter = self.comp_event_emitter.clone();
        for arc_node in self.project.media.write().expect("media lock poisoned").values_mut() {
            // Arc::make_mut: copy-on-write if other refs exist (rare at startup)
            let node = std::sync::Arc::make_mut(arc_node);
            node.set_event_emitter(emitter.clone());
        }
    }

    /// Load sequences from file paths and append to player/project.
    ///
    /// Detects sequences from provided paths, appends them to the player project,
    /// and clears any error messages on success.
    ///
    /// # Arguments
    /// * `paths` - Vector of file paths to detect sequences from
    ///
    /// # Returns
    /// * `Ok(())` - Sequences loaded successfully
    /// * `Err(String)` - Detection or loading failed with error message
    pub fn load_sequences(&mut self, paths: Vec<PathBuf>) -> Result<(), String> {
        match FileNode::detect_from_paths(paths) {
            Ok(nodes) => {
                if nodes.is_empty() {
                    let error_msg = "No valid sequences detected".to_string();
                    warn!("{}", error_msg);
                    self.error_msg = Some(error_msg.clone());
                    return Err(error_msg);
                }

                // Add all detected sequences to unified media pool
                let nodes_count = nodes.len();
                let mut first_uuid: Option<uuid::Uuid> = None;
                for node in nodes {
                    let uuid = node.uuid();
                    let name = node.name().to_string();
                    let frames = node.frame_count();
                    let (start, end) = node.play_range(true);
                    info!("Adding FileNode: {} ({}) frames={} range={}..{}", name, uuid, frames, start, end);

                    // add_node() adds to media pool and order
                    self.project.add_node(node.into());

                    // Remember first sequence for activation
                    if self.player.active_comp().is_none() && first_uuid.is_none() {
                        first_uuid = Some(uuid);
                    }
                }

                self.attach_comp_event_emitter();

                // Activate first sequence and trigger frame loading
                if let Some(uuid) = first_uuid {
                    self.player.set_active_comp(Some(uuid), &mut self.project);
                    let total = self.player.total_frames(&self.project);
                    let range = self.player.play_range(&self.project);
                    info!("After activation: total_frames={} play_range={:?}", total, range);
                    self.node_editor_state.set_comp(uuid);
                    self.node_editor_state.mark_dirty();
                    self.enqueue_frame_loads_around_playhead(self.settings.preload_radius);
                }

                self.error_msg = None;
                info!("Loaded {} clip(s)", nodes_count);
                Ok(())
            }
            Err(e) => {
                let error_msg = format!("Failed to load sequences: {}", e);
                warn!("{}", error_msg);
                self.error_msg = Some(error_msg.clone());
                Err(error_msg)
            }
        }
    }

    /// Enqueue frame loading around playhead for active comp.
    ///
    /// Unified interface: works for both File mode and Layer mode.
    /// Enqueue only the current frame for immediate loading.
    /// Used during attribute changes - shows result immediately while
    /// debounced preloader schedules full preload after delay.
    pub fn enqueue_current_frame_only(&self) {
        self.enqueue_frame_loads_around_playhead(0);
    }

    /// File mode: loads frames from disk using spiral/forward strategies
    /// Layer mode: composes frames from children (on-demand for now)
    ///
    /// # Arguments
    /// * `radius` - Frames around playhead to preload (-1 = entire comp, 0 = current only)
    pub fn enqueue_frame_loads_around_playhead(&self, radius: i32) {
        // Get active comp
        let Some(comp_uuid) = self.player.active_comp() else {
            trace!("No active comp for frame loading");
            return;
        };

        // -1 means load entire comp (use i32::MAX, will be capped by work_area)
        let effective_radius = if radius < 0 { i32::MAX } else { radius };

        // Trigger preload (works for both File and Layer modes)
        trace!("[PRELOAD] enqueue_frame_loads: comp={}, radius={}", comp_uuid, effective_radius);
        self.project.with_comp(comp_uuid, |comp| {
            comp.signal_preload(&self.workers, &self.project, effective_radius);
        });
    }

    /// Save project to JSON file.
    pub fn save_project(&mut self, path: PathBuf) {
        if let Err(e) = self.project.to_json(&path) {
            error!("{}", e);
            self.error_msg = Some(e);
        } else {
            self.project.set_last_save_path(Some(path.clone()));
            info!("Saved project to {}", path.display());
        }
    }

    /// Quick save - saves to last path or shows dialog.
    pub fn quick_save(&mut self) {
        if let Some(path) = self.project.last_save_path() {
            info!("Quick save to {}", path.display());
            self.save_project(path);
        } else {
            // No previous save path - show file dialog
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Playa Project", &["playa"])
                .add_filter("JSON", &["json"])
                .set_file_name("project.playa")
                .save_file()
            {
                self.save_project(path);
            }
        }
    }

    /// Show open project dialog.
    pub fn show_open_project_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Playa Project", &["playa", "json"])
            .pick_file()
        {
            self.load_project(path);
        }
    }

    /// Load project from JSON file.
    pub fn load_project(&mut self, path: PathBuf) {
        match crate::entities::Project::from_json(&path) {
            Ok(mut project) => {
                info!("Loaded project from {}", path.display());

                // Attach schemas (not serialized)
                project.attach_schemas();
                
                // Rebuild runtime + set cache manager (unified)
                project.rebuild_with_manager(
                    Arc::clone(&self.cache_manager),
                    self.settings.cache_strategy,
                    Some(self.comp_event_emitter.clone()),
                );
                // Set event emitter for auto-emit of AttrsChangedEvent
                project.set_event_emitter(self.event_bus.emitter());

                self.project = project;
                // Restore active comp from project (also sync selection)
                if let Some(active) = self.project.active() {
                    self.player.set_active_comp(Some(active), &mut self.project);
                    self.node_editor_state.set_comp(active);
                } else {
                    // Ensure default if none
                    let uuid = self.project.ensure_default_comp();
                    self.player.set_active_comp(Some(uuid), &mut self.project);
                    self.node_editor_state.set_comp(uuid);
                }
                self.selected_media_uuid = self.project.selection().last().cloned();
                self.error_msg = None;

                // Mark active comp as dirty to trigger preload via centralized dirty check
                if let Some(active) = self.player.active_comp() {
                    self.project.modify_comp(active, |comp| {
                        comp.attrs.mark_dirty();
                    });
                }
            }
            Err(e) => {
                error!("{}", e);
                self.error_msg = Some(e);
            }
        }
    }
}
