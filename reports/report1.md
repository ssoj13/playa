# Memory Leak Analysis for Playa

## Overview
Analysis of the memory leak issue reported where memory consumption increases during playback of a 50-frame work area (set with B and N keys) even after frames are already cached.

## Key Findings

### 1. Global Frame Caching System
The application uses a sophisticated caching system with the following components:
- `CacheManager`: Tracks global memory usage and enforces limits
- `GlobalFrameCache`: Single global cache for all compositions, using LRU eviction
- `Comp` entities that interface with the global cache for frame storage

### 2. Potential Memory Leak Sources

#### A. Background Preload Threads
The main issue appears to be in the background preload mechanism:

```rust
// In comp.rs - signal_preload() and enqueue_load() functions
fn enqueue_load(&self, workers: &Arc<Workers>, epoch: u64, frame_idx: i32) {
    // Convert frame_idx to seq_frame (same as get_file_frame)
    let comp_start = self.start();
    let local_idx = frame_idx - comp_start;
    let seq_start = self.file_start.unwrap_or(comp_start);
    let seq_frame = seq_start.saturating_add(local_idx);

    // Skip if already in global cache
    if let Some(ref global_cache) = self.global_cache {
        if global_cache.contains(&self.uuid, seq_frame) {
            return;
        }
    } else {
        return; // No global_cache available
    }

    // Get frame path
    let frame_path = match self.resolve_frame_path(frame_idx) {
        Some(path) => path,
        None => return, // No file to load
    };

    // Clone data for move into closure
    let uuid = self.uuid.clone();
    let global_cache = self.global_cache.as_ref().unwrap().clone();
    let (w, h) = self.dim();

    // Enqueue background load with epoch check
    workers.execute_with_epoch(epoch, move || {
        // ... frame loading logic that adds to cache
    });
}
```

During playback, even within a cached work area, the application continues to call `signal_preload()` which enqueues background tasks for frames around the current playhead. These tasks may continue to be queued without being cancelled properly during rapid playback, leading to multiple attempts to load the same frames.

#### B. Epoch Cancellation During Playback
The epoch-based cancellation system is designed to cancel stale preload requests during fast timeline scrubbing. However, during continuous playback within a work area, the epoch may not be properly incremented to cancel ongoing preload requests. This could result in:

- Multiple concurrent preload requests for the same frames
- Incomplete cancellation of background tasks
- Accumulation of tasks in the worker queue

#### C. LRU Eviction Not Triggering
The LRU eviction in the `GlobalFrameCache::insert()` method may not be aggressive enough during rapid playback. The memory check `self.cache_manager.check_memory_limit()` is only called during insertion, but if frames are being accessed faster than they're being evicted, memory can build up.

#### D. Frame Retention in Active Playback
During playback, the current frame and nearby frames remain in memory. However, if there are timing issues or if frames are repeatedly loaded/reloaded without proper cleanup during each playback cycle, memory can accumulate.

### 3. Memory Tracking Verification
The system does have proper memory tracking:
- Each `CacheManager::add_memory(bytes)` call when frames are added
- Each `CacheManager::free_memory(bytes)` call when frames are evicted
- `Frame::mem()` method to calculate frame size

## Root Cause Hypothesis
The memory leak is most likely caused by **background preload tasks accumulating during playback**. When playing within a work area, the app continues to call `signal_preload()` which may result in:

1. Multiple preload tasks being queued for the same frames
2. Incomplete epoch-based cancellation of stale preload requests
3. Worker threads continuously creating Frame objects that are added to cache before being potentially evicted

The core issue is that during continuous playback, the preload system doesn't distinguish between "already loaded and cached frames" and "frames that need preloading", potentially leading to redundant operations.

## Recommended Fixes

### Immediate Fix
1. Add more aggressive cache size monitoring in the playback loop
2. Implement explicit cleanup of pending preload tasks when entering/exiting playback mode
3. Enhance epoch incrementing during playback state changes

### Long-term Fix
1. Implement smarter preload logic that skips preloading when cache is already populated within the work area
2. Add explicit preload cancellation when stopping playback
3. Consider using weak references or more sophisticated cleanup strategies for background tasks
