//! Multi-sequence frame cache with LRU eviction and concurrent access
//!
//! **Why**: Smooth playback requires keeping decoded frames in RAM. With 4K EXR sequences
//! at ~64MB/frame, we need intelligent eviction and fast concurrent reads.
//!
//! **Used by**: Player (frame display), UI (timeline scrubbing), Viewport (rendering)
//!
//! # Architecture
//!
//! - **LruCache**: O(1) access and eviction via `lru` crate
//! - **RwLock**: Multiple concurrent readers, single writer for cache operations
//! - **AtomicUsize**: Lock-free memory tracking across threads
//! - **Worker pool**: 75% of CPU cores for parallel frame loading
//! - **Spiral preload**: Loads frames in order: 0, ±1, ±2, ±3... from current position
//!
//! # Memory Management
//!
//! Default 4GB limit (configurable via `max_memory_mb`). LRU eviction removes
//! least-recently-accessed frames when limit reached.
//!
//! # Concurrency
//!
//! Read operations (`get_frame`, `contains`) use read locks - multiple threads simultaneously.
//! Write operations (`insert`, evict) use write locks - exclusive access.
//! Atomic frame claiming prevents duplicate loads (TOCTOU race).

use log::{debug, info, warn};
use lru::LruCache;
use serde::{Deserialize, Serialize};
use std::sync::{mpsc, Arc, Mutex, RwLock};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::thread;
// SystemTime/UNIX_EPOCH removed - LruCache handles access tracking automatically
use std::path::{Path, PathBuf};
use sysinfo::System;

use crate::frame::{Frame, FrameStatus};
use crate::sequence::Sequence;
use crate::progress::LoadProgress;

/// Load request for worker threads
#[derive(Debug)]
struct LoadRequest {
    frame: Frame,     // Clone of Arc - cheap!
    seq_idx: usize,   // For tracking/result
    frame_idx: usize,
    epoch: u64,       // For cancelling stale requests
}

/// Lightweight frame info for preload thread
#[derive(Debug, Clone)]
struct FramePath {
    frame: Frame,     // Clone of Arc - cheap!
    seq_idx: usize,
    frame_idx: usize,
}

/// Loaded frame result
#[derive(Debug)]
struct LoadedFrame {
    seq_idx: usize,
    frame_idx: usize,
    result: Result<Frame, String>,
}

/// Messages sent to UI for status updates
#[derive(Debug, Clone)]
pub enum CacheMessage {
    SequenceDetected(Sequence),  // Async sequence detection complete
    FrameLoaded,
    LoadProgress { cached_count: usize, total_count: usize },
    StatusMessage(String),
}

/// Cache state for serialization/deserialization
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheState {
    sequences: Vec<Sequence>,
    current_frame: usize,
}

/// Cache entry with access tracking
#[derive(Debug, Clone)]
struct CacheEntry {
    frame: Frame,
    // Note: LruCache automatically tracks access order, no manual timestamp needed
}

/// Main cache with multiple sequences
#[derive(Debug)]
pub struct Cache {
    sequences: Vec<Sequence>,
    global_start: usize,
    global_end: usize,
    global_frame: usize,

    // LRU cache for loaded frames (O(1) access, eviction, insertion)
    // RwLock allows multiple concurrent readers or single writer
    lru_cache: Arc<RwLock<LruCache<(usize, usize), CacheEntry>>>, // (seq_idx, frame_idx) -> Frame
    memory_usage: Arc<AtomicUsize>,
    max_memory_bytes: usize,

    // Async loading (bounded channels for backpressure)
    load_request_sender: mpsc::SyncSender<LoadRequest>,
    loaded_frame_receiver: Arc<Mutex<mpsc::Receiver<LoadedFrame>>>,

    // UI notifications
    ui_message_sender: mpsc::Sender<CacheMessage>,

    // Preload signaling
    preload_tx: mpsc::Sender<(usize, usize, Vec<FramePath>)>, // (center_frame, global_end, frame_paths)
    cancel_preload: Arc<AtomicBool>,
    current_epoch: Arc<AtomicU64>, // Epoch counter for cancelling stale requests

    // Cached frame paths (updated when sequences change)
    frame_paths_cache: Vec<FramePath>,

    // Sequence change tracking
    sequences_version: Arc<AtomicUsize>,

    // Progress tracking
    progress: LoadProgress,

    // Incremented on each successfully loaded frame (for UI invalidation)
    loaded_events_counter: AtomicUsize,
}

impl Cache {
    /// Create frame cache with memory limit and worker pool
    ///
    /// **Why**: Initializes LRU cache, worker threads, and preload system for parallel frame loading
    ///
    /// **Used by**: Application startup (`main.rs`)
    ///
    /// # Arguments
    ///
    /// - `max_mem`: Percentage of available RAM (0.0-1.0). Default: 0.75 (75%)
    ///
    /// Worker pool size: 75% of CPU cores (leaves room for UI/decode threads)
    ///
    /// # Returns
    ///
    /// Tuple: `(Cache, UI message receiver, Path sender for async sequence loading)`
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use playa::cache::Cache;
    /// let (cache, ui_rx, path_tx) = Cache::new(0.75, None); // 75% of available RAM, default workers
    /// ```
    pub fn new(max_mem: f64, workers_override: Option<usize>) -> (Self, mpsc::Receiver<CacheMessage>, mpsc::Sender<PathBuf>) {
        let mut sys = System::new_all();
        sys.refresh_memory();

        let total_memory = sys.total_memory() as usize;
        let available_memory = sys.available_memory() as usize;
        let max_memory_bytes = (available_memory as f64 * max_mem) as usize;

        info!("System memory: {} MB total, {} MB available",
              total_memory / 1024 / 1024,
              available_memory / 1024 / 1024);
        info!("Cache limit: {} MB ({}% of available)",
              max_memory_bytes / 1024 / 1024,
              max_mem * 100.0);

        // Calculate worker count first (needed for channel capacity)
        let num_workers = if let Some(w) = workers_override { w.max(1) } else {
            (std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(8) * 3 / 4)
                .max(1)
        };

        info!("Using {} worker threads", num_workers);

        // Create channels with bounded capacity for backpressure
        let (load_request_sender, load_request_receiver) =
            mpsc::sync_channel::<LoadRequest>(num_workers * 4);
        let (loaded_frame_sender, loaded_frame_receiver) =
            mpsc::sync_channel::<LoadedFrame>(num_workers * 4);
        let (ui_message_sender, ui_message_receiver) = mpsc::channel::<CacheMessage>();
        let (path_sender, path_receiver) = mpsc::channel::<PathBuf>();

        // Path processing thread - async sequence detection
        let ui_sender_for_paths = ui_message_sender.clone();
        thread::spawn(move || {
            // Wrap path processing logic in catch_unwind for graceful panic recovery
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                info!("Path processing thread started");
                while let Ok(path) = path_receiver.recv() {
                    info!("Detecting sequence from path: {}", path.display());
                    match Sequence::detect(vec![path.clone()]) {
                        Ok(sequences) => {
                            for seq in sequences {
                                info!("Detected sequence: {} ({} frames)", seq.pattern(), seq.len());
                                let _ = ui_sender_for_paths.send(CacheMessage::SequenceDetected(seq));
                            }
                        }
                        Err(e) => {
                            let msg = format!("Failed to load {}: {}", path.display(), e);
                            warn!("{}", msg);
                            let _ = ui_sender_for_paths.send(CacheMessage::StatusMessage(msg));
                        }
                    }
                }
                info!("Path processing thread exiting");
            }));

            if let Err(e) = result {
                log::error!("Path processing thread panicked: {:?}", e);
            }
        });

        // Shared structures
        // LruCache with unbounded capacity (we use memory-based eviction instead)
        // RwLock allows multiple concurrent readers for better performance
        let lru_cache_shared = Arc::new(RwLock::new(LruCache::unbounded()));
        let memory_usage_shared = Arc::new(AtomicUsize::new(0));
        let current_epoch_shared = Arc::new(AtomicU64::new(0));

        let load_request_receiver = Arc::new(Mutex::new(load_request_receiver));

        // Start worker threads
        info!("Starting {} worker threads", num_workers);

        for worker_id in 0..num_workers {
            let receiver = Arc::clone(&load_request_receiver);
            let sender = loaded_frame_sender.clone();
            let ui_sender = ui_message_sender.clone();
            let worker_epoch = Arc::clone(&current_epoch_shared);

            thread::spawn(move || {
                // Wrap worker logic in catch_unwind for graceful panic recovery
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    loop {
                        let req = {
                            let receiver = receiver.lock().unwrap();
                            receiver.recv()
                        };

                        match req {
                            Ok(req) => {
                                // Check if request is stale (from old epoch)
                                let current_epoch = worker_epoch.load(Ordering::Relaxed);
                                if req.epoch != current_epoch {
                                    continue; // Skip stale request
                                }

                                // Load frame - atomic claim prevents duplicates
                                // Frame.load() will skip if already loading/loaded
                                let result = req.frame.load()
                                    .map(|_| req.frame.clone())
                                    .map_err(|e| e.to_string());

                                // Log only failures
                                if let Err(ref e) = result {
                                    warn!("Worker {}: failed [{},{}]: {}", worker_id, req.seq_idx, req.frame_idx, e);
                                }

                                let loaded_frame = LoadedFrame {
                                    seq_idx: req.seq_idx,
                                    frame_idx: req.frame_idx,
                                    result: result.clone(),
                                };

                                if sender.send(loaded_frame).is_err() {
                                    break;
                                }

                                // Notify UI about loaded frame
                                if result.is_ok() {
                                    let _ = ui_sender.send(CacheMessage::FrameLoaded);
                                }
                            },
                            Err(_) => break,
                        }
                    }
                }));

                if let Err(e) = result {
                    log::error!("Worker {} panicked: {:?}", worker_id, e);
                }
            });
        }

        // Preload thread
        let (preload_tx, preload_rx) = mpsc::channel::<(usize, usize, Vec<FramePath>)>();
        let cancel_preload = Arc::new(AtomicBool::new(false));

        let preload_sender = load_request_sender.clone();
        let preload_cancel = Arc::clone(&cancel_preload);
        let preload_lru = Arc::clone(&lru_cache_shared);
        let preload_epoch = Arc::clone(&current_epoch_shared);

        thread::spawn(move || {
            // Wrap preload logic in catch_unwind for graceful panic recovery
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut session_counter = 0u64;
                loop {
                    // Wait for first signal (blocks, no CPU usage when idle)
                    let mut latest = match preload_rx.recv() {
                        Ok(msg) => msg,
                        Err(_) => break,
                    };

                    // Drain channel to get LATEST message (skip stale requests from fast UI clicks)
                    while let Ok(msg) = preload_rx.try_recv() {
                        latest = msg;
                    }

                    let (center_frame, global_end, frame_paths) = latest;

                    // Increment epoch counter
                    session_counter += 1;
                    let epoch = session_counter;
                    preload_epoch.store(epoch, Ordering::Relaxed);
                    debug!("Preload epoch {}: center={}, total={}", epoch, center_frame, global_end + 1);

                    // Reset cancel flag
                    preload_cancel.store(false, Ordering::Relaxed);

                    // Spiral load from center: 0, +1, -1, +2, -2, ...
                    let mut sent = 0;
                    let mut skipped = 0;
                    for offset in 0..=global_end {
                        // Check cancel flag BEFORE each request
                        if preload_cancel.load(Ordering::Relaxed) {
                            debug!("Preload epoch {} cancelled at offset {} ({} sent, {} skipped)",
                                   epoch, offset, sent, skipped);
                            break;
                        }

                        // Helper to send request if not already loaded
                        let try_send = |global_idx: usize| -> bool {
                            if let Some(fp) = frame_paths.get(global_idx) {
                                // Check if already loaded (read lock - concurrent access OK)
                                let lru = preload_lru.read().unwrap();
                                if lru.contains(&(fp.seq_idx, fp.frame_idx)) {
                                    return false; // Already loaded, skip
                                }
                                drop(lru);

                                let req = LoadRequest {
                                    frame: fp.frame.clone(), // Clone Arc - cheap!
                                    seq_idx: fp.seq_idx,
                                    frame_idx: fp.frame_idx,
                                    epoch,
                                };
                                return preload_sender.send(req).is_ok();
                            }
                            false
                        };

                        // Load backward
                        if center_frame >= offset {
                            let global_idx = center_frame - offset;
                            if try_send(global_idx) {
                                sent += 1;
                            } else if frame_paths.get(global_idx).is_some() {
                                skipped += 1;
                            }
                        }

                        // Load forward (skip offset=0 as already loaded)
                        if offset > 0 && center_frame + offset <= global_end {
                            let global_idx = center_frame + offset;
                            if try_send(global_idx) {
                                sent += 1;
                            } else if frame_paths.get(global_idx).is_some() {
                                skipped += 1;
                            }
                        }
                    }

                    debug!("Preload epoch {} finished: {} sent, {} already loaded", epoch, sent, skipped);
                }
            }));

            if let Err(e) = result {
                log::error!("Preload thread panicked: {:?}", e);
            }
        });

        let cache = Self {
            sequences: Vec::new(),
            global_start: 0,
            global_end: 0,
            global_frame: 0,

            lru_cache: lru_cache_shared,
            memory_usage: memory_usage_shared,
            max_memory_bytes,

            load_request_sender,
            loaded_frame_receiver: Arc::new(Mutex::new(loaded_frame_receiver)),

            ui_message_sender,

            preload_tx,
            cancel_preload,
            current_epoch: current_epoch_shared,

            frame_paths_cache: Vec::new(),

            sequences_version: Arc::new(AtomicUsize::new(0)),

            progress: LoadProgress::new(0),

            loaded_events_counter: AtomicUsize::new(0),
        };

        (cache, ui_message_receiver, path_sender)
    }

    /// Append sequence to cache
    pub fn append_seq(&mut self, seq: Sequence) {
        let seq_len = seq.len();

        self.sequences.push(seq);
        self.global_end = self.global_start + self.total_frames().saturating_sub(1);
        self.progress.set_total(self.total_frames());

        // Update frame paths cache
        self.rebuild_frame_paths_cache();

        // Increment version to invalidate UI cache
        self.sequences_version.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        info!("Appended sequence: {} frames, global_end={}", seq_len, self.global_end);
    }

    /// Update existing sequence in-place (when sequence changed on disk)
    pub fn update_sequence(&mut self, idx: usize, new_seq: Sequence) {
        if idx >= self.sequences.len() {
            warn!("Invalid sequence index: {}", idx);
            return;
        }

        let old_len = self.sequences[idx].len();
        let new_len = new_seq.len();

        info!("Updating sequence [{}]: {} → {} frames", idx, old_len, new_len);

        // If fewer frames - remove extras from cache
        if new_len < old_len {
            let mut lru = self.lru_cache.write().unwrap();
            for frame_idx in new_len..old_len {
                if let Some(entry) = lru.pop(&(idx, frame_idx)) {
                    let removed_size = entry.frame.mem();
                    self.memory_usage.fetch_sub(removed_size, Ordering::Relaxed);
                }
            }
            info!("Removed {} frames from cache", old_len - new_len);
        }

        // Replace sequence
        self.sequences[idx] = new_seq;

        // Recalculate global_end
        self.global_end = self.global_start + self.total_frames().saturating_sub(1);

        // Rebuild frame paths cache
        self.rebuild_frame_paths_cache();

        // Increment version to invalidate UI cache
        self.sequences_version.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// Clear all sequences
    pub fn clear(&mut self) {
        self.sequences.clear();
        self.global_start = 0;
        self.global_end = 0;
        self.global_frame = 0;

        // Clear cache
        let mut lru = self.lru_cache.write().unwrap();
        lru.clear();
        self.memory_usage.store(0, Ordering::Relaxed);

        // Clear frame paths cache
        self.frame_paths_cache.clear();

        // Clear progress
        self.progress.clear();

        // Increment version to invalidate UI cache
        self.sequences_version.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// Rebuild frame paths cache from current sequences
    fn rebuild_frame_paths_cache(&mut self) {
        self.frame_paths_cache.clear();
        for (seq_idx, seq) in self.sequences.iter().enumerate() {
            for frame_idx in 0..seq.len() {
                if let Some(frame) = seq.idx(frame_idx as isize, false) {
                    if frame.file().is_some() {
                        self.frame_paths_cache.push(FramePath {
                            frame: frame.clone(), // Clone Arc - cheap!
                            seq_idx,
                            frame_idx,
                        });
                    }
                }
            }
        }
    }

    /// Get total frame count across all sequences
    pub fn total_frames(&self) -> usize {
        self.sequences.iter().map(|s| s.len()).sum()
    }

    /// Map global index to (seq_idx, frame_idx)
    fn global_to_local(&self, global_idx: usize) -> Option<(usize, usize)> {
        let mut offset = 0;

        for (seq_idx, seq) in self.sequences.iter().enumerate() {
            let seq_len = seq.len();
            if global_idx < offset + seq_len {
                let frame_idx = global_idx - offset;
                return Some((seq_idx, frame_idx));
            }
            offset += seq_len;
        }

        None
    }

    /// Get cached frame for display (non-blocking read)
    ///
    /// **Why**: UI needs fast frame lookup without blocking other readers
    ///
    /// **Used by**: Viewport rendering (every frame), timeline scrubbing
    ///
    /// # Arguments
    ///
    /// - `global_idx`: Frame index across all sequences (0-based)
    ///
    /// # Returns
    ///
    /// - `Some(Frame)`: Frame loaded and ready for display
    /// - `None`: Frame not in cache (still loading, not requested, or invalid index)
    ///
    /// # Performance
    ///
    /// - O(1) LRU lookup
    /// - Read lock: Multiple threads can call simultaneously
    /// - No write lock: Doesn't block cache updates
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use playa::cache::Cache;
    /// # let (mut cache, _, _) = Cache::new(0.75);
    /// if let Some(frame) = cache.get_frame(42) {
    ///     // Frame ready for rendering
    ///     println!("Frame: {}x{}", frame.width(), frame.height());
    /// } else {
    ///     // Frame not loaded yet, show placeholder
    /// }
    /// ```
    pub fn get_frame(&mut self, global_idx: usize) -> Option<&Frame> {
        // Note: processing of loaded frames is centralized in the UI loop
        
        let (seq_idx, frame_idx) = self.global_to_local(global_idx)?;

        // Check if cached (read lock - allows concurrent access from other threads)
        {
            let lru = self.lru_cache.read().unwrap();
            if lru.contains(&(seq_idx, frame_idx)) {
                drop(lru);
                // No need to update access time here - LruCache already tracks order
                // and we don't want write locks on hot path (UI frame display)
                return self.sequences.get(seq_idx)?.idx(frame_idx as isize, false);
            }
        }

        // Not cached, trigger load ONLY if not already loaded
        if let Some(seq) = self.sequences.get(seq_idx) {
            if let Some(frame) = seq.idx(frame_idx as isize, false) {
                // Check if frame needs loading (Header status = file set but not loaded)
                if matches!(frame.status(), FrameStatus::Header) {
                    let _ = self.load_request_sender.send(LoadRequest {
                        frame: frame.clone(), // Clone Arc - cheap!
                        seq_idx,
                        frame_idx,
                        epoch: self.current_epoch.load(Ordering::Relaxed),
                    });
                }
            }
        }

        // Return placeholder from sequence
        self.sequences.get(seq_idx)?.idx(frame_idx as isize, false)
    }

    /// Process loaded frames from worker threads
    pub fn process_loaded_frames(&mut self) {
        loop {
            let receiver = self.loaded_frame_receiver.lock().unwrap();
            let result = receiver.try_recv();
            drop(receiver);

            match result {
                Ok(loaded_frame) => {
                    match loaded_frame.result {
                        Ok(frame) => {
                            let frame_size = frame.mem();

                            let mut lru = self.lru_cache.write().unwrap();

                            // Ensure space (O(1) eviction with pop_lru)
                            self.ensure_space_locked(&mut lru, frame_size);

                            // Cache frame (LruCache automatically tracks access order)
                            let key = (loaded_frame.seq_idx, loaded_frame.frame_idx);
                            lru.put(key, CacheEntry {
                                frame: frame.clone(),
                            });  // O(1) insertion + automatic access tracking!
                            self.memory_usage.fetch_add(frame_size, Ordering::Relaxed);

                            // Update sequence frame
                            if let Some(seq) = self.sequences.get_mut(loaded_frame.seq_idx) {
                                if let Some(seq_frame) = seq.idx_mut(loaded_frame.frame_idx as isize, false) {
                                    *seq_frame = frame;
                                }
                            }

                            // Progress
                            self.progress.update(loaded_frame.seq_idx, loaded_frame.frame_idx);

                            // Notify UI-side caches that a frame successfully loaded
                            self.loaded_events_counter.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(error_msg) => {
                            warn!("Failed to load frame ({}, {}): {}",
                                  loaded_frame.seq_idx, loaded_frame.frame_idx, error_msg);

                            // Reset frame back to Placeholder so playback can continue
                            if let Some(seq) = self.sequences.get_mut(loaded_frame.seq_idx) {
                                if let Some(seq_frame) = seq.idx_mut(loaded_frame.frame_idx as isize, false) {
                                    use crate::frame::FrameStatus;
                                    seq_frame.set_status(FrameStatus::Placeholder);
                                }
                            }
                        }
                    }
                }
                Err(_) => break,
            }
        }

        // Send progress update to UI after processing all loaded frames
        let cached_count = self.cached_frames_count();
        let total_count = self.total_frames();
        let _ = self.ui_message_sender.send(CacheMessage::LoadProgress {
            cached_count,
            total_count,
        });
    }

    // Note: update_access_time() removed - LruCache automatically maintains access order
    // No need for manual timestamp tracking or write locks on read path

    /// Ensure space for new frame (LRU eviction with O(1) pop_lru)
    fn ensure_space_locked(
        &self,
        lru: &mut LruCache<(usize, usize), CacheEntry>,
        new_frame_size: usize,
    ) {
        let memory = &self.memory_usage;

        while memory.load(Ordering::Relaxed) + new_frame_size > self.max_memory_bytes {
            if let Some((key, entry)) = lru.pop_lru() {  // O(1) eviction!
                let removed_size = entry.frame.mem();
                memory.fetch_sub(removed_size, Ordering::Relaxed);
                debug!("Evicted frame {:?} ({} bytes)", key, removed_size);
            } else {
                // No more entries in cache
                break;
            }
        }
    }

    /// Get memory usage
    pub fn mem(&self) -> (usize, usize) {
        let usage = self.memory_usage.load(Ordering::Relaxed);
        (usage, self.max_memory_bytes)
    }

    /// Update memory limit as a fraction of currently available system memory
    /// and immediately enforce the new limit by evicting least-recently-used frames.
    pub fn set_memory_fraction(&mut self, max_mem: f64) {
        let mut sys = System::new_all();
        sys.refresh_memory();
        let available_memory = sys.available_memory() as usize;
        self.max_memory_bytes = (available_memory as f64 * max_mem) as usize;

        // Evict if over the new budget
        self.enforce_memory_limit();
    }

    /// Evict LRU frames until usage <= max_memory_bytes
    pub fn enforce_memory_limit(&mut self) {
        let mut lru = self.lru_cache.write().unwrap();
        while self.memory_usage.load(Ordering::Relaxed) > self.max_memory_bytes {
            if let Some((_key, entry)) = lru.pop_lru() {
                let removed_size = entry.frame.mem();
                self.memory_usage.fetch_sub(removed_size, Ordering::Relaxed);
            } else {
                break;
            }
        }
    }

    /// Get count of cached frames in memory
    pub fn cached_frames_count(&self) -> usize {
        let lru = self.lru_cache.read().unwrap();
        lru.len()
    }

    /// Get sequences (cloned)
    pub fn sequences(&self) -> Vec<Sequence> {
        self.sequences.clone()
    }

    /// Set global frame
    pub fn set_frame(&mut self, global_idx: usize) {
        self.global_frame = global_idx.min(self.global_end);
    }

    /// Remove sequence by index
    pub fn remove_seq(&mut self, seq_idx: usize) {
        if seq_idx < self.sequences.len() {
            self.sequences.remove(seq_idx);

            // Recalculate global_end
            self.global_end = self.total_frames().saturating_sub(1);

            // Adjust global_frame if needed
            if self.global_frame > self.global_end {
                self.global_frame = self.global_end;
            }

            // Reindex cache to reflect new sequence positions
            self.reindex();

            // Rebuild frame paths cache
            self.rebuild_frame_paths_cache();

            // Increment version to invalidate UI cache
            self.sequences_version.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Reindex entire cache based on current sequences state
    /// Uses frame paths as source of truth for correct indices
    fn reindex(&mut self) {
        let mut lru = self.lru_cache.write().unwrap();

        // Build path -> (seq_idx, frame_idx) mapping from current state
        let mut path_to_idx = std::collections::HashMap::new();
        for fp in &self.frame_paths_cache {
            if let Some(path) = fp.frame.file() {
                path_to_idx.insert(path.clone(), (fp.seq_idx, fp.frame_idx));
            }
        }

        // Rebuild LRU cache with correct indices
        // LruCache doesn't have drain(), so we collect keys first then rebuild
        let old_entries: Vec<_> = {
            let mut entries = Vec::new();
            // Iterate and collect all entries (preserves order for LRU)
            while let Some((key, entry)) = lru.pop_lru() {
                entries.push((key, entry));
            }
            entries
        };

        // Reinsert with correct indices based on path (in reverse order to preserve LRU)
        for ((_old_seq, _old_frame), entry) in old_entries.into_iter().rev() {
            if let Some(path) = entry.frame.file() {
                if let Some(&(new_seq, new_frame)) = path_to_idx.get(path) {
                    lru.put((new_seq, new_frame), entry);
                }
            }
            // If path not found, frame was removed - don't reinsert
        }
    }

    /// Move sequence by offset (-1 = up, +1 = down, etc.)
    pub fn move_seq(&mut self, seq_idx: usize, offset: isize) {
        if offset == 0 || self.sequences.is_empty() {
            return;
        }

        let len = self.sequences.len();
        if seq_idx >= len {
            return;
        }

        // Calculate new index with bounds checking
        let new_idx = if offset < 0 {
            let abs_offset = (-offset) as usize;
            seq_idx.saturating_sub(abs_offset)
        } else {
            (seq_idx + offset as usize).min(len - 1)
        };

        if new_idx == seq_idx {
            return;
        }

        // Remove sequence from old position
        let seq = self.sequences.remove(seq_idx);

        // Insert at new position
        self.sequences.insert(new_idx, seq);

        // Reindex cache to reflect new sequence positions
        self.reindex();

        self.rebuild_frame_paths_cache();

        // Increment version to invalidate UI cache
        self.sequences_version.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// Jump to start of sequence
    pub fn jump_to_seq(&mut self, seq_idx: usize) {
        let mut offset = 0;
        for (idx, seq) in self.sequences.iter().enumerate() {
            if idx == seq_idx {
                self.global_frame = offset;
                return;
            }
            offset += seq.len();
        }
    }

    /// Get global frame
    pub fn frame(&self) -> usize {
        self.global_frame
    }

    /// Get global range
    pub fn range(&self) -> (usize, usize) {
        (self.global_start, self.global_end)
    }

    /// Signal preload thread to start loading frames from current position
    pub fn signal_preload(&self) {
        // Set cancel flag to interrupt any ongoing preload
        self.cancel_preload.store(true, Ordering::Relaxed);

        // Use cached frame paths (cheap clone - only PathBuf which is Arc internally)
        let frame_paths = self.frame_paths_cache.clone();
        let center = self.global_frame;
        let total = self.global_end;

        if let Err(e) = self.preload_tx.send((center, total, frame_paths)) {
            warn!("Failed to signal preload: {}", e);
        }
    }

    /// Save cache state to JSON file (sequences + current frame)
    pub fn to_json(&self, path: &Path) -> Result<(), String> {
        // Ensure .json extension
        let path = if path.extension().and_then(|s| s.to_str()) != Some("json") {
            path.with_extension("json")
        } else {
            path.to_path_buf()
        };

        // Create cache state (sequences are cloned, frames are skipped during serialization)
        let state = CacheState {
            sequences: self.sequences.clone(),
            current_frame: self.global_frame,
        };

        let json = serde_json::to_string_pretty(&state)
            .map_err(|e| format!("Serialize error: {}", e))?;

        std::fs::write(&path, json)
            .map_err(|e| format!("Write error: {}", e))?;

        info!("Cache state saved to {}: {} sequences, frame {}",
              path.display(), state.sequences.len(), state.current_frame);
        Ok(())
    }

    /// Load cache state from JSON file (fast restore, no I/O)
    /// - append=true: add sequences to existing playlist
    /// - append=false: clear cache before loading
    pub fn from_json(&mut self, path: &Path, append: bool) -> Result<usize, String> {
        let json = std::fs::read_to_string(path)
            .map_err(|e| format!("Read error: {}", e))?;

        let mut state: CacheState = serde_json::from_str(&json)
            .map_err(|e| format!("Parse error: {}", e))?;

        if !append {
            info!("Clearing cache before loading");
            self.clear();
        }

        info!("Restoring {} sequence(s) from cache", state.sequences.len());

        // Restore frames for each sequence (creates unloaded Frame placeholders)
        for seq in &mut state.sequences {
            seq.restore_frames();
            self.append_seq(seq.clone());
        }

        // Restore current frame
        self.set_frame(state.current_frame);

        info!("Cache restored: {} sequences, current frame {}",
              state.sequences.len(), self.global_frame);

        Ok(state.sequences.len())
    }

    /// Get current sequences version (incremented when sequences change)
    pub fn sequences_version(&self) -> usize {
        self.sequences_version.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Get status for all frames in global order
    pub fn get_frame_stats(&self) -> Vec<FrameStatus> {
        let mut stats = Vec::with_capacity(self.total_frames());

        for seq in &self.sequences {
            for frame_idx in 0..seq.len() {
                if let Some(frame) = seq.idx(frame_idx as isize, false) {
                    stats.push(frame.status());
                }
            }
        }

        stats
    }

    /// Monotonic counter incremented on each successful frame load
    pub fn loaded_events_counter(&self) -> usize {
        self.loaded_events_counter.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    /// Test: Cache initialization
    /// Validates: Basic cache creation and structure
    #[test]
    fn test_cache_creation() {
        let (cache, _ui_rx, _path_tx) = Cache::new(0.5, None); // 50% of RAM

        // Cache should start empty
        assert_eq!(cache.total_frames(), 0);

        // Memory tracking should be initialized
        assert_eq!(cache.memory_usage.load(Ordering::Relaxed), 0);
    }

    /// Test: Concurrent reads don't block each other
    /// Validates: RwLock allows multiple simultaneous readers
    #[test]
    fn test_concurrent_reads() {
        let (cache, _ui_rx, _path_tx) = Cache::new(0.1, None);
        let cache = Arc::new(cache);

        let mut handles = vec![];

        // Spawn 10 reader threads
        for _ in 0..10 {
            let cache_clone = Arc::clone(&cache);
            let handle = thread::spawn(move || {
                // Read lock - should not block other readers
                let lru = cache_clone.lru_cache.read().unwrap();
                thread::sleep(std::time::Duration::from_millis(10));
                drop(lru);
            });
            handles.push(handle);
        }

        // All threads should complete without deadlock
        for handle in handles {
            handle.join().unwrap();
        }
    }

    /// Test: Concurrent load attempts don't panic
    /// Validates: Multiple threads can safely attempt frame loading
    #[test]
    fn test_concurrent_load_attempts() {
        use crate::frame::Frame;
        use std::path::PathBuf;

        let frame = Frame::new_unloaded(PathBuf::from("test.exr"));
        let frame: Arc<Frame> = Arc::new(frame);

        let mut handles = vec![];

        // Spawn 5 threads trying to load same frame
        for _ in 0..5 {
            let frame_clone = Arc::clone(&frame);

            let handle = thread::spawn(move || {
                // All threads can safely call load() - atomic claiming prevents duplicates
                let _ = frame_clone.load();
            });
            handles.push(handle);
        }

        // All threads should complete without panic
        for handle in handles {
            handle.join().unwrap();
        }

        // Frame should be in Error state (file doesn't exist)
        assert_eq!(frame.status(), FrameStatus::Error);
    }
}

