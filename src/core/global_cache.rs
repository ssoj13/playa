//! Global frame cache with nested HashMap structure
//!
//! Structure: HashMap<Uuid, HashMap<i32, Frame>>
//! - Outer map: comp_uuid -> frames
//! - Inner map: frame_idx -> Frame
//!
//! Benefits:
//! - O(1) clear_comp() - just remove outer key
//! - O(1) lookup by (comp_uuid, frame_idx)
//! - Memory tracking via CacheManager

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};
use std::hash::{Hash, Hasher};
use indexmap::IndexSet;
use log::debug;
use uuid::Uuid;

use crate::core::cache_man::CacheManager;
use crate::entities::Frame;

/// Cache strategy for frame retention
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CacheStrategy {
    /// Cache only the last accessed frame per comp (minimal memory)
    LastOnly,
    /// Cache all frames within work area (maximum performance)
    All,
}

/// Cache statistics for monitoring performance
#[derive(Debug, Default)]
pub struct CacheStats {
    hits: AtomicU64,
    misses: AtomicU64,
}

impl CacheStats {
    pub fn new() -> Self {
        Self {
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        }
    }

    pub fn record_hit(&self) {
        self.hits.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_miss(&self) {
        self.misses.fetch_add(1, Ordering::Relaxed);
    }

    pub fn hits(&self) -> u64 {
        self.hits.load(Ordering::Relaxed)
    }

    pub fn misses(&self) -> u64 {
        self.misses.load(Ordering::Relaxed)
    }

    pub fn total(&self) -> u64 {
        self.hits() + self.misses()
    }

    pub fn hit_rate(&self) -> f64 {
        let total = self.total();
        if total == 0 { 0.0 } else { self.hits() as f64 / total as f64 }
    }

    pub fn reset(&self) {
        self.hits.store(0, Ordering::Relaxed);
        self.misses.store(0, Ordering::Relaxed);
    }
}

/// Entry in LRU eviction queue
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct CacheKey {
    comp_uuid: Uuid,
    frame_idx: i32,
}

impl Hash for CacheKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.comp_uuid.hash(state);
        self.frame_idx.hash(state);
    }
}

/// Global frame cache with nested HashMap + LRU eviction
///
/// Structure: HashMap<Uuid, HashMap<i32, Frame>>
/// - O(1) clear_comp() by removing outer key
/// - O(1) lookup via nested HashMap
/// - O(1) LRU eviction via IndexSet (insertion-order + O(1) remove by key)
#[derive(Debug)]
pub struct GlobalFrameCache {
    /// Nested cache: comp_uuid -> (frame_idx -> Frame)
    cache: Arc<Mutex<HashMap<Uuid, HashMap<i32, Frame>>>>,
    /// LRU eviction queue: IndexSet preserves insertion order, O(1) remove by key
    lru_order: Arc<Mutex<IndexSet<CacheKey>>>,
    /// Cache manager for memory tracking
    cache_manager: Arc<CacheManager>,
    /// Caching strategy
    strategy: Arc<Mutex<CacheStrategy>>,
    /// Cache statistics
    stats: Arc<CacheStats>,
    /// Maximum entries (for eviction trigger)
    capacity: usize,
}

impl GlobalFrameCache {
    /// Create new global cache
    ///
    /// # Arguments
    /// * `capacity` - Maximum number of frames before eviction
    /// * `manager` - Cache manager for memory tracking
    /// * `strategy` - Caching strategy (LastOnly or All)
    pub fn new(capacity: usize, manager: Arc<CacheManager>, strategy: CacheStrategy) -> Self {
        let capacity = capacity.max(100); // Min 100 frames

        debug!(
            "GlobalFrameCache created: capacity={}, strategy={:?} (nested HashMap)",
            capacity, strategy
        );

        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
            lru_order: Arc::new(Mutex::new(IndexSet::with_capacity(capacity))),
            cache_manager: manager,
            strategy: Arc::new(Mutex::new(strategy)),
            stats: Arc::new(CacheStats::new()),
            capacity,
        }
    }

    /// Get frame from cache
    ///
    /// Returns None if frame not cached.
    /// Updates LRU order on hit (moves to back of queue).
    pub fn get(&self, comp_uuid: Uuid, frame_idx: i32) -> Option<Frame> {
        // Minimize lock hold time - release cache lock before LRU update
        let result = {
            let cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            cache
                .get(&comp_uuid)
                .and_then(|frames| frames.get(&frame_idx))
                .cloned()
        }; // cache lock released here

        if result.is_some() {
            self.stats.record_hit();
            // Update LRU order: move to back (most recently used) - O(1) with IndexSet
            let key = CacheKey { comp_uuid, frame_idx };
            let mut lru = self.lru_order.lock().unwrap_or_else(|e| e.into_inner());
            // shift_remove is O(n) but we need it for LRU ordering; swap_remove would be O(1) but breaks order
            // Alternative: use move_index but IndexSet doesn't have it - just re-insert
            lru.shift_remove(&key);
            lru.insert(key);
        } else {
            self.stats.record_miss();
        }

        result
    }

    /// Check if frame exists in cache (without updating LRU)
    pub fn contains(&self, comp_uuid: Uuid, frame_idx: i32) -> bool {
        let cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        cache
            .get(&comp_uuid)
            .map(|frames| frames.contains_key(&frame_idx))
            .unwrap_or(false)
    }

    /// Get frame status without cloning the frame (lightweight query for UI)
    pub fn get_status(&self, comp_uuid: Uuid, frame_idx: i32) -> Option<crate::entities::FrameStatus> {
        let cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        cache
            .get(&comp_uuid)
            .and_then(|frames| frames.get(&frame_idx))
            .map(|frame| frame.status())
    }

    /// Atomically get existing frame or insert new one.
    /// Returns (frame, was_inserted) - true if we inserted, false if already existed.
    /// This prevents race conditions where two threads both check and insert.
    pub fn get_or_insert(&self, comp_uuid: Uuid, frame_idx: i32, make_frame: impl FnOnce() -> Frame) -> (Frame, bool) {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());

        // Check if frame already exists
        if let Some(existing) = cache.get(&comp_uuid).and_then(|f| f.get(&frame_idx)) {
            return (existing.clone(), false);
        }

        // Frame doesn't exist - create and insert
        let frame = make_frame();
        let frame_clone = frame.clone();
        let frame_size = frame.mem();

        // Insert into cache and update LRU atomically (hold both locks)
        {
            let mut lru = self.lru_order.lock().unwrap_or_else(|e| e.into_inner());
            cache.entry(comp_uuid).or_default().insert(frame_idx, frame);
            lru.insert(CacheKey { comp_uuid, frame_idx });
            // Track memory while holding locks to prevent race
            self.cache_manager.add_memory(frame_size);
        }

        (frame_clone, true)
    }

    /// Insert frame into cache
    ///
    /// Automatically evicts oldest frames if memory limit exceeded.
    /// Tracks memory usage via CacheManager.
    ///
    /// Accepts frames in ANY status (Header, Loading, Loaded, Error).
    /// Header/Loading frames serve as placeholders that get loaded in-place.
    /// Re-insert after loading to update memory tracking.
    pub fn insert(&self, comp_uuid: Uuid, frame_idx: i32, frame: Frame) {
        let frame_size = frame.mem();

        // Apply strategy: LastOnly clears previous frames for this comp
        if *self.strategy.lock().unwrap_or_else(|e| e.into_inner()) == CacheStrategy::LastOnly {
            self.clear_comp(comp_uuid);
        }

        // Eviction: both memory limit and capacity limit
        // First evict if over memory limit
        while self.cache_manager.check_memory_limit() {
            if !self.evict_oldest() {
                break; // Nothing to evict
            }
        }
        // Then evict if over capacity limit (allow up to capacity entries)
        while self.len() > self.capacity {
            if !self.evict_oldest() {
                break;
            }
        }

        // Insert frame
        {
            let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            let mut lru = self.lru_order.lock().unwrap_or_else(|e| e.into_inner());

            // Remove old frame if exists (prevents memory leak)
            if let Some(old_frame) = cache.get_mut(&comp_uuid).and_then(|f| f.remove(&frame_idx)) {
                let old_size = old_frame.mem();
                self.cache_manager.free_memory(old_size);
                // Remove from LRU queue - O(1) hash lookup + O(n) shift
                let key = CacheKey { comp_uuid, frame_idx };
                lru.shift_remove(&key);
                debug!("Replaced frame: {}:{} (freed {} bytes)", comp_uuid, frame_idx, old_size);
            }

            // Insert new frame
            cache.entry(comp_uuid).or_default().insert(frame_idx, frame);

            // Add to LRU queue (back = most recent)
            lru.insert(CacheKey { comp_uuid, frame_idx });

            // Track memory
            self.cache_manager.add_memory(frame_size);
        }

        debug!("Cached frame: {}:{} ({} bytes)", comp_uuid, frame_idx, frame_size);
    }

    /// Evict oldest frame from cache
    ///
    /// Returns true if a frame was evicted, false if cache empty.
    fn evict_oldest(&self) -> bool {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        let mut lru = self.lru_order.lock().unwrap_or_else(|e| e.into_inner());

        // Get oldest key from front of queue (first inserted = oldest)
        let key = match lru.shift_remove_index(0) {
            Some(k) => k,
            None => return false,
        };

        // Remove from nested HashMap (need frames ref for is_empty check after remove)
        #[allow(clippy::collapsible_if)]
        if let Some(frames) = cache.get_mut(&key.comp_uuid) {
            if let Some(evicted) = frames.remove(&key.frame_idx) {
                let evicted_size = evicted.mem();
                self.cache_manager.free_memory(evicted_size);

                // Remove empty inner HashMap
                if frames.is_empty() {
                    cache.remove(&key.comp_uuid);
                }

                debug!(
                    "LRU evicted: {}:{} (freed {} MB)",
                    key.comp_uuid,
                    key.frame_idx,
                    evicted_size / 1024 / 1024
                );
                return true;
            }
        }

        false
    }

    /// Clear a single cached frame for a specific comp
    ///
    /// Use this for light attribute changes (opacity, blend_mode) that only
    /// require recomposing the current frame, not the entire comp.
    pub fn clear_frame(&self, comp_uuid: Uuid, frame_idx: i32) {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        let mut lru = self.lru_order.lock().unwrap_or_else(|e| e.into_inner());

        if let Some(frame) = cache.get_mut(&comp_uuid).and_then(|f| f.remove(&frame_idx)) {
            let size = frame.mem();
            self.cache_manager.free_memory(size);

            // Remove from LRU queue - O(1) lookup via hash
            let key = CacheKey { comp_uuid, frame_idx };
            lru.shift_remove(&key);

            log::debug!(
                "Cleared single frame {}:{} ({} bytes freed)",
                comp_uuid, frame_idx, size
            );
        }
    }

    /// Clear all cached frames for a specific comp - O(1)
    ///
    /// This is the main benefit of nested HashMap structure.
    pub fn clear_comp(&self, comp_uuid: Uuid) {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        let mut lru = self.lru_order.lock().unwrap_or_else(|e| e.into_inner());

        // Remove entire inner HashMap in O(1)
        if let Some(frames) = cache.remove(&comp_uuid) {
            // Free memory for all frames
            let mut total_freed = 0usize;
            for (_, frame) in frames.iter() {
                let size = frame.mem();
                self.cache_manager.free_memory(size);
                total_freed += size;
            }

            // Remove from LRU queue
            lru.retain(|k| k.comp_uuid != comp_uuid);

            debug!(
                "Cleared comp {}: {} frames, {} MB freed",
                comp_uuid,
                frames.len(),
                total_freed / 1024 / 1024
            );
        }
    }

    /// Clear frames in a specific range for a comp
    ///
    /// More efficient than clear_comp when only part of timeline changed.
    /// Frames will be recreated as Header on next access.
    pub fn clear_range(&self, comp_uuid: Uuid, start: i32, end: i32) {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        let mut lru = self.lru_order.lock().unwrap_or_else(|e| e.into_inner());

        let Some(frames) = cache.get_mut(&comp_uuid) else {
            return;
        };

        let mut total_freed = 0usize;
        let mut count = 0;

        // Collect keys to remove (can't modify while iterating)
        let keys_to_remove: Vec<i32> = frames
            .keys()
            .filter(|&&idx| idx >= start && idx <= end)
            .copied()
            .collect();

        for idx in keys_to_remove {
            if let Some(frame) = frames.remove(&idx) {
                let size = frame.mem();
                self.cache_manager.free_memory(size);
                total_freed += size;
                count += 1;
            }
        }

        // Remove from LRU queue
        lru.retain(|k| !(k.comp_uuid == comp_uuid && k.frame_idx >= start && k.frame_idx <= end));

        // Remove empty inner HashMap
        if frames.is_empty() {
            cache.remove(&comp_uuid);
        }

        if count > 0 {
            debug!(
                "Cleared range {}:[{}..{}]: {} frames, {} MB freed",
                comp_uuid, start, end, count, total_freed / 1024 / 1024
            );
        }
    }

    /// Clear entire cache
    pub fn clear_all(&self) {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        let mut lru = self.lru_order.lock().unwrap_or_else(|e| e.into_inner());

        // Free all memory
        for (_, frames) in cache.iter() {
            for (_, frame) in frames.iter() {
                self.cache_manager.free_memory(frame.mem());
            }
        }

        cache.clear();
        lru.clear();

        debug!("Cleared entire cache");
    }

    /// Change caching strategy
    pub fn set_strategy(&self, strategy: CacheStrategy) {
        let mut current = self.strategy.lock().unwrap_or_else(|e| e.into_inner());
        if *current != strategy {
            debug!("Cache strategy: {:?} -> {:?}", *current, strategy);
            *current = strategy;

            // If switching to LastOnly, clear all
            if strategy == CacheStrategy::LastOnly {
                drop(current); // Release lock before clear_all
                self.clear_all();
            }
        }
    }

    /// Get cache statistics
    pub fn stats(&self) -> Arc<CacheStats> {
        Arc::clone(&self.stats)
    }

    /// Get current cache size (total number of frames)
    pub fn len(&self) -> usize {
        let cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        cache.values().map(|frames| frames.len()).sum()
    }

    /// Get number of cached comps
    pub fn comp_count(&self) -> usize {
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// Check if cache has any frames for a comp
    pub fn has_comp(&self, comp_uuid: Uuid) -> bool {
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).contains_key(&comp_uuid)
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).is_empty()
    }

    /// Get frame count for specific comp
    pub fn comp_frame_count(&self, comp_uuid: Uuid) -> usize {
        self.cache
            .lock()
            .unwrap()
            .get(&comp_uuid)
            .map(|frames| frames.len())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a test frame with Loaded status (required for cache insert)
    fn make_loaded_frame(width: usize, height: usize) -> Frame {
        // Use from_u8_buffer which creates Loaded frames
        let buf = vec![0u8; width * height * 4];
        Frame::from_u8_buffer(buf, width, height)
    }

    #[test]
    fn test_cache_basic_operations() {
        let manager = Arc::new(CacheManager::new(0.75, 2.0));
        let cache = GlobalFrameCache::new(100, manager, CacheStrategy::All);

        let frame = make_loaded_frame(64, 64);
        let comp_uuid = Uuid::new_v4();

        // Insert and retrieve
        cache.insert(comp_uuid, 0, frame.clone());
        assert!(cache.contains(comp_uuid, 0));

        let retrieved = cache.get(comp_uuid, 0);
        assert!(retrieved.is_some());

        // Check counts
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.comp_count(), 1);
        assert_eq!(cache.comp_frame_count(comp_uuid), 1);
    }

    #[test]
    fn test_cache_last_only_strategy() {
        let manager = Arc::new(CacheManager::new(0.75, 2.0));
        let cache = GlobalFrameCache::new(100, manager, CacheStrategy::LastOnly);

        let frame1 = make_loaded_frame(64, 64);
        let frame2 = make_loaded_frame(64, 64);
        let comp_uuid = Uuid::new_v4();

        // Insert frame 0
        cache.insert(comp_uuid, 0, frame1);
        assert!(cache.contains(comp_uuid, 0));

        // Insert frame 1 (should clear frame 0 in LastOnly mode)
        cache.insert(comp_uuid, 1, frame2);
        assert!(cache.contains(comp_uuid, 1));
        assert!(!cache.contains(comp_uuid, 0)); // Frame 0 evicted
    }

    #[test]
    fn test_cache_clear_comp_o1() {
        let manager = Arc::new(CacheManager::new(0.75, 2.0));
        let cache = GlobalFrameCache::new(100, manager, CacheStrategy::All);

        let frame = make_loaded_frame(64, 64);
        let comp1 = Uuid::new_v4();
        let comp2 = Uuid::new_v4();

        // Insert frames for two comps
        for i in 0..100 {
            cache.insert(comp1, i, frame.clone());
        }
        cache.insert(comp2, 0, frame.clone());

        assert_eq!(cache.comp_frame_count(comp1), 100);
        assert_eq!(cache.comp_frame_count(comp2), 1);

        // Clear comp1 - should be O(1) on HashMap level
        cache.clear_comp(comp1);

        assert_eq!(cache.comp_frame_count(comp1), 0);
        assert!(!cache.contains(comp1, 0));
        assert!(!cache.contains(comp1, 50));
        assert!(cache.contains(comp2, 0)); // comp2 unaffected
    }

    #[test]
    fn test_cache_statistics() {
        let manager = Arc::new(CacheManager::new(0.75, 2.0));
        let cache = GlobalFrameCache::new(100, manager, CacheStrategy::All);

        let frame = make_loaded_frame(64, 64);
        let comp_uuid = Uuid::new_v4();

        let stats = cache.stats();
        assert_eq!(stats.hits(), 0);
        assert_eq!(stats.misses(), 0);

        cache.insert(comp_uuid, 0, frame.clone());

        // Cache hit
        let _ = cache.get(comp_uuid, 0);
        assert_eq!(stats.hits(), 1);
        assert_eq!(stats.misses(), 0);

        // Cache miss
        let _ = cache.get(comp_uuid, 999);
        assert_eq!(stats.hits(), 1);
        assert_eq!(stats.misses(), 1);
        assert_eq!(stats.hit_rate(), 0.5);
    }

    #[test]
    fn test_multiple_comps() {
        let manager = Arc::new(CacheManager::new(0.75, 2.0));
        let cache = GlobalFrameCache::new(100, manager, CacheStrategy::All);

        let frame = make_loaded_frame(64, 64);

        // Insert frames for 5 comps
        let mut comps = Vec::new();
        for _ in 0..5 {
            let comp = Uuid::new_v4();
            comps.push(comp);
            for i in 0..10 {
                cache.insert(comp, i, frame.clone());
            }
        }

        assert_eq!(cache.comp_count(), 5);
        assert_eq!(cache.len(), 50);

        // Clear middle comp
        cache.clear_comp(comps[2]);
        assert_eq!(cache.comp_count(), 4);
        assert_eq!(cache.len(), 40);
    }
}
