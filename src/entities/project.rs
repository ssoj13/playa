//! Project: top-level scene container (playlist).
//!
//! Holds clips (MediaPool) and compositions (Comps) that reference clips.
//! Project is the unit of serialization: scenes are saved and loaded via
//! `Project::to_json` / `Project::from_json`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{Attrs, Comp, CompositorType};
use crate::cache_man::CacheManager;

/// Top-level project / scene.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    /// Global project attributes (fps defaults, resolution presets, etc.)
    pub attrs: Attrs,

    /// Unified media pool: all comps (both Layer and File modes) keyed by UUID
    pub media: HashMap<String, Comp>,

    /// Order for all media (clips + comps) in UI (UUIDs)
    pub comps_order: Vec<String>,

    /// Current selection (ordered UUIDs)
    #[serde(default)]
    pub selection: Vec<String>,

    /// Currently active item (UUID)
    #[serde(default)]
    pub active: Option<String>,

    /// Runtime-only selection anchor for shift-click range
    #[serde(skip)]
    #[serde(default)]
    pub selection_anchor: Option<usize>,

    /// Frame compositor (runtime-only, not serialized)
    /// Used by Comp.compose() for multi-layer blending
    /// Uses RefCell for interior mutability (GPU compositor needs mutable access)
    #[serde(skip)]
    #[serde(default = "Project::default_compositor")]
    pub compositor: RefCell<CompositorType>,

    /// Global cache manager (runtime-only, set on creation/load)
    #[serde(skip)]
    cache_manager: Option<Arc<CacheManager>>,
}

impl Project {
    /// Default compositor constructor for serde
    fn default_compositor() -> RefCell<CompositorType> {
        RefCell::new(CompositorType::default())
    }

    pub fn new(cache_manager: Arc<CacheManager>) -> Self {
        log::info!("Project::new() called with cache_manager");
        Self {
            attrs: Attrs::new(),
            media: HashMap::new(),
            comps_order: Vec::new(),
            selection: Vec::new(),
            active: None,
            selection_anchor: None,
            compositor: RefCell::new(CompositorType::default()), // CPU compositor by default
            cache_manager: Some(cache_manager),
        }
    }

    /// Serialize project to JSON file.
    pub fn to_json<P: AsRef<Path>>(&self, path: P) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Serialize project error: {}", e))?;

        let path = path.as_ref();
        let path = if path.extension().and_then(|s| s.to_str()) != Some("json") {
            path.with_extension("json")
        } else {
            path.to_path_buf()
        };

        fs::write(&path, json).map_err(|e| format!("Write project error: {}", e))?;
        Ok(())
    }

    /// Load project from JSON file and rebuild runtime-only state (caches, Arc links).
    pub fn from_json<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let json =
            fs::read_to_string(path.as_ref()).map_err(|e| format!("Read project error: {}", e))?;

        let mut project: Project =
            serde_json::from_str(&json).map_err(|e| format!("Parse project error: {}", e))?;

        // Rebuild without event sender (caller must set it)
        project.rebuild_runtime(None);
        Ok(project)
    }

    /// Ensure project has at least one composition.
    /// Creates "Main" comp if none exist, and sets it as active.
    ///
    /// Returns UUID of the default/first comp.
    pub fn ensure_default_comp(&mut self) -> String {
        // Check if we have any comps in media (Layer mode comps in comps_order)
        let has_comps = !self.comps_order.is_empty();

        if !has_comps {
            let comp = Comp::new("Main", 0, 0, 24.0);
            let uuid = comp.uuid.clone();
            self.media.insert(uuid.clone(), comp);
            self.comps_order.push(uuid.clone());
            log::info!("Created default comp: {}", uuid);
            uuid
        } else {
            // Return first comp UUID from order
            self.comps_order.first().cloned().unwrap_or_else(|| {
                // Fallback: create new if order is broken
                let comp = Comp::new("Main", 0, 0, 24.0);
                let uuid = comp.uuid.clone();
                self.media.insert(uuid.clone(), comp);
                self.comps_order.push(uuid.clone());
                uuid
            })
        }
    }

    /// Rebuild runtime-only state after deserialization.
    ///
    /// - Clears per-comp caches.
    /// - Reinitializes compositor to default (CPU).
    /// - Sets event sender for all comps.
    pub fn rebuild_runtime(&mut self, event_sender: Option<crate::events::CompEventSender>) {
        // Reinitialize compositor (not serialized)
        *self.compositor.borrow_mut() = CompositorType::default();

        // Rebuild comps in unified media HashMap
        for comp in self.media.values_mut() {
            comp.clear_cache();

            // Set event sender for comps if provided
            if let Some(ref sender) = event_sender {
                comp.set_event_sender(sender.clone());
            }
        }
    }

    /// Set compositor type (CPU or GPU).
    ///
    /// Allows switching between CPU and GPU compositing backends.
    /// GPU compositor requires OpenGL context.
    pub fn set_compositor(&self, compositor: CompositorType) {
        log::info!("Compositor changed to: {:?}", compositor);
        *self.compositor.borrow_mut() = compositor;
    }

    /// Get mutable reference to a composition by UUID.
    pub fn get_comp_mut(&mut self, uuid: &str) -> Option<&mut Comp> {
        self.media.get_mut(uuid)
    }

    /// Get immutable reference to a composition by UUID.
    pub fn get_comp(&self, uuid: &str) -> Option<&Comp> {
        self.media.get(uuid)
    }

    /// Add a composition to the project (automatically sets cache_manager)
    pub fn add_comp(&mut self, mut comp: Comp) {
        // Automatically set cache_manager if available
        if let Some(ref manager) = self.cache_manager {
            comp.set_cache_manager(Arc::clone(manager));
        }

        let uuid = comp.uuid.clone();
        self.media.insert(uuid.clone(), comp);
        self.comps_order.push(uuid);
    }

    /// Set CacheManager for project and all existing comps (call after deserialization)
    pub fn set_cache_manager(&mut self, manager: Arc<CacheManager>) {
        log::info!("Project::set_cache_manager() called, setting for {} comps", self.media.len());
        self.cache_manager = Some(Arc::clone(&manager));
        for comp in self.media.values_mut() {
            comp.set_cache_manager(Arc::clone(&manager));
        }
    }

    /// Get reference to cache manager
    pub fn cache_manager(&self) -> Option<&Arc<CacheManager>> {
        if self.cache_manager.is_none() {
            log::warn!("Project::cache_manager() returning None!");
        }
        self.cache_manager.as_ref()
    }

    /// Remove media (clip or comp) by UUID.
    pub fn remove_media(&mut self, uuid: &str) {
        self.media.remove(uuid);
        self.comps_order.retain(|u| u != uuid);
    }
}
