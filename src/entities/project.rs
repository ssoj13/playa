//! Project: top-level scene container (playlist).
//!
//! Holds clips (MediaPool) and compositions (Comps) that reference clips.
//! Project is the unit of serialization: scenes are saved and loaded via
//! `Project::to_json` / `Project::from_json`.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{Attrs, Comp, CompositorType};
use crate::core::cache_man::CacheManager;
use crate::core::global_cache::{CacheStrategy, GlobalFrameCache};

/// Top-level project / scene.
///
/// **Attrs keys** (stored in `attrs`):
/// - `comps_order`: Vec<Uuid> as JSON - UI order of media items
/// - `selection`: Vec<Uuid> as JSON - current selection (ordered)
/// - `active`: Option<Uuid> as JSON - currently active item
#[derive(Debug, Serialize, Deserialize)]
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
    /// Uses Mutex for thread-safe interior mutability
    #[serde(skip)]
    #[serde(default = "Project::default_compositor")]
    pub compositor: Mutex<CompositorType>,

    /// Global cache manager (runtime-only, set on creation/load)
    #[serde(skip)]
    cache_manager: Option<Arc<CacheManager>>,

    /// Global frame cache (runtime-only, replaces per-Comp local caches)
    #[serde(skip)]
    pub global_cache: Option<Arc<GlobalFrameCache>>,

    /// Last save path for quick save (runtime-only)
    #[serde(skip)]
    last_save_path: Option<std::path::PathBuf>,
}

impl Clone for Project {
    fn clone(&self) -> Self {
        Self {
            attrs: self.attrs.clone(),
            media: Arc::clone(&self.media),
            selection_anchor: self.selection_anchor,
            // Clone compositor by locking and cloning inner value
            compositor: Mutex::new(
                self.compositor.lock().unwrap_or_else(|e| e.into_inner()).clone()
            ),
            cache_manager: self.cache_manager.clone(),
            global_cache: self.global_cache.clone(),
            last_save_path: self.last_save_path.clone(),
        }
    }
}

impl Project {
    /// Default compositor constructor for serde
    fn default_compositor() -> Mutex<CompositorType> {
        Mutex::new(CompositorType::default())
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
            compositor: Mutex::new(CompositorType::default()), // CPU compositor by default
            cache_manager: Some(cache_manager),
            global_cache: Some(global_cache),
            last_save_path: None,
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

    /// Get last save path for quick save
    pub fn last_save_path(&self) -> Option<std::path::PathBuf> {
        self.last_save_path.clone()
    }

    /// Set last save path
    pub fn set_last_save_path(&mut self, path: Option<std::path::PathBuf>) {
        self.last_save_path = path;
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
            let uuid = comp.get_uuid();
            self.media.write().expect("media lock poisoned").insert(uuid, comp);
            self.push_comps_order(uuid);
            log::info!("Created default comp: {}", uuid);
            uuid
        } else {
            // Return first comp UUID from order
            order.first().copied().unwrap_or_else(|| {
                // Fallback: create new if order is broken
                let comp = Comp::new("Main", 0, 0, 24.0);
                let uuid = comp.get_uuid();
                self.media.write().expect("media lock poisoned").insert(uuid, comp);
                self.push_comps_order(uuid);
                uuid
            })
        }
    }

    /// Rebuild runtime-only state after deserialization.
    ///
    /// - Reinitializes compositor to default (CPU).
    /// - Sets event emitter and global_cache for all comps.
    pub fn rebuild_runtime(&mut self, event_emitter: Option<crate::core::event_bus::CompEventEmitter>) {
        // Reinitialize compositor (not serialized)
        *self.compositor.lock().unwrap_or_else(|e| e.into_inner()) = CompositorType::default();

        // Rebuild comps in unified media HashMap
        let mut media = self.media.write().expect("media lock poisoned");
        for comp in media.values_mut() {
            // NOTE: No need to clear cache - GlobalFrameCache is project-level
            // Cache will be naturally invalidated via dirty tracking

            // Set event emitter for comps if provided
            if let Some(ref emitter) = event_emitter {
                comp.set_event_emitter(emitter.clone());
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
        event_emitter: Option<crate::core::event_bus::CompEventEmitter>,
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

        self.rebuild_runtime(event_emitter);
    }

    /// Set compositor type (CPU or GPU).
    ///
    /// Allows switching between CPU and GPU compositing backends.
    /// GPU compositor requires OpenGL context.
    pub fn set_compositor(&self, compositor: CompositorType) {
        log::info!("Compositor changed to: {:?}", compositor);
        *self.compositor.lock().unwrap_or_else(|e| e.into_inner()) = compositor;
    }

    /// Get cloned composition by UUID.
    /// Returns owned Comp (cloned from media pool).
    pub fn get_comp(&self, uuid: Uuid) -> Option<Comp> {
        self.media.read().expect("media lock poisoned").get(&uuid).cloned()
    }

    /// Update composition in media pool.
    /// Replaces existing comp with same UUID.
    pub fn update_comp(&self, comp: Comp) {
        self.media.write().expect("media lock poisoned").insert(comp.get_uuid(), comp);
    }

    /// Check if composition exists in media pool.
    pub fn contains_comp(&self, uuid: Uuid) -> bool {
        self.media.read().expect("media lock poisoned").contains_key(&uuid)
    }

    /// Modify composition in-place via closure.
    /// Acquires write lock, calls closure with mutable reference, releases lock.
    /// Returns true if comp was found and modified.
    pub fn modify_comp<F>(&self, uuid: Uuid, f: F) -> bool
    where
        F: FnOnce(&mut Comp),
    {
        if let Some(comp) = self.media.write().expect("media lock poisoned").get_mut(&uuid) {
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

        let uuid = comp.get_uuid();
        self.media.write().expect("media lock poisoned").insert(uuid, comp);
        self.push_comps_order(uuid);
    }

    /// Create and add a new composition, returns its UUID
    pub fn create_comp(
        &mut self,
        name: &str,
        fps: f32,
        event_emitter: crate::core::event_bus::CompEventEmitter,
    ) -> Uuid {
        let end = (fps * 5.0) as i32; // 5 seconds default duration
        let mut comp = Comp::new(name, 0, end, fps);
        comp.set_event_emitter(event_emitter);
        let uuid = comp.get_uuid();
        self.add_comp(comp);
        uuid
    }

    /// Generate unique layer name based on source name
    /// Strips trailing numbers, scans ALL names in project.media, returns "base_N"
    pub fn gen_name(&self, source_name: &str) -> String {
        // Strip extension and trailing numbers: "clip_0017.exr" -> "clip"
        let base = {
            let name = source_name.rsplit_once('.').map(|(n, _)| n).unwrap_or(source_name);
            let name = name.trim_end_matches(|c: char| c.is_ascii_digit());
            let name = name.trim_end_matches('_');
            if name.is_empty() { "layer" } else { name }
        };

        // Find max existing number for this base across ALL comps and their children
        let mut max_num = 0u32;
        let media = self.media.read().expect("media lock poisoned");
        for comp in media.values() {
            // Check comp name
            if comp.name().starts_with(base) {
                let suffix = comp.name()[base.len()..].trim_start_matches('_');
                if let Ok(n) = suffix.parse::<u32>() {
                    max_num = max_num.max(n);
                }
            }
            // Check all children names
            for (_, attrs) in comp.get_children() {
                if let Some(name) = attrs.get_str("name") {
                    if name.starts_with(base) {
                        let suffix = name[base.len()..].trim_start_matches('_');
                        if let Ok(n) = suffix.parse::<u32>() {
                            max_num = max_num.max(n);
                        }
                    }
                }
            }
        }
        format!("{}_{}", base, max_num + 1)
    }

    /// Set CacheManager for project and all existing comps (call after deserialization)
    pub fn set_cache_manager(&mut self, manager: Arc<CacheManager>) {
        let media = self.media.read().expect("media lock poisoned");
        log::info!("Project::set_cache_manager() called, setting for {} comps", media.len());
        drop(media); // Release read lock before acquiring write lock

        self.cache_manager = Some(Arc::clone(&manager));
        let mut media = self.media.write().expect("media lock poisoned");
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
    /// Clears cache, cancels workers, removes references from other comps, fixes selection.
    pub fn del_comp(&mut self, uuid: Uuid) {
        // 1. Increment epoch to cancel pending worker tasks
        if let Some(ref manager) = self.cache_manager {
            manager.increment_epoch();
        }

        // 2. Clear cached frames
        if let Some(ref cache) = self.global_cache {
            cache.clear_comp(uuid);
            log::debug!("Cleared cache for removed comp: {}", uuid);
        }

        // 3. Remove references from other comps (layers using this media as source)
        {
            let mut media = self.media.write().expect("media lock poisoned");
            for (_comp_uuid, comp) in media.iter_mut() {
                let children_to_remove = comp.find_children_by_source(uuid);
                for child_uuid in children_to_remove {
                    comp.remove_child(child_uuid);
                }
            }
        }

        // 4. Remove from media pool and order
        self.media.write().expect("media lock poisoned").remove(&uuid);
        self.retain_comps_order(|u| *u != uuid);

        // 5. Fix selection
        self.retain_selection(|u| *u != uuid);
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
        map.read().expect("media lock poisoned").serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Arc<RwLock<HashMap<Uuid, Comp>>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let map = HashMap::<Uuid, Comp>::deserialize(deserializer)?;
        Ok(Arc::new(RwLock::new(map)))
    }
}
