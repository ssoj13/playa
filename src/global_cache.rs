//! Global frame cache with LRU eviction and configurable strategies
//!
//! Replaces per-Comp local caches with a unified global cache.
//! Key format: (comp_uuid, frame_idx) -> Frame
//! Integrated with CacheManager for memory tracking.

use std::sync::{Arc, Mutex};
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU64, Ordering};
use lru::LruCache;
use log::debug;
use uuid::Uuid;

use crate::cache_man::CacheManager;
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
    /// Total number of cache hits (frame found in cache)
    hits: AtomicU64,
    /// Total number of cache misses (frame not in cache)
    misses: AtomicU64,
}

impl CacheStats {
    /// Create new empty stats
    pub fn new() -> Self {
        Self {
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        }
    }

    /// Record cache hit
    pub fn record_hit(&self) {
        self.hits.fetch_add(1, Ordering::Relaxed);
    }

    /// Record cache miss
    pub fn record_miss(&self) {
        self.misses.fetch_add(1, Ordering::Relaxed);
    }

    /// Get total hits
    pub fn hits(&self) -> u64 {
        self.hits.load(Ordering::Relaxed)
    }

    /// Get total misses
    pub fn misses(&self) -> u64 {
        self.misses.load(Ordering::Relaxed)
    }

    /// Get total accesses (hits + misses)
    pub fn total(&self) -> u64 {
        self.hits() + self.misses()
    }

    /// Calculate hit rate (0.0 - 1.0)
    pub fn hit_rate(&self) -> f64 {
        let total = self.total();
        if total == 0 {
            0.0
        } else {
            self.hits() as f64 / total as f64
        }
    }

    /// Reset all statistics
    pub fn reset(&self) {
        self.hits.store(0, Ordering::Relaxed);
        self.misses.store(0, Ordering::Relaxed);
    }
}

/// Global frame cache with LRU eviction
///
/// Thread-safe cache shared across all Comps.
/// Automatically evicts oldest frames when memory limit is reached.
#[derive(Debug)]
pub struct GlobalFrameCache {
    /// LRU cache: (comp_uuid, frame_idx) -> Frame
    cache: Arc<Mutex<LruCache<(Uuid, i32), Frame>>>,
    /// Cache manager for memory tracking
    cache_manager: Arc<CacheManager>,
    /// Caching strategy (wrapped in Mutex for interior mutability)
    strategy: Arc<Mutex<CacheStrategy>>,
    /// Cache statistics
    stats: Arc<CacheStats>,
}

impl GlobalFrameCache {
    /// Create new global cache with specified capacity
    ///
    /// # Arguments
    /// * `capacity` - Maximum number of frames to cache (not memory size)
    /// * `manager` - Cache manager for memory tracking
    /// * `strategy` - Caching strategy (LastOnly or All)
    pub fn new(capacity: usize, manager: Arc<CacheManager>, strategy: CacheStrategy) -> Self {
        let capacity = NonZeroUsize::new(capacity).unwrap_or(NonZeroUsize::new(10000).unwrap());

        debug!(
            "GlobalFrameCache created: capacity={}, strategy={:?}",
            capacity, strategy
        );

        Self {
            cache: Arc::new(Mutex::new(LruCache::new(capacity))),
            cache_manager: manager,
            strategy: Arc::new(Mutex::new(strategy)),
            stats: Arc::new(CacheStats::new()),
        }
    }

    /// Get frame from cache
    ///
    /// Returns None if frame not cached.
    /// Updates LRU access order on hit.
    pub fn get(&self, comp_uuid: Uuid, frame_idx: i32) -> Option<Frame> {
        let key = (comp_uuid, frame_idx);
        let mut cache = self.cache.lock().unwrap();
        let result = cache.get(&key).cloned();

        // Record stats
        if result.is_some() {
            self.stats.record_hit();
        } else {
            self.stats.record_miss();
        }

        result
    }

    /// Check if frame exists in cache (without updating LRU)
    pub fn contains(&self, comp_uuid: Uuid, frame_idx: i32) -> bool {
        let key = (comp_uuid, frame_idx);
        let cache = self.cache.lock().unwrap();
        cache.peek(&key).is_some()
    }

    /// Insert frame into cache with LRU eviction
    ///
    /// Automatically evicts oldest frames if memory limit exceeded.
    /// Tracks memory usage via CacheManager.
    /// If replacing existing frame, properly frees old frame's memory first.
    pub fn insert(&self, comp_uuid: Uuid, frame_idx: i32, frame: Frame) {
        let key = (comp_uuid, frame_idx);
        let frame_size = frame.mem();

        // Apply strategy: LastOnly clears previous frames for this comp
        if *self.strategy.lock().unwrap() == CacheStrategy::LastOnly {
            self.clear_comp(comp_uuid);
        }

        // LRU eviction loop: free frames until under memory limit
        {
            let mut cache = self.cache.lock().unwrap();
            while self.cache_manager.check_memory_limit() {
                if let Some((_, evicted)) = cache.pop_lru() {
                    let evicted_size = evicted.mem();
                    self.cache_manager.free_memory(evicted_size);
                    debug!(
                        "LRU evicted frame: freed {} MB (usage: {} MB / {} MB)",
                        evicted_size / 1024 / 1024,
                        self.cache_manager.mem().0 / 1024 / 1024,
                        self.cache_manager.mem().1 / 1024 / 1024
                    );
                } else {
                    break; // Cache empty
                }
            }

            // Remove old frame if exists (prevents memory leak when replacing)
            if let Some(old_frame) = cache.pop(&key) {
                let old_size = old_frame.mem();
                self.cache_manager.free_memory(old_size);
                debug!(
                    "Replaced existing frame: {}:{} (freed {} bytes)",
                    comp_uuid, frame_idx, old_size
                );
            }

            // Insert new frame
            cache.push(key, frame);

            // Track new frame's memory
            self.cache_manager.add_memory(frame_size);
        }

        debug!(
            "Cached frame: {}:{} ({} bytes)",
            comp_uuid, frame_idx, frame_size
        );
    }

    /// Clear all cached frames for a specific comp
    ///
    /// Used when comp attributes change (dirty tracking).
    pub fn clear_comp(&self, comp_uuid: Uuid) {
        let mut cache = self.cache.lock().unwrap();

        // Collect keys to remove (can't modify while iterating)
        let to_remove: Vec<(Uuid, i32)> = cache
            .iter()
            .filter(|((uuid, _), _)| *uuid == comp_uuid)
            .map(|(key, _)| *key)
            .collect();

        // Remove and free memory
        for key in to_remove {
            if let Some(frame) = cache.pop(&key) {
                let size = frame.mem();
                self.cache_manager.free_memory(size);
            }
        }

        debug!("Cleared cache for comp: {}", comp_uuid);
    }

    /// Clear entire cache
    pub fn clear_all(&self) {
        let mut cache = self.cache.lock().unwrap();

        // Free all memory
        for (_, frame) in cache.iter() {
            let size = frame.mem();
            self.cache_manager.free_memory(size);
        }

        cache.clear();
        debug!("Cleared entire cache");
    }

    /// Change caching strategy
    ///
    /// If switching to LastOnly, clears all but most recent frame per comp.
    pub fn set_strategy(&self, strategy: CacheStrategy) {
        let mut current_strategy = self.strategy.lock().unwrap();
        if *current_strategy != strategy {
            debug!("Changing cache strategy: {:?} -> {:?}", *current_strategy, strategy);
            *current_strategy = strategy;

            // If switching to LastOnly, keep only most recent frame per comp
            if strategy == CacheStrategy::LastOnly {
                // TODO: implement selective eviction
                // For now, just clear all (will be refilled on next access)
                self.clear_all();
            }
        }
    }

    /// Get cache statistics
    pub fn stats(&self) -> Arc<CacheStats> {
        Arc::clone(&self.stats)
    }

    /// Get current cache size (number of entries)
    pub fn len(&self) -> usize {
        self.cache.lock().unwrap().len()
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.cache.lock().unwrap().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::frame::PixelDepth;

    #[test]
    fn test_cache_basic_operations() {
        let manager = Arc::new(CacheManager::new(0.75, 2.0));
        let cache = GlobalFrameCache::new(100, manager, CacheStrategy::All);

        // Create test frame
        let frame = Frame::new(64, 64, PixelDepth::U8);
        let comp_uuid = Uuid::new_v4();

        // Insert and retrieve
        cache.insert(comp_uuid, 0, frame.clone());
        assert!(cache.contains(comp_uuid, 0));

        let retrieved = cache.get(comp_uuid, 0);
        assert!(retrieved.is_some());
    }

    #[test]
    fn test_cache_last_only_strategy() {
        let manager = Arc::new(CacheManager::new(0.75, 2.0));
        let cache = GlobalFrameCache::new(100, manager, CacheStrategy::LastOnly);

        let frame1 = Frame::new(64, 64, PixelDepth::U8);
        let frame2 = Frame::new(64, 64, PixelDepth::U8);
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
    fn test_cache_clear_comp() {
        let manager = Arc::new(CacheManager::new(0.75, 2.0));
        let cache = GlobalFrameCache::new(100, manager, CacheStrategy::All);

        let frame = Frame::new(64, 64, PixelDepth::U8);
        let comp1 = Uuid::new_v4();
        let comp2 = Uuid::new_v4();

        // Insert frames for two comps
        cache.insert(comp1, 0, frame.clone());
        cache.insert(comp1, 1, frame.clone());
        cache.insert(comp2, 0, frame.clone());

        // Clear comp1
        cache.clear_comp(comp1);

        assert!(!cache.contains(comp1, 0));
        assert!(!cache.contains(comp1, 1));
        assert!(cache.contains(comp2, 0)); // comp2 unaffected
    }

    #[test]
    fn test_cache_statistics() {
        let manager = Arc::new(CacheManager::new(0.75, 2.0));
        let cache = GlobalFrameCache::new(100, manager, CacheStrategy::All);

        let frame = Frame::new(64, 64, PixelDepth::U8);
        let comp_uuid = Uuid::new_v4();

        // Initially zero stats
        let stats = cache.stats();
        assert_eq!(stats.hits(), 0);
        assert_eq!(stats.misses(), 0);
        assert_eq!(stats.total(), 0);
        assert_eq!(stats.hit_rate(), 0.0);

        // Insert frame
        cache.insert(comp_uuid, 0, frame.clone());

        // First get: cache hit
        let _retrieved = cache.get(comp_uuid, 0);
        assert_eq!(stats.hits(), 1);
        assert_eq!(stats.misses(), 0);
        assert_eq!(stats.total(), 1);
        assert_eq!(stats.hit_rate(), 1.0);

        // Get non-existent frame: cache miss
        let _missing = cache.get(comp_uuid, 999);
        assert_eq!(stats.hits(), 1);
        assert_eq!(stats.misses(), 1);
        assert_eq!(stats.total(), 2);
        assert_eq!(stats.hit_rate(), 0.5);

        // Another hit
        let _retrieved2 = cache.get(comp_uuid, 0);
        assert_eq!(stats.hits(), 2);
        assert_eq!(stats.misses(), 1);
        assert_eq!(stats.total(), 3);
        assert!((stats.hit_rate() - 0.666).abs() < 0.01); // ~66.67%

        // Reset stats
        stats.reset();
        assert_eq!(stats.hits(), 0);
        assert_eq!(stats.misses(), 0);
        assert_eq!(stats.total(), 0);
        assert_eq!(stats.hit_rate(), 0.0);
    }
}
