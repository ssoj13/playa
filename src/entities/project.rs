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
use crate::global_cache::{CacheStrategy, GlobalFrameCache};

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

    /// Global frame cache (runtime-only, replaces per-Comp local caches)
    #[serde(skip)]
    pub global_cache: Option<Arc<GlobalFrameCache>>,
}

impl Project {
    /// Default compositor constructor for serde
    fn default_compositor() -> RefCell<CompositorType> {
        RefCell::new(CompositorType::default())
    }

    pub fn new(cache_manager: Arc<CacheManager>) -> Self {
        Self::new_with_strategy(cache_manager, CacheStrategy::All)
    }

    pub fn new_with_strategy(cache_manager: Arc<CacheManager>, strategy: CacheStrategy) -> Self {
        log::info!("Project::new_with_strategy() called with cache_manager, strategy={:?}", strategy);

        // Create global frame cache with specified capacity and strategy
        let global_cache = Arc::new(GlobalFrameCache::new(
            10000,                   // Default capacity: 10k frames
            Arc::clone(&cache_manager),
            strategy,
        ));

        Self {
            attrs: Attrs::new(),
            media: HashMap::new(),
            comps_order: Vec::new(),
            selection: Vec::new(),
            active: None,
            selection_anchor: None,
            compositor: RefCell::new(CompositorType::default()), // CPU compositor by default
            cache_manager: Some(cache_manager),
            global_cache: Some(global_cache),
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
    /// - Reinitializes compositor to default (CPU).
    /// - Sets event sender and global_cache for all comps.
    pub fn rebuild_runtime(&mut self, event_sender: Option<crate::events::CompEventSender>) {
        // Reinitialize compositor (not serialized)
        *self.compositor.borrow_mut() = CompositorType::default();

        // Rebuild comps in unified media HashMap
        for comp in self.media.values_mut() {
            // NOTE: No need to clear cache - GlobalFrameCache is project-level
            // Cache will be naturally invalidated via dirty tracking

            // Set event sender for comps if provided
            if let Some(ref sender) = event_sender {
                comp.set_event_sender(sender.clone());
            }

            // Set global_cache reference for each comp
            if let Some(ref cache) = self.global_cache {
                comp.set_global_cache(Arc::clone(cache));
            }
        }
    }

    /// Rebuild runtime state AND set cache manager (unified after deserialization).
    ///
    /// Combines set_cache_manager() + rebuild_runtime() in correct order.
    /// Use this after Project::from_json() or Project.clone().
    pub fn rebuild_with_manager(
        &mut self,
        manager: Arc<CacheManager>,
        event_sender: Option<crate::events::CompEventSender>,
    ) {
        log::info!("Project::rebuild_with_manager() - unified rebuild");
        self.set_cache_manager(manager.clone());

        // Create global frame cache
        let global_cache = Arc::new(GlobalFrameCache::new(
            10000,
            manager,
            CacheStrategy::All,
        ));
        self.global_cache = Some(global_cache);

        self.rebuild_runtime(event_sender);
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

    /// Add a composition to the project (automatically sets cache_manager and global_cache)
    pub fn add_comp(&mut self, mut comp: Comp) {
        // Automatically set cache_manager if available
        if let Some(ref manager) = self.cache_manager {
            comp.set_cache_manager(Arc::clone(manager));
        }

        // Automatically set global_cache if available
        if let Some(ref cache) = self.global_cache {
            comp.set_global_cache(Arc::clone(cache));
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

    /// Cascade invalidation: mark all parent comps as dirty
    ///
    /// When a comp's attrs are modified, all parent comps that reference it
    /// must be invalidated to force recomposition.
    /// This method recursively traverses up the parent hierarchy.
    pub fn invalidate_cascade(&mut self, comp_uuid: &str) {
        log::debug!("Cascade invalidation starting from comp: {}", comp_uuid);

        // Find all parents recursively
        let mut parents_to_invalidate = Vec::new();
        let mut current_uuid = comp_uuid.to_string();

        loop {
            // Find parent of current comp
            let parent_uuid = self.media.get(&current_uuid)
                .and_then(|comp| comp.parent.clone());

            match parent_uuid {
                Some(parent) => {
                    parents_to_invalidate.push(parent.clone());
                    current_uuid = parent;
                }
                None => break, // Reached root
            }
        }

        // Mark all parents as dirty and clear their caches
        for parent_uuid in &parents_to_invalidate {
            if let Some(parent_comp) = self.media.get_mut(parent_uuid) {
                log::debug!("Invalidating parent comp: {}", parent_uuid);
                parent_comp.attrs.mark_dirty();

                // Clear cached frames for this parent comp
                if let Some(ref cache) = self.global_cache {
                    cache.clear_comp(parent_uuid);
                }
            }
        }

        log::debug!(
            "Cascade invalidation complete: {} parents invalidated",
            parents_to_invalidate.len()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache_man::CacheManager;

    #[test]
    fn test_cascade_invalidation() {
        let manager = Arc::new(CacheManager::new(0.75, 2.0));
        let mut project = Project::new(manager);

        // Create hierarchy: grandparent -> parent -> child
        let mut child = Comp::new("Child", 0, 100, 24.0);
        let child_uuid = child.uuid.clone();

        let mut parent = Comp::new("Parent", 0, 100, 24.0);
        let parent_uuid = parent.uuid.clone();
        parent.children.push(child_uuid.clone());
        child.parent = Some(parent_uuid.clone());

        let mut grandparent = Comp::new("Grandparent", 0, 100, 24.0);
        let grandparent_uuid = grandparent.uuid.clone();
        grandparent.children.push(parent_uuid.clone());
        parent.parent = Some(grandparent_uuid.clone());

        // Add comps to project
        project.media.insert(child_uuid.clone(), child);
        project.media.insert(parent_uuid.clone(), parent);
        project.media.insert(grandparent_uuid.clone(), grandparent);

        // Clear dirty flags (Comp::new() marks attrs as dirty)
        project.media.get_mut(&child_uuid).unwrap().attrs.clear_dirty();
        project.media.get_mut(&parent_uuid).unwrap().attrs.clear_dirty();
        project.media.get_mut(&grandparent_uuid).unwrap().attrs.clear_dirty();

        // Mark only child as dirty
        project.media.get_mut(&child_uuid).unwrap().attrs.mark_dirty();
        assert!(project.media.get(&child_uuid).unwrap().attrs.is_dirty());

        // Parent and grandparent should be clean
        assert!(!project.media.get(&parent_uuid).unwrap().attrs.is_dirty());
        assert!(!project.media.get(&grandparent_uuid).unwrap().attrs.is_dirty());

        // Trigger cascade invalidation from child
        project.invalidate_cascade(&child_uuid);

        // After cascade, both parent and grandparent should be dirty
        assert!(
            project.media.get(&parent_uuid).unwrap().attrs.is_dirty(),
            "Parent should be dirty after cascade"
        );
        assert!(
            project.media.get(&grandparent_uuid).unwrap().attrs.is_dirty(),
            "Grandparent should be dirty after cascade"
        );
    }

    #[test]
    fn test_cascade_invalidation_no_parents() {
        let manager = Arc::new(CacheManager::new(0.75, 2.0));
        let mut project = Project::new(manager);

        // Create orphan comp (no parents)
        let mut comp = Comp::new("Orphan", 0, 100, 24.0);
        let comp_uuid = comp.uuid.clone();
        comp.attrs.mark_dirty();

        project.media.insert(comp_uuid.clone(), comp);

        // Cascade invalidation should not crash
        project.invalidate_cascade(&comp_uuid);

        // Comp should still be dirty
        assert!(project.media.get(&comp_uuid).unwrap().attrs.is_dirty());
    }
}
