//! Project: top-level scene container (playlist).
//!
//! Holds clips (MediaPool) and compositions (Comps) that reference clips.
//! Project is the unit of serialization: scenes are saved and loaded via
//! `Project::to_json` / `Project::from_json`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{Attrs, Comp, CompositorType};
use crate::cache_man::CacheManager;
use crate::global_cache::{CacheStrategy, GlobalFrameCache};

/// Top-level project / scene.
///
/// **Attrs keys** (stored in `attrs`):
/// - `comps_order`: Vec<Uuid> as JSON - UI order of media items
/// - `selection`: Vec<Uuid> as JSON - current selection (ordered)
/// - `active`: Option<Uuid> as JSON - currently active item
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    /// All serializable project state (includes comps_order, selection, active)
    pub attrs: Attrs,

    /// Unified media pool: all comps (both Layer and File modes) keyed by UUID
    /// Thread-safe for concurrent reads from background composition workers
    #[serde(with = "arc_rwlock_hashmap")]
    pub media: Arc<RwLock<HashMap<Uuid, Comp>>>,

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

        // Initialize attrs with default values
        let mut attrs = Attrs::new();
        attrs.set_json("comps_order", &Vec::<Uuid>::new());
        attrs.set_json("selection", &Vec::<Uuid>::new());
        attrs.set_json("active", &None::<Uuid>);

        Self {
            attrs,
            media: Arc::new(RwLock::new(HashMap::new())),
            selection_anchor: None,
            compositor: RefCell::new(CompositorType::default()), // CPU compositor by default
            cache_manager: Some(cache_manager),
            global_cache: Some(global_cache),
        }
    }

    // === Accessor methods for attrs fields ===

    /// Get comps order (Vec<Uuid>)
    pub fn comps_order(&self) -> Vec<Uuid> {
        self.attrs.get_json("comps_order").unwrap_or_default()
    }

    /// Set comps order
    pub fn set_comps_order(&mut self, order: Vec<Uuid>) {
        self.attrs.set_json("comps_order", &order);
    }

    /// Push UUID to comps_order
    pub fn push_comps_order(&mut self, uuid: Uuid) {
        let mut order = self.comps_order();
        order.push(uuid);
        self.set_comps_order(order);
    }

    /// Retain comps_order by predicate
    pub fn retain_comps_order<F>(&mut self, f: F) where F: FnMut(&Uuid) -> bool {
        let mut order = self.comps_order();
        order.retain(f);
        self.set_comps_order(order);
    }

    /// Get selection (Vec<Uuid>)
    pub fn selection(&self) -> Vec<Uuid> {
        self.attrs.get_json("selection").unwrap_or_default()
    }

    /// Set selection
    pub fn set_selection(&mut self, sel: Vec<Uuid>) {
        self.attrs.set_json("selection", &sel);
    }

    /// Push UUID to selection
    pub fn push_selection(&mut self, uuid: Uuid) {
        let mut sel = self.selection();
        sel.push(uuid);
        self.set_selection(sel);
    }

    /// Retain selection by predicate
    pub fn retain_selection<F>(&mut self, f: F) where F: FnMut(&Uuid) -> bool {
        let mut sel = self.selection();
        sel.retain(f);
        self.set_selection(sel);
    }

    /// Get active comp UUID
    pub fn active(&self) -> Option<Uuid> {
        self.attrs.get_json("active").unwrap_or(None)
    }

    /// Set active comp UUID
    pub fn set_active(&mut self, uuid: Option<Uuid>) {
        self.attrs.set_json("active", &uuid);
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
    pub fn ensure_default_comp(&mut self) -> Uuid {
        // Check if we have any comps in media (Layer mode comps in comps_order)
        let order = self.comps_order();
        let has_comps = !order.is_empty();

        if !has_comps {
            let comp = Comp::new("Main", 0, 0, 24.0);
            let uuid = comp.uuid;
            self.media.write().unwrap().insert(uuid, comp);
            self.push_comps_order(uuid);
            log::info!("Created default comp: {}", uuid);
            uuid
        } else {
            // Return first comp UUID from order
            order.first().copied().unwrap_or_else(|| {
                // Fallback: create new if order is broken
                let comp = Comp::new("Main", 0, 0, 24.0);
                let uuid = comp.uuid;
                self.media.write().unwrap().insert(uuid, comp);
                self.push_comps_order(uuid);
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
        let mut media = self.media.write().unwrap();
        for comp in media.values_mut() {
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

    /// Get cloned composition by UUID.
    /// Returns owned Comp (cloned from media pool).
    pub fn get_comp(&self, uuid: Uuid) -> Option<Comp> {
        self.media.read().unwrap().get(&uuid).cloned()
    }

    /// Update composition in media pool.
    /// Replaces existing comp with same UUID.
    pub fn update_comp(&self, comp: Comp) {
        self.media.write().unwrap().insert(comp.uuid, comp);
    }

    /// Check if composition exists in media pool.
    pub fn contains_comp(&self, uuid: Uuid) -> bool {
        self.media.read().unwrap().contains_key(&uuid)
    }

    /// Modify composition in-place via closure.
    /// Acquires write lock, calls closure with mutable reference, releases lock.
    /// Returns true if comp was found and modified.
    pub fn modify_comp<F>(&self, uuid: Uuid, f: F) -> bool
    where
        F: FnOnce(&mut Comp),
    {
        if let Some(comp) = self.media.write().unwrap().get_mut(&uuid) {
            f(comp);
            true
        } else {
            false
        }
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

        let uuid = comp.uuid;
        self.media.write().unwrap().insert(uuid, comp);
        self.push_comps_order(uuid);
    }

    /// Set CacheManager for project and all existing comps (call after deserialization)
    pub fn set_cache_manager(&mut self, manager: Arc<CacheManager>) {
        let media = self.media.read().unwrap();
        log::info!("Project::set_cache_manager() called, setting for {} comps", media.len());
        drop(media); // Release read lock before acquiring write lock

        self.cache_manager = Some(Arc::clone(&manager));
        let mut media = self.media.write().unwrap();
        for comp in media.values_mut() {
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
    /// Automatically clears cached frames for this comp from global cache.
    pub fn remove_media(&mut self, uuid: Uuid) {
        // Clear cached frames for this comp first
        if let Some(ref cache) = self.global_cache {
            cache.clear_comp(uuid);
            log::debug!("Cleared cache for removed comp: {}", uuid);
        }

        // Remove from media pool and order
        self.media.write().unwrap().remove(&uuid);
        self.retain_comps_order(|u| *u != uuid);
    }

    /// Cascade invalidation: mark all parent comps as dirty
    ///
    /// When a comp's attrs are modified, all parent comps that reference it
    /// must be invalidated to force recomposition.
    /// This method recursively traverses up the parent hierarchy.
    pub fn invalidate_cascade(&mut self, comp_uuid: Uuid) {
        log::debug!("Cascade invalidation starting from comp: {}", comp_uuid);

        // Find all parents recursively
        let mut parents_to_invalidate = Vec::new();
        let mut current_uuid = comp_uuid;

        {
            let media = self.media.read().unwrap();
            loop {
                // Find parent of current comp
                let parent_uuid = media.get(&current_uuid)
                    .and_then(|comp| comp.parent);

                match parent_uuid {
                    Some(parent) => {
                        parents_to_invalidate.push(parent);
                        current_uuid = parent;
                    }
                    None => break, // Reached root
                }
            }
        } // Release read lock

        // Mark all parents as dirty and clear their caches
        let mut media = self.media.write().unwrap();
        for parent_uuid in &parents_to_invalidate {
            if let Some(parent_comp) = media.get_mut(parent_uuid) {
                log::debug!("Invalidating parent comp: {}", parent_uuid);
                parent_comp.attrs.mark_dirty();

                // Clear cached frames for this parent comp
                if let Some(ref cache) = self.global_cache {
                    cache.clear_comp(*parent_uuid);
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
        let child_uuid = child.uuid;

        let mut parent = Comp::new("Parent", 0, 100, 24.0);
        let parent_uuid = parent.uuid;
        parent.children.push((child_uuid, crate::entities::Attrs::new()));
        child.parent = Some(parent_uuid);

        let mut grandparent = Comp::new("Grandparent", 0, 100, 24.0);
        let grandparent_uuid = grandparent.uuid;
        grandparent.children.push((parent_uuid, crate::entities::Attrs::new()));
        parent.parent = Some(grandparent_uuid);

        // Add comps to project
        {
            let mut media = project.media.write().unwrap();
            media.insert(child_uuid, child);
            media.insert(parent_uuid, parent);
            media.insert(grandparent_uuid, grandparent);
        }

        // Clear dirty flags (Comp::new() marks attrs as dirty)
        {
            let mut media = project.media.write().unwrap();
            media.get_mut(&child_uuid).unwrap().attrs.clear_dirty();
            media.get_mut(&parent_uuid).unwrap().attrs.clear_dirty();
            media.get_mut(&grandparent_uuid).unwrap().attrs.clear_dirty();
        }

        // Mark only child as dirty
        {
            let mut media = project.media.write().unwrap();
            media.get_mut(&child_uuid).unwrap().attrs.mark_dirty();
        }
        assert!(project.media.read().unwrap().get(&child_uuid).unwrap().attrs.is_dirty());

        // Parent and grandparent should be clean
        {
            let media = project.media.read().unwrap();
            assert!(!media.get(&parent_uuid).unwrap().attrs.is_dirty());
            assert!(!media.get(&grandparent_uuid).unwrap().attrs.is_dirty());
        }

        // Trigger cascade invalidation from child
        project.invalidate_cascade(child_uuid);

        // After cascade, both parent and grandparent should be dirty
        {
            let media = project.media.read().unwrap();
            assert!(
                media.get(&parent_uuid).unwrap().attrs.is_dirty(),
                "Parent should be dirty after cascade"
            );
            assert!(
                media.get(&grandparent_uuid).unwrap().attrs.is_dirty(),
                "Grandparent should be dirty after cascade"
            );
        }
    }

    #[test]
    fn test_cascade_invalidation_no_parents() {
        let manager = Arc::new(CacheManager::new(0.75, 2.0));
        let mut project = Project::new(manager);

        // Create orphan comp (no parents)
        let mut comp = Comp::new("Orphan", 0, 100, 24.0);
        let comp_uuid = comp.uuid;
        comp.attrs.mark_dirty();

        project.media.write().unwrap().insert(comp_uuid, comp);

        // Cascade invalidation should not crash
        project.invalidate_cascade(comp_uuid);

        // Comp should still be dirty
        assert!(project.media.read().unwrap().get(&comp_uuid).unwrap().attrs.is_dirty());
    }
}

// Serde helper for Arc<RwLock<HashMap<Uuid, Comp>>>
mod arc_rwlock_hashmap {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(
        map: &Arc<RwLock<HashMap<Uuid, Comp>>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        map.read().unwrap().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Arc<RwLock<HashMap<Uuid, Comp>>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let map = HashMap::<Uuid, Comp>::deserialize(deserializer)?;
        Ok(Arc::new(RwLock::new(map)))
    }
}
