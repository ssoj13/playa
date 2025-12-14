//! Project: top-level scene container (playlist).
//!
//! Holds clips (MediaPool) and compositions (Comps) that reference clips.
//! Project is the unit of serialization: scenes are saved and loaded via
//! `Project::to_json` / `Project::from_json`.
//!
//! # Auto-Emit & Cache Invalidation
//!
//! Project has an `event_emitter` field (runtime-only, `#[serde(skip)]`) that
//! enables automatic cache invalidation when comp attributes change.
//!
//! ## `modify_comp()` Pattern
//!
//! All comp modifications should go through `modify_comp()` which:
//! 1. Executes the closure (may call `attrs.set()` → dirty=true)
//! 2. If comp or any layer is dirty → emits `AttrsChangedEvent`
//!
//! ```text
//! project.modify_comp(uuid, |comp| {
//!     comp.set_child_attrs(...);  // attrs.set() → dirty=true
//! });
//! // Auto-emits AttrsChangedEvent if comp/layers dirty
//! // → triggers cache.clear_comp() and viewport refresh
//! ```
//!
//! ## Important: Event Emitter Restoration
//!
//! Since `event_emitter` has `#[serde(skip)]`, it's lost during deserialization.
//! Must call `project.set_event_emitter()` after:
//! - `Project::from_json()` (load project)
//! - eframe's persisted state deserialization
//! - Any clone/rebuild operation

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{Attrs, CompositorType};
use super::node::{Node, ComputeContext};
use super::node_kind::NodeKind;
use super::comp_node::CompNode;
use super::file_node::FileNode;
use super::frame::Frame;
use super::keys::*;
use crate::core::cache_man::CacheManager;
use crate::core::event_bus::EventEmitter;
use crate::core::global_cache::{CacheStrategy, GlobalFrameCache};
use super::comp_events::AttrsChangedEvent;

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

    /// Unified media pool: all nodes (FileNode, CompNode) keyed by UUID
    /// Thread-safe for concurrent reads from background composition workers
    #[serde(with = "arc_rwlock_hashmap")]
    pub media: Arc<RwLock<HashMap<Uuid, NodeKind>>>,

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

    /// Event emitter for auto-emitting AttrsChangedEvent (runtime-only)
    #[serde(skip)]
    event_emitter: Option<EventEmitter>,
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
            event_emitter: self.event_emitter.clone(),
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
            event_emitter: None,
        }
    }

    /// Set event emitter for auto-emitting AttrsChangedEvent on comp modifications.
    /// Called once during App initialization to enable automatic cache invalidation.
    pub fn set_event_emitter(&mut self, emitter: EventEmitter) {
        self.event_emitter = Some(emitter);
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
        let order = self.comps_order();
        let has_comps = !order.is_empty();

        if !has_comps {
            let comp = CompNode::new("Main", 0, 0, 24.0);
            let uuid = comp.uuid();
            self.media.write().expect("media lock poisoned").insert(uuid, NodeKind::Comp(comp));
            self.push_comps_order(uuid);
            log::info!("Created default comp: {}", uuid);
            uuid
        } else {
            order.first().copied().unwrap_or_else(|| {
                let comp = CompNode::new("Main", 0, 0, 24.0);
                let uuid = comp.uuid();
                self.media.write().expect("media lock poisoned").insert(uuid, NodeKind::Comp(comp));
                self.push_comps_order(uuid);
                uuid
            })
        }
    }

    /// Rebuild runtime-only state after deserialization.
    /// Reinitializes compositor to default (CPU).
    pub fn rebuild_runtime(&mut self, _event_emitter: Option<crate::core::event_bus::CompEventEmitter>) {
        // Reinitialize compositor (not serialized)
        *self.compositor.lock().unwrap_or_else(|e| e.into_inner()) = CompositorType::default();
        // NodeKind doesn't need event emitters or cache refs - they're passed via ComputeContext
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

    // === Node access methods ===

    /// Access node by reference via closure (no clone)
    pub fn with_node<F, R>(&self, uuid: Uuid, f: F) -> Option<R>
    where
        F: FnOnce(&NodeKind) -> R,
    {
        let media = self.media.read().expect("media lock poisoned");
        media.get(&uuid).map(f)
    }

    /// Access CompNode by reference via closure (no clone)
    pub fn with_comp<F, R>(&self, uuid: Uuid, f: F) -> Option<R>
    where
        F: FnOnce(&CompNode) -> R,
    {
        let media = self.media.read().expect("media lock poisoned");
        media.get(&uuid).and_then(|n| n.as_comp()).map(f)
    }

    /// Access FileNode by reference via closure (no clone)
    pub fn with_file<F, R>(&self, uuid: Uuid, f: F) -> Option<R>
    where
        F: FnOnce(&FileNode) -> R,
    {
        let media = self.media.read().expect("media lock poisoned");
        media.get(&uuid).and_then(|n| n.as_file()).map(f)
    }

    /// Compute frame for comp (single lock, no double-lock issue)
    /// Use this instead of with_comp + comp.get_frame which would deadlock.
    pub fn compute_frame(&self, comp_uuid: Uuid, frame_idx: i32) -> Option<Frame> {
        let cache = self.global_cache.as_ref()?;
        let media = self.media.read().expect("media lock poisoned");
        let comp = media.get(&comp_uuid)?.as_comp()?;
        let ctx = ComputeContext {
            cache,
            media: &media,
            workers: None,
            epoch: 0,
        };
        comp.compute(frame_idx, &ctx)
    }

    /// Update node in media pool
    pub fn update_node(&self, node: NodeKind) {
        let uuid = node.uuid();
        self.media.write().expect("media lock poisoned").insert(uuid, node);
    }

    /// Check if node exists in media pool
    pub fn contains_node(&self, uuid: Uuid) -> bool {
        self.media.read().expect("media lock poisoned").contains_key(&uuid)
    }

    /// Alias for contains_node (compat)
    pub fn contains_comp(&self, uuid: Uuid) -> bool {
        self.contains_node(uuid)
    }

    /// Modify node in-place via closure
    pub fn modify_node<F>(&self, uuid: Uuid, f: F) -> bool
    where
        F: FnOnce(&mut NodeKind),
    {
        if let Some(node) = self.media.write().expect("media lock poisoned").get_mut(&uuid) {
            f(node);
            true
        } else {
            false
        }
    }

    /// Modify CompNode in-place via closure.
    ///
    /// Auto-emits `AttrsChangedEvent` if comp or any layer is dirty after modification,
    /// triggering cache invalidation and viewport refresh.
    pub fn modify_comp<F>(&self, uuid: Uuid, f: F) -> bool
    where
        F: FnOnce(&mut CompNode),
    {
        if let Some(node) = self.media.write().expect("media lock poisoned").get_mut(&uuid)
            && let Some(comp) = node.as_comp_mut() {
                f(comp);
                // Emit event if comp or any layer is dirty after modification.
                // This ensures ALL changes that affect render trigger cache invalidation,
                // even when multiple modify_comp calls happen before next render.
                let dirty = comp.is_dirty();
                if dirty && let Some(ref emitter) = self.event_emitter {
                    emitter.emit(AttrsChangedEvent(uuid));
                } else if dirty {
                    log::warn!("modify_comp: dirty but no emitter! uuid={}", uuid);
                }
                return true;
            }
        false
    }

    /// Add node to project
    pub fn add_node(&mut self, node: NodeKind) {
        let uuid = node.uuid();
        self.media.write().expect("media lock poisoned").insert(uuid, node);
        self.push_comps_order(uuid);
    }

    /// Create and add new CompNode, returns its UUID
    pub fn create_comp(
        &mut self,
        name: &str,
        fps: f32,
        _event_emitter: crate::core::event_bus::CompEventEmitter,
    ) -> Uuid {
        let end = (fps * 5.0) as i32;
        let comp = CompNode::new(name, 0, end, fps);
        let uuid = comp.uuid();
        self.add_node(NodeKind::Comp(comp));
        uuid
    }

    /// Create and add new FileNode, returns its UUID
    pub fn create_file(&mut self, file_mask: String, start: i32, end: i32, fps: f32) -> Uuid {
        let file = FileNode::new(file_mask, start, end, fps);
        let uuid = file.uuid();
        self.add_node(NodeKind::File(file));
        uuid
    }

    /// Generate unique layer name based on source name
    pub fn gen_name(&self, source_name: &str) -> String {
        let base = {
            let name = source_name.rsplit_once('.').map(|(n, _)| n).unwrap_or(source_name);
            let name = name.trim_end_matches(|c: char| c.is_ascii_digit());
            let name = name.trim_end_matches('_');
            if name.is_empty() { "layer" } else { name }
        };

        let mut max_num = 0u32;
        let media = self.media.read().expect("media lock poisoned");
        for node in media.values() {
            // Check node name
            if node.name().starts_with(base) {
                let suffix = node.name()[base.len()..].trim_start_matches('_');
                if let Ok(n) = suffix.parse::<u32>() {
                    max_num = max_num.max(n);
                }
            }
            // Check layer names for CompNode
            if let Some(comp) = node.as_comp() {
                for layer in &comp.layers {
                    if let Some(name) = layer.attrs.get_str(A_NAME)
                        && name.starts_with(base) {
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

    /// Set CacheManager for project
    pub fn set_cache_manager(&mut self, manager: Arc<CacheManager>) {
        let media = self.media.read().expect("media lock poisoned");
        log::info!("Project::set_cache_manager() called, {} nodes", media.len());
        drop(media);
        self.cache_manager = Some(manager);
    }

    /// Get reference to cache manager
    pub fn cache_manager(&self) -> Option<&Arc<CacheManager>> {
        if self.cache_manager.is_none() {
            log::warn!("Project::cache_manager() returning None!");
        }
        self.cache_manager.as_ref()
    }

    /// Remove node by UUID. Clears cache, removes layer references.
    pub fn del_node(&mut self, uuid: Uuid) {
        // 1. Cancel pending workers
        if let Some(ref manager) = self.cache_manager {
            manager.increment_epoch();
        }

        // 2. Clear cached frames
        if let Some(ref cache) = self.global_cache {
            cache.clear_comp(uuid);
            log::trace!("Cleared cache for removed node: {}", uuid);
        }

        // 3. Remove layer references from CompNodes
        {
            let mut media = self.media.write().expect("media lock poisoned");
            for node in media.values_mut() {
                if let Some(comp) = node.as_comp_mut() {
                    comp.layers.retain(|layer| layer.source_uuid() != uuid);
                }
            }
        }

        // 4. Remove from media pool and order
        self.media.write().expect("media lock poisoned").remove(&uuid);
        self.retain_comps_order(|u| *u != uuid);

        // 5. Fix selection
        self.retain_selection(|u| *u != uuid);
    }

    /// Alias for del_node (compat)
    pub fn del_comp(&mut self, uuid: Uuid) {
        self.del_node(uuid);
    }

    // === Node iteration ===

    /// Iterate node tree depth-first starting from root.
    /// 
    /// # Arguments
    /// * `root` - Starting node UUID
    /// * `depth` - Max depth to traverse (-1 = unlimited, 0 = root only, 1 = direct children, etc.)
    /// 
    /// # Returns
    /// Iterator yielding NodeIterItem with uuid, depth, and is_leaf flag
    pub fn iter_node(&self, root: Uuid, depth: i32) -> NodeIter<'_> {
        NodeIter::new(self, root, depth)
    }

    /// Get all descendant UUIDs of a node (including the node itself)
    pub fn descendants(&self, root: Uuid) -> Vec<Uuid> {
        self.iter_node(root, -1).map(|item| item.uuid).collect()
    }

    /// Check if ancestor contains descendant (directly or indirectly)
    pub fn is_ancestor(&self, ancestor: Uuid, descendant: Uuid) -> bool {
        if ancestor == descendant {
            return true;
        }
        self.iter_node(ancestor, -1)
            .skip(1) // Skip root itself
            .any(|item| item.uuid == descendant)
    }
}

/// Item yielded by NodeIter
#[derive(Debug, Clone)]
pub struct NodeIterItem {
    /// Node UUID
    pub uuid: Uuid,
    /// Depth from root (0 = root)
    pub depth: i32,
    /// True if node has no children
    pub is_leaf: bool,
}

/// Depth-first iterator over node tree
pub struct NodeIter<'a> {
    project: &'a Project,
    stack: Vec<(Uuid, i32)>, // (uuid, current_depth)
    max_depth: i32,
}

impl<'a> NodeIter<'a> {
    fn new(project: &'a Project, root: Uuid, max_depth: i32) -> Self {
        Self {
            project,
            stack: vec![(root, 0)],
            max_depth,
        }
    }
}

impl<'a> Iterator for NodeIter<'a> {
    type Item = NodeIterItem;

    fn next(&mut self) -> Option<Self::Item> {
        let (uuid, depth) = self.stack.pop()?;

        // Get children (layer source UUIDs) from media pool
        let children: Vec<Uuid> = {
            let media = self.project.media.read().expect("media lock poisoned");
            media.get(&uuid)
                .and_then(|node| node.as_comp())
                .map(|comp| comp.layers.iter().map(|l| l.source_uuid()).collect())
                .unwrap_or_default()
        };

        let is_leaf = children.is_empty();

        // Push children to stack if within depth limit
        // depth=-1 means unlimited
        if self.max_depth < 0 || depth < self.max_depth {
            // Push in reverse order so first child is processed first
            for child_uuid in children.into_iter().rev() {
                self.stack.push((child_uuid, depth + 1));
            }
        }

        Some(NodeIterItem {
            uuid,
            depth,
            is_leaf,
        })
    }
}

// Serde helper for Arc<RwLock<HashMap<Uuid, NodeKind>>>
mod arc_rwlock_hashmap {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(
        map: &Arc<RwLock<HashMap<Uuid, NodeKind>>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        map.read().expect("media lock poisoned").serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Arc<RwLock<HashMap<Uuid, NodeKind>>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let map = HashMap::<Uuid, NodeKind>::deserialize(deserializer)?;
        Ok(Arc::new(RwLock::new(map)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::cache_man::CacheManager;

    fn test_project() -> Project {
        let cache_manager = Arc::new(CacheManager::new(0.75, 2.0));
        Project::new(cache_manager)
    }

    #[test]
    fn test_iter_node_empty() {
        let project = test_project();
        let fake_uuid = Uuid::new_v4();
        
        // Iterating non-existent node returns just the root (with is_leaf=true)
        let items: Vec<_> = project.iter_node(fake_uuid, -1).collect();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].uuid, fake_uuid);
        assert_eq!(items[0].depth, 0);
        assert!(items[0].is_leaf);
    }

    #[test]
    fn test_iter_node_depth_limit() {
        let project = test_project();
        let root = Uuid::new_v4();
        
        // depth=0 returns only root
        let items: Vec<_> = project.iter_node(root, 0).collect();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].depth, 0);
    }

    #[test]
    fn test_descendants() {
        let project = test_project();
        let root = Uuid::new_v4();
        
        let desc = project.descendants(root);
        assert_eq!(desc.len(), 1);
        assert_eq!(desc[0], root);
    }

    #[test]
    fn test_is_ancestor_self() {
        let project = test_project();
        let uuid = Uuid::new_v4();
        
        // Node is ancestor of itself
        assert!(project.is_ancestor(uuid, uuid));
    }

    #[test]
    fn test_is_ancestor_different() {
        let project = test_project();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        
        // Unrelated nodes are not ancestors
        assert!(!project.is_ancestor(a, b));
    }
}
