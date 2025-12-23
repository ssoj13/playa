//! Abstract traits for dependency inversion.
//!
//! These traits define interfaces that `entities` needs from infrastructure,
//! allowing `core` to depend on `entities` (not vice versa).
//!
//! Implementations live in `core/` module.

use std::sync::Arc;
use uuid::Uuid;

use super::frame::{Frame, FrameStatus};

/// Cache strategy for frame retention (moved here for dependency inversion)
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum CacheStrategy {
    /// Cache only the last accessed frame per comp (minimal memory)
    LastOnly,
    /// Cache all frames within work area (maximum performance)
    #[default]
    All,
}

/// Simple cache statistics (subset exposed via trait)
#[derive(Debug, Clone, Copy, Default)]
pub struct CacheStatsSnapshot {
    pub hits: u64,
    pub misses: u64,
    pub size: usize,
}

impl CacheStatsSnapshot {
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 { 0.0 } else { self.hits as f64 / total as f64 }
    }
}

/// Abstract frame cache interface.
///
/// Allows nodes to cache computed frames without knowing
/// the concrete cache implementation.
pub trait FrameCache: Send + Sync {
    /// Get cached frame by (node_uuid, frame_index).
    /// Returns None if not cached.
    fn get(&self, node_uuid: Uuid, frame_idx: i32) -> Option<Frame>;

    /// Insert frame into cache.
    fn insert(&self, node_uuid: Uuid, frame_idx: i32, frame: Frame);

    /// Get frame status without cloning the frame (lightweight query).
    fn get_status(&self, node_uuid: Uuid, frame_idx: i32) -> Option<FrameStatus>;

    /// Get current cache size (number of cached frames).
    fn len(&self) -> usize;

    /// Check if cache is empty.
    fn is_empty(&self) -> bool { self.len() == 0 }

    /// Set caching strategy.
    fn set_strategy(&self, strategy: CacheStrategy);

    /// Get cache statistics snapshot.
    fn stats_snapshot(&self) -> CacheStatsSnapshot;
}

/// Abstract worker pool interface.
///
/// Allows nodes to schedule background work without knowing
/// the concrete thread pool implementation.
pub trait WorkerPool: Send + Sync {
    /// Execute closure on worker thread with epoch-based cancellation.
    ///
    /// If epoch changed before execution, the closure is skipped.
    /// This allows fast timeline scrubbing without wasted work.
    fn execute_with_epoch(&self, epoch: u64, f: Box<dyn FnOnce() + Send + 'static>);
}

/// Blanket impl: Arc<T> implements traits if T does
impl<T: FrameCache + ?Sized> FrameCache for Arc<T> {
    fn get(&self, node_uuid: Uuid, frame_idx: i32) -> Option<Frame> {
        (**self).get(node_uuid, frame_idx)
    }

    fn insert(&self, node_uuid: Uuid, frame_idx: i32, frame: Frame) {
        (**self).insert(node_uuid, frame_idx, frame)
    }

    fn get_status(&self, node_uuid: Uuid, frame_idx: i32) -> Option<FrameStatus> {
        (**self).get_status(node_uuid, frame_idx)
    }

    fn len(&self) -> usize {
        (**self).len()
    }

    fn set_strategy(&self, strategy: CacheStrategy) {
        (**self).set_strategy(strategy)
    }

    fn stats_snapshot(&self) -> CacheStatsSnapshot {
        (**self).stats_snapshot()
    }
}

impl<T: WorkerPool + ?Sized> WorkerPool for Arc<T> {
    fn execute_with_epoch(&self, epoch: u64, f: Box<dyn FnOnce() + Send + 'static>) {
        (**self).execute_with_epoch(epoch, f)
    }
}
