//! Global thread pool for background tasks (frame loading, encoding, etc.)
//!
//! Uses crossbeam for efficient MPMC queue with closure-based task execution.

use crossbeam::channel::{unbounded, Sender};
use log::{debug, error};
use std::thread;

type Job = Box<dyn FnOnce() + Send + 'static>;

/// Global worker pool for CPU/IO-bound tasks.
///
/// Workers execute arbitrary closures with captured state (payloads).
/// Used for frame loading, encoding, and other background work.
///
/// # Example
/// ```
/// let workers = Workers::new(4);
///
/// // Enqueue frame load
/// workers.execute(move || {
///     frame.set_status(FrameStatus::Loaded).ok();
/// });
/// ```
pub struct Workers {
    sender: Sender<Job>,
    _handles: Vec<thread::JoinHandle<()>>, // Keep handles to prevent premature drop
}

impl Workers {
    /// Create worker pool with `num_threads` threads.
    ///
    /// Recommended: `num_cpus::get() * 3 / 4` (leave 25% for UI/main thread).
    pub fn new(num_threads: usize) -> Self {
        let (tx, rx): (Sender<Job>, _) = unbounded();
        let mut handles = Vec::new();

        for worker_id in 0..num_threads {
            let rx = rx.clone();

            let handle = thread::Builder::new()
                .name(format!("playa-worker-{}", worker_id))
                .spawn(move || {
                    debug!("Worker {} started", worker_id);

                    // Worker loop: execute closures until channel closes
                    while let Ok(job) = rx.recv() {
                        job(); // Execute closure with payload
                    }

                    debug!("Worker {} stopped", worker_id);
                })
                .expect("Failed to spawn worker thread");

            handles.push(handle);
        }

        debug!("Workers initialized: {} threads", num_threads);

        Self {
            sender: tx,
            _handles: handles,
        }
    }

    /// Execute closure on worker thread.
    ///
    /// Closure runs asynchronously, no return value.
    /// Use Arc/Mutex for shared state if needed.
    ///
    /// # Example
    /// ```
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
        if let Err(e) = self.sender.send(Box::new(f)) {
            error!("Failed to enqueue job: {}", e);
        }
    }
}

// Drop implementation: channels close automatically, threads exit gracefully
impl Drop for Workers {
    fn drop(&mut self) {
        debug!("Workers shutting down ({} threads)...", self._handles.len());
        // Sender drops → channel closes → workers exit recv() loop
    }
}
