//! Global thread pool for background tasks (frame loading, encoding, etc.)
//!
//! Uses work-stealing deques for priority-based execution:
//! - New tasks pushed to front (high priority)
//! - Workers steal old tasks from back (low priority)
//! - Zero lock contention between workers
//!
//! Epoch mechanism allows cancelling stale requests during fast timeline scrubbing.

use crossbeam::deque::{Injector, Worker};
use log::trace;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;

use crate::entities::WorkerPool;

type Job = Box<dyn FnOnce() + Send + 'static>;

/// Global worker pool with work-stealing for priority-based execution.
///
/// New tasks are pushed to front (high priority), old tasks naturally age to back.
/// Workers steal tasks from each other when idle, ensuring work distribution.
///
/// # Example
/// ```ignore
/// let workers = Workers::new(4, epoch);
///
/// // Enqueue frame load (goes to front of queue)
/// workers.execute(move || {
///     frame.set_status(FrameStatus::Loaded).ok();
/// });
/// ```
pub struct Workers {
    injector: Arc<Injector<Job>>,          // Global queue for external tasks
    // Note: stealers Vec cloned into each thread, not stored here
    handles: Vec<thread::JoinHandle<()>>,  // Thread handles for proper shutdown
    current_epoch: Arc<AtomicU64>,         // Epoch counter (shared with CacheManager)
    shutdown: Arc<AtomicBool>,             // Shutdown signal
}

impl Workers {
    /// Create worker pool with work-stealing deques and shared epoch counter.
    ///
    /// Recommended: `num_cpus::get() * 3 / 4` (leave 25% for UI/main thread).
    ///
    /// # Arguments
    ///
    /// * `num_threads` - Number of worker threads
    /// * `epoch` - Shared epoch counter for cancelling stale requests
    pub fn new(num_threads: usize, epoch: Arc<AtomicU64>) -> Self {
        let injector: Arc<Injector<Job>> = Arc::new(Injector::new());
        let shutdown = Arc::new(AtomicBool::new(false));

        let mut workers_local: Vec<Worker<Job>> = Vec::new();
        let mut stealers = Vec::new();
        let mut handles = Vec::new();

        // Create per-worker deques
        for _ in 0..num_threads {
            let worker: Worker<Job> = Worker::new_fifo();
            stealers.push(worker.stealer());
            workers_local.push(worker);
        }

        // Spawn worker threads
        for (worker_id, worker) in workers_local.into_iter().enumerate() {
            let injector = Arc::clone(&injector);
            let shutdown = Arc::clone(&shutdown);
            let stealers = stealers.clone();

            let handle = thread::Builder::new()
                .name(format!("playa-worker-{}", worker_id))
                .spawn(move || {
                    trace!("Worker {} started", worker_id);

                    // Work-stealing loop
                    loop {
                        // 1. Try own queue first (LIFO for cache locality)
                        if let Some(job) = worker.pop() {
                            job();
                            continue;
                        }

                        // 2. Try global injector
                        if let Some(job) = injector.steal().success() {
                            job();
                            continue;
                        }

                        // 3. Try stealing from other workers (oldest tasks first)
                        let mut found_work = false;
                        for stealer in &stealers {
                            if let Some(job) = stealer.steal().success() {
                                job();
                                found_work = true;
                                break;
                            }
                        }

                        if found_work {
                            continue;
                        }

                        // 4. Check shutdown
                        if shutdown.load(Ordering::Relaxed) {
                            break;
                        }

                        // 5. No work - short sleep to avoid CPU spin
                        // Using 1ms sleep instead of pure yield to reduce CPU usage
                        thread::sleep(std::time::Duration::from_millis(1));
                    }

                    trace!("Worker {} stopped", worker_id);
                })
                .expect("Failed to spawn worker thread");

            handles.push(handle);
        }

        trace!("Workers initialized: {} threads (work-stealing)", num_threads);

        Self {
            injector,
            handles,
            current_epoch: epoch,
            shutdown,
        }
    }

    /// Execute closure on worker thread (high priority - goes to front).
    ///
    /// Closure runs asynchronously, no return value.
    /// Use Arc/Mutex for shared state if needed.
    ///
    /// # Example
    /// ```ignore
    /// let frame = frame.clone();
    /// workers.execute(move || {
    ///     // This runs on worker thread
    ///     if let Err(e) = frame.set_status(FrameStatus::Loaded) {
    ///         log::error!("Load failed: {}", e);
    ///     }
    /// });
    /// ```
    pub fn execute<F>(&self, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        // Push to global injector
        // Why: All workers poll the injector, ensuring fair distribution
        // New tasks effectively have high priority as workers check injector before stealing
        self.injector.push(Box::new(f));
    }

    /// Get current epoch
    pub fn current_epoch(&self) -> u64 {
        self.current_epoch.load(Ordering::Relaxed)
    }

    /// Execute closure with epoch check (for cancellable requests).
    ///
    /// Wraps the job with epoch validation that runs at execution time.
    /// Why: Allows tasks to be enqueued immediately but cancelled if epoch changed
    /// before the worker picks them up. Essential for fast timeline scrubbing.
    ///
    /// With work-stealing: newer requests naturally get higher priority as they're
    /// pushed to the injector which workers check before stealing old tasks.
    pub fn execute_with_epoch<F>(&self, epoch: u64, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let current_epoch = Arc::clone(&self.current_epoch);

        // Wrap job with epoch check
        // Why: Check happens at execution time, not enqueue time
        // This allows epoch to change after enqueue but before execution
        let wrapped = move || {
            if current_epoch.load(Ordering::Relaxed) == epoch {
                f(); // Execute only if epoch still matches
            }
            // Otherwise silently skip (epoch changed, request is stale)
        };

        // Push to injector (high priority path)
        self.injector.push(Box::new(wrapped));
    }
}

impl Drop for Workers {
    fn drop(&mut self) {
        use std::time::{Duration, Instant};

        let num_threads = self.handles.len();
        trace!("Workers shutting down ({} threads)...", num_threads);

        // Signal all workers to stop
        self.shutdown.store(true, Ordering::SeqCst);

        // Wait with timeout (500ms total for all threads)
        // After on_exit() increments epoch, pending tasks with epoch check are skipped,
        // so threads should finish quickly. Timeout is a safety net.
        let deadline = Instant::now() + Duration::from_millis(500);

        let handles = std::mem::take(&mut self.handles);
        for handle in handles {
            // Poll until thread finished or timeout
            while !handle.is_finished() {
                if Instant::now() >= deadline {
                    trace!("Shutdown timeout reached, exiting anyway");
                    // Don't join remaining threads - they'll die with process
                    return;
                }
                thread::sleep(Duration::from_millis(1));
            }
            // Thread finished, join to clean up handle
            let _ = handle.join();
        }

        trace!("All {} workers stopped gracefully", num_threads);
    }
}

// ============================================================================
// WorkerPool Trait Implementation
// ============================================================================

impl WorkerPool for Workers {
    fn execute_with_epoch(&self, epoch: u64, f: Box<dyn FnOnce() + Send + 'static>) {
        // Delegate to inherent method, unboxing is handled
        Workers::execute_with_epoch(self, epoch, f)
    }
}
