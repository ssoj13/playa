//! Global cache memory manager with LRU eviction and epoch-based preload cancellation
//!
//! **Why**: Per-comp caches need coordinated memory tracking to prevent OOM.
//! Epoch mechanism cancels stale preload requests during fast timeline scrubbing.
//!
//! **Used by**: App (global singleton), Comp (per-comp cache tracking)

use log::{debug, info};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use sysinfo::System;

/// Preload strategy for frame loading
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreloadStrategy {
    /// Spiral pattern: 0, +1, -1, +2, -2, ... (good for image sequences with cheap seeking)
    Spiral,
    /// Forward-only: center → end (optimized for video where backward seeking is expensive)
    Forward,
}

/// Global cache memory manager
///
/// Tracks memory usage across all Comp caches and provides epoch mechanism
/// for cancelling stale preload requests.
#[derive(Debug)]
pub struct CacheManager {
    /// Atomically tracked memory usage (bytes)
    memory_usage: Arc<AtomicUsize>,
    /// Maximum allowed memory (bytes) - atomic for lock-free updates
    max_memory_bytes: AtomicUsize,
    /// Epoch counter for cancelling stale requests
    current_epoch: Arc<AtomicU64>,
}

impl CacheManager {
    /// Create cache manager with memory limit
    ///
    /// # Arguments
    ///
    /// * `mem_fraction` - Fraction of available memory (0.0-1.0, e.g. 0.75 = 75%)
    /// * `reserve_gb` - Reserve memory for system (GB, e.g. 2.0 = 2GB)
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use playa::CacheManager;
    /// let manager = CacheManager::new(0.75, 2.0); // 75% of available, reserve 2GB for system
    /// ```
    pub fn new(mem_fraction: f64, reserve_gb: f64) -> Self {
        let mut sys = System::new_all();
        sys.refresh_memory();

        let available = sys.available_memory() as usize;
        let reserve = (reserve_gb * 1024.0 * 1024.0 * 1024.0) as usize;
        let usable = available.saturating_sub(reserve);
        let max_memory_bytes = (usable as f64 * mem_fraction) as usize;

        info!(
            "CacheManager init: available={} MB, reserve={} MB, limit={} MB ({}%)",
            available / 1024 / 1024,
            reserve / 1024 / 1024,
            max_memory_bytes / 1024 / 1024,
            (mem_fraction * 100.0) as u32
        );

        Self {
            memory_usage: Arc::new(AtomicUsize::new(0)),
            max_memory_bytes: AtomicUsize::new(max_memory_bytes),
            current_epoch: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Increment epoch and return new value
    ///
    /// Call this when current time changes to cancel all pending preload requests.
    pub fn increment_epoch(&self) -> u64 {
        let new_epoch = self.current_epoch.fetch_add(1, Ordering::Relaxed) + 1;
        debug!("Epoch incremented: {}", new_epoch);
        new_epoch
    }

    /// Get current epoch
    pub fn current_epoch(&self) -> u64 {
        self.current_epoch.load(Ordering::Relaxed)
    }

    /// Get shared epoch counter (for Workers)
    pub fn epoch_ref(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.current_epoch)
    }

    /// Check if memory limit exceeded
    pub fn check_memory_limit(&self) -> bool {
        self.memory_usage.load(Ordering::Relaxed) > self.max_memory_bytes.load(Ordering::Relaxed)
    }

    /// Get memory statistics (usage, limit)
    pub fn mem(&self) -> (usize, usize) {
        let usage = self.memory_usage.load(Ordering::Relaxed);
        let limit = self.max_memory_bytes.load(Ordering::Relaxed);
        (usage, limit)
    }

    /// Get memory usage percentage (0.0-1.0)
    pub fn mem_usage_fraction(&self) -> f64 {
        let (usage, limit) = self.mem();
        if limit == 0 {
            0.0
        } else {
            usage as f64 / limit as f64
        }
    }

    /// Add memory usage
    pub fn add_memory(&self, bytes: usize) {
        let new_usage = self.memory_usage.fetch_add(bytes, Ordering::Relaxed) + bytes;
        let limit = self.max_memory_bytes.load(Ordering::Relaxed);
        if new_usage > limit {
            debug!(
                "Memory limit exceeded: {} MB / {} MB",
                new_usage / 1024 / 1024,
                limit / 1024 / 1024
            );
        }
    }

    /// Free memory usage (saturating subtraction to prevent underflow)
    pub fn free_memory(&self, bytes: usize) {
        // Use compare-exchange loop for saturating subtraction
        loop {
            let current = self.memory_usage.load(Ordering::Relaxed);
            let new_val = current.saturating_sub(bytes);
            if self
                .memory_usage
                .compare_exchange_weak(current, new_val, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
        }
    }

    /// Update memory limit (e.g. from settings)
    /// Now takes &self instead of &mut self thanks to atomic max_memory_bytes
    pub fn set_memory_limit(&self, mem_fraction: f64, reserve_gb: f64) {
        let mut sys = System::new_all();
        sys.refresh_memory();

        let available = sys.available_memory() as usize;
        let reserve = (reserve_gb * 1024.0 * 1024.0 * 1024.0) as usize;
        let usable = available.saturating_sub(reserve);
        let new_limit = (usable as f64 * mem_fraction) as usize;
        self.max_memory_bytes.store(new_limit, Ordering::Relaxed);

        info!(
            "Memory limit updated: {} MB ({}%)",
            new_limit / 1024 / 1024,
            (mem_fraction * 100.0) as u32
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_manager_creation() {
        let manager = CacheManager::new(0.5, 1.0);
        assert_eq!(manager.current_epoch(), 0);

        let (usage, _limit) = manager.mem();
        assert_eq!(usage, 0);
    }

    #[test]
    fn test_epoch_increment() {
        let manager = CacheManager::new(0.5, 1.0);
        assert_eq!(manager.current_epoch(), 0);

        let epoch1 = manager.increment_epoch();
        assert_eq!(epoch1, 1);
        assert_eq!(manager.current_epoch(), 1);

        let epoch2 = manager.increment_epoch();
        assert_eq!(epoch2, 2);
    }

    #[test]
    fn test_memory_tracking() {
        let manager = CacheManager::new(0.5, 1.0);

        manager.add_memory(1024 * 1024); // 1 MB
        let (usage, _) = manager.mem();
        assert_eq!(usage, 1024 * 1024);

        manager.free_memory(512 * 1024); // Free 0.5 MB
        let (usage, _) = manager.mem();
        assert_eq!(usage, 512 * 1024);
    }

}
