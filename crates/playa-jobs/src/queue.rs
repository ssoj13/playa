//! Concrete [`JobQueue`] implementation.
//!
//! Worker model: N=`max(2, ncpu/4)` long-lived threads waiting on a
//! `(Mutex<VecDeque<JobId>>, Condvar)` work queue. One additional updater
//! thread drains state-update messages from a `mpsc::Receiver` and writes
//! them to the in-memory job map + persistence log + listeners.
//!
//! All public reads (`get`, `list`) clone out of the lock so callers don't
//! block writers on the UI thread.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::cancel::CancelToken;
use crate::event::JobEvent;
use crate::job::{Job, JobError, JobId, JobState, now_secs};
#[cfg(feature = "persist")]
use crate::persist::{Log as PersistLog, LogEntry};
use crate::provider::{JobContext, JobProvider, UpdateMsg};

/// Listener callback invoked synchronously from the updater thread.
pub type Listener = Box<dyn Fn(JobEvent) + Send + Sync>;

#[derive(Debug, Clone)]
pub struct JobQueueConfig {
    /// Number of IO worker threads. Defaults to `max(2, ncpu / 4)` to leave
    /// CPU headroom for the engine's frame-decode pool.
    pub thread_count: usize,
    /// Per-job staging directory root; each job writes to
    /// `{files_dir}/{job_id}/`.
    pub files_dir: PathBuf,
    /// JSONL persistence log path. `None` disables persistence even with the
    /// `persist` feature compiled in (useful for tests).
    #[cfg(feature = "persist")]
    pub persist_path: Option<PathBuf>,
}

impl Default for JobQueueConfig {
    fn default() -> Self {
        let nthreads = thread::available_parallelism()
            .map(|n| n.get() / 4)
            .unwrap_or(2)
            .max(2);
        Self {
            thread_count: nthreads,
            files_dir: std::env::temp_dir().join("playa-jobs"),
            #[cfg(feature = "persist")]
            persist_path: None,
        }
    }
}

#[derive(Clone)]
pub struct JobQueue {
    inner: Arc<Inner>,
}

struct Inner {
    jobs: RwLock<HashMap<JobId, Job>>,
    cancel_tokens: RwLock<HashMap<JobId, CancelToken>>,
    providers: RwLock<HashMap<String, Arc<dyn JobProvider>>>,
    listeners: RwLock<Vec<Listener>>,
    files_dir: PathBuf,
    work_queue: (Mutex<VecDeque<JobId>>, Condvar),
    update_tx: Sender<UpdateMsg>,
    shutdown: AtomicBool,
    workers: Mutex<Vec<JoinHandle<()>>>,
    updater: Mutex<Option<JoinHandle<()>>>,
    #[cfg(feature = "persist")]
    persist: Option<Arc<PersistLog>>,
}

impl JobQueue {
    pub fn new(config: JobQueueConfig) -> std::io::Result<Self> {
        std::fs::create_dir_all(&config.files_dir)?;

        #[cfg(feature = "persist")]
        let persist = match &config.persist_path {
            Some(path) => Some(Arc::new(PersistLog::open(path.clone())?)),
            None => None,
        };

        let (update_tx, update_rx) = mpsc::channel::<UpdateMsg>();

        let inner = Arc::new(Inner {
            jobs: RwLock::new(HashMap::new()),
            cancel_tokens: RwLock::new(HashMap::new()),
            providers: RwLock::new(HashMap::new()),
            listeners: RwLock::new(Vec::new()),
            files_dir: config.files_dir,
            work_queue: (Mutex::new(VecDeque::new()), Condvar::new()),
            update_tx,
            shutdown: AtomicBool::new(false),
            workers: Mutex::new(Vec::with_capacity(config.thread_count)),
            updater: Mutex::new(None),
            #[cfg(feature = "persist")]
            persist,
        });

        // Updater thread.
        {
            let inner_clone = Arc::clone(&inner);
            let handle = thread::Builder::new()
                .name("playa-jobs/updater".into())
                .spawn(move || updater_loop(inner_clone, update_rx))
                .expect("spawn updater");
            *inner.updater.lock().unwrap() = Some(handle);
        }

        // Worker threads.
        for i in 0..config.thread_count {
            let inner_clone = Arc::clone(&inner);
            let handle = thread::Builder::new()
                .name(format!("playa-jobs/worker-{i}"))
                .spawn(move || worker_loop(inner_clone))
                .expect("spawn worker");
            inner.workers.lock().unwrap().push(handle);
        }

        Ok(Self { inner })
    }

    pub fn register_provider<P: JobProvider>(&self, provider: P) {
        let kind = provider.kind().to_string();
        self.inner
            .providers
            .write()
            .unwrap()
            .insert(kind, Arc::new(provider));
    }

    /// Subscribe to [`JobEvent`]s. Listener is invoked synchronously from the
    /// updater thread — keep it cheap (push into a queue, request_repaint, …).
    pub fn subscribe<F>(&self, listener: F)
    where
        F: Fn(JobEvent) + Send + Sync + 'static,
    {
        self.inner.listeners.write().unwrap().push(Box::new(listener));
    }

    pub fn submit(
        &self,
        kind: impl Into<String>,
        params: serde_json::Value,
    ) -> Result<JobId, JobError> {
        let kind = kind.into();
        if !self.inner.providers.read().unwrap().contains_key(&kind) {
            return Err(JobError::UnknownProvider(kind));
        }
        let job = Job::new(kind, params);
        let id = job.id;
        let cancel = CancelToken::new();

        #[cfg(feature = "persist")]
        if let Some(p) = &self.inner.persist {
            let _ = p.append(&LogEntry::Created(job.clone()));
        }

        self.inner.jobs.write().unwrap().insert(id, job);
        self.inner
            .cancel_tokens
            .write()
            .unwrap()
            .insert(id, cancel);
        self.inner.broadcast(JobEvent::Created(id));

        // Push onto the work queue and wake one worker.
        let (lock, cvar) = &self.inner.work_queue;
        lock.lock().unwrap().push_back(id);
        cvar.notify_one();

        Ok(id)
    }

    /// Cancel an in-flight job. No-op if `id` is unknown or already terminal.
    pub fn cancel(&self, id: JobId) {
        if let Some(token) = self.inner.cancel_tokens.read().unwrap().get(&id) {
            token.cancel();
        }
    }

    pub fn get(&self, id: JobId) -> Option<Job> {
        self.inner.jobs.read().unwrap().get(&id).cloned()
    }

    pub fn list(&self) -> Vec<Job> {
        self.inner.jobs.read().unwrap().values().cloned().collect()
    }

    /// On startup: read the persistence log (if configured), restore the job
    /// map, and re-enqueue every job whose state [`JobState::is_resumable`].
    /// Caller must register providers **before** calling this so the workers
    /// find a handler when each resumed job pops.
    #[cfg(feature = "persist")]
    pub fn replay_persisted(&self) -> std::io::Result<usize> {
        let Some(persist) = &self.inner.persist else {
            return Ok(0);
        };
        let restored = PersistLog::replay_to_jobs(persist.path())?;
        let mut resumed = 0;
        let mut jobs_lock = self.inner.jobs.write().unwrap();
        let mut tokens_lock = self.inner.cancel_tokens.write().unwrap();
        let (qlock, cvar) = &self.inner.work_queue;
        let mut queue = qlock.lock().unwrap();
        for (id, job) in restored {
            let resumable = job.state.is_resumable();
            jobs_lock.insert(id, job);
            tokens_lock.insert(id, CancelToken::new());
            if resumable {
                queue.push_back(id);
                resumed += 1;
            }
        }
        // Notify enough workers to pick up resumed jobs.
        for _ in 0..resumed {
            cvar.notify_one();
        }
        Ok(resumed)
    }

    /// Block until all workers + the updater have exited. Subsequent submits
    /// after [`Self::shutdown`] are silently dropped.
    pub fn shutdown(&self) {
        if self.inner.shutdown.swap(true, Ordering::Release) {
            return;
        }
        // Wake every worker so they observe the shutdown flag.
        let (_lock, cvar) = &self.inner.work_queue;
        cvar.notify_all();
        // Tell the updater to exit.
        let _ = self.inner.update_tx.send(UpdateMsg::Shutdown);

        // Join workers.
        let workers = std::mem::take(&mut *self.inner.workers.lock().unwrap());
        for w in workers {
            let _ = w.join();
        }
        // Join updater.
        if let Some(u) = self.inner.updater.lock().unwrap().take() {
            let _ = u.join();
        }
    }
}

impl Drop for JobQueue {
    fn drop(&mut self) {
        // Best-effort: stop background threads when the last clone goes away.
        if Arc::strong_count(&self.inner) == 1 {
            self.shutdown();
        }
    }
}

impl Inner {
    fn broadcast(&self, event: JobEvent) {
        let listeners = self.listeners.read().unwrap();
        for l in listeners.iter() {
            l(event.clone());
        }
    }
}

// -----------------------------------------------------------------------------
// Worker thread loop.
// -----------------------------------------------------------------------------

fn worker_loop(inner: Arc<Inner>) {
    loop {
        let id = match wait_for_work(&inner) {
            Some(id) => id,
            None => break, // shutdown
        };

        // Snapshot: kind, params, cancel token. Then run provider outside any
        // lock so other workers can pick up the next job.
        let snapshot = {
            let jobs = inner.jobs.read().unwrap();
            jobs.get(&id).map(|j| (j.kind.clone(), j.params.clone()))
        };
        let Some((kind, params)) = snapshot else {
            continue;
        };
        let cancel = match inner.cancel_tokens.read().unwrap().get(&id).cloned() {
            Some(c) => c,
            None => CancelToken::new(),
        };

        let provider = inner.providers.read().unwrap().get(&kind).cloned();
        let Some(provider) = provider else {
            let _ = inner.update_tx.send(UpdateMsg::Final(
                id,
                Err(JobError::UnknownProvider(kind)),
            ));
            continue;
        };

        // Per-job staging directory.
        let files_dir = inner.files_dir.join(id.0.to_string());
        let _ = std::fs::create_dir_all(&files_dir);

        let ctx = JobContext {
            job_id: id,
            cancel: cancel.clone(),
            files_dir,
            update_tx: inner.update_tx.clone(),
        };

        let result = provider.run(&ctx, params);
        let _ = inner.update_tx.send(UpdateMsg::Final(id, result));
    }
}

fn wait_for_work(inner: &Inner) -> Option<JobId> {
    let (lock, cvar) = &inner.work_queue;
    let mut queue = lock.lock().unwrap();
    loop {
        if inner.shutdown.load(Ordering::Acquire) {
            return None;
        }
        if let Some(id) = queue.pop_front() {
            return Some(id);
        }
        // 250 ms tick keeps shutdown latency bounded if a notify_all races.
        let (q, _) = cvar.wait_timeout(queue, Duration::from_millis(250)).unwrap();
        queue = q;
    }
}

// -----------------------------------------------------------------------------
// Updater thread loop: applies UpdateMsg to the job map and emits events.
// -----------------------------------------------------------------------------

fn updater_loop(inner: Arc<Inner>, rx: Receiver<UpdateMsg>) {
    while let Ok(msg) = rx.recv() {
        match msg {
            UpdateMsg::State(id, state) => apply_state(&inner, id, state),
            UpdateMsg::Progress(id, progress) => {
                let mut jobs = inner.jobs.write().unwrap();
                if let Some(j) = jobs.get_mut(&id) {
                    j.progress = Some(progress.clone());
                    j.touch();
                }
                drop(jobs);
                #[cfg(feature = "persist")]
                if let Some(p) = &inner.persist {
                    let snapshot = inner.jobs.read().unwrap().get(&id).cloned();
                    if let Some(j) = snapshot {
                        let _ = p.append(&LogEntry::Updated {
                            id,
                            state: j.state,
                            progress: j.progress.clone(),
                            result: j.result.clone(),
                            error: j.error.clone(),
                            updated_at: j.updated_at,
                        });
                    }
                }
                inner.broadcast(JobEvent::Progress(id, progress));
            }
            UpdateMsg::ParamPatch(id, key, value) => {
                let mut jobs = inner.jobs.write().unwrap();
                if let Some(j) = jobs.get_mut(&id)
                    && let Some(obj) = j.params.as_object_mut()
                {
                    obj.insert(key.clone(), value.clone());
                    j.touch();
                }
                drop(jobs);
                #[cfg(feature = "persist")]
                if let Some(p) = &inner.persist {
                    let _ = p.append(&LogEntry::ParamPatch { id, key, value });
                }
            }
            UpdateMsg::Final(id, outcome) => {
                let (state, result, error) = match outcome {
                    Ok(value) => (JobState::Complete, Some(value), None),
                    Err(JobError::Cancelled) => (JobState::Cancelled, None, None),
                    Err(e) => (JobState::Failed, None, Some(e.to_string())),
                };
                {
                    let mut jobs = inner.jobs.write().unwrap();
                    if let Some(j) = jobs.get_mut(&id) {
                        j.state = state;
                        j.result = result.clone();
                        j.error = error.clone();
                        j.touch();
                    }
                }
                #[cfg(feature = "persist")]
                if let Some(p) = &inner.persist {
                    let snapshot = inner.jobs.read().unwrap().get(&id).cloned();
                    if let Some(j) = snapshot {
                        let _ = p.append(&LogEntry::Updated {
                            id,
                            state: j.state,
                            progress: j.progress.clone(),
                            result: j.result.clone(),
                            error: j.error.clone(),
                            updated_at: j.updated_at,
                        });
                    }
                }
                let event = match state {
                    JobState::Complete => {
                        JobEvent::Completed(id, result.unwrap_or(serde_json::Value::Null))
                    }
                    JobState::Cancelled => JobEvent::Cancelled(id),
                    JobState::Failed => {
                        JobEvent::Failed(id, error.unwrap_or_else(|| "unknown".into()))
                    }
                    _ => JobEvent::StateChanged(id, state),
                };
                inner.broadcast(event);
            }
            UpdateMsg::Shutdown => break,
        }
    }
}

fn apply_state(inner: &Arc<Inner>, id: JobId, state: JobState) {
    {
        let mut jobs = inner.jobs.write().unwrap();
        if let Some(j) = jobs.get_mut(&id) {
            j.state = state;
            j.updated_at = now_secs();
        }
    }
    #[cfg(feature = "persist")]
    if let Some(p) = &inner.persist {
        let snapshot = inner.jobs.read().unwrap().get(&id).cloned();
        if let Some(j) = snapshot {
            let _ = p.append(&LogEntry::Updated {
                id,
                state: j.state,
                progress: j.progress.clone(),
                result: j.result.clone(),
                error: j.error.clone(),
                updated_at: j.updated_at,
            });
        }
    }
    inner.broadcast(JobEvent::StateChanged(id, state));
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Instant;

    /// Trivial provider: returns the params back as the result.
    struct EchoProvider;

    impl JobProvider for EchoProvider {
        fn kind(&self) -> &'static str {
            "echo"
        }
        fn run(
            &self,
            ctx: &JobContext,
            params: serde_json::Value,
        ) -> Result<serde_json::Value, JobError> {
            ctx.set_state(JobState::Submitting);
            ctx.cancel.check_err()?;
            Ok(params)
        }
    }

    /// Provider that polls the cancel flag every 10 ms; used to test cancel
    /// arriving mid-run.
    struct SlowProvider;

    impl JobProvider for SlowProvider {
        fn kind(&self) -> &'static str {
            "slow"
        }
        fn run(
            &self,
            ctx: &JobContext,
            _params: serde_json::Value,
        ) -> Result<serde_json::Value, JobError> {
            ctx.set_state(JobState::AwaitingProvider);
            for _ in 0..200 {
                ctx.cancel.check_err()?;
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(serde_json::json!({"ok": true}))
        }
    }

    fn poll_until<F: Fn() -> bool>(timeout: Duration, cond: F) -> bool {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if cond() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    fn config_no_persist() -> JobQueueConfig {
        let mut cfg = JobQueueConfig::default();
        cfg.thread_count = 2;
        cfg.files_dir = std::env::temp_dir().join(format!(
            "playa-jobs-test-{}",
            uuid::Uuid::new_v4()
        ));
        #[cfg(feature = "persist")]
        {
            cfg.persist_path = None;
        }
        cfg
    }

    #[test]
    fn unknown_provider_rejected_at_submit() {
        let q = JobQueue::new(config_no_persist()).unwrap();
        let err = q.submit("nonexistent", serde_json::json!({})).unwrap_err();
        assert!(matches!(err, JobError::UnknownProvider(_)));
        q.shutdown();
    }

    #[test]
    fn echo_round_trip_completes() {
        let q = JobQueue::new(config_no_persist()).unwrap();
        q.register_provider(EchoProvider);

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = Arc::clone(&counter);
        q.subscribe(move |ev| {
            if matches!(ev, JobEvent::Completed(_, _)) {
                counter_clone.fetch_add(1, Ordering::Relaxed);
            }
        });

        let id = q.submit("echo", serde_json::json!({"x": 42})).unwrap();
        assert!(poll_until(Duration::from_secs(2), || counter.load(Ordering::Relaxed) > 0));

        let got = q.get(id).unwrap();
        assert_eq!(got.state, JobState::Complete);
        assert_eq!(got.result, Some(serde_json::json!({"x": 42})));
        q.shutdown();
    }

    #[test]
    fn cancel_mid_run_resolves_to_cancelled() {
        let q = JobQueue::new(config_no_persist()).unwrap();
        q.register_provider(SlowProvider);

        let id = q.submit("slow", serde_json::json!({})).unwrap();
        // Let the worker reach AwaitingProvider before cancelling.
        assert!(poll_until(Duration::from_millis(500), || {
            q.get(id)
                .map(|j| j.state == JobState::AwaitingProvider)
                .unwrap_or(false)
        }));
        q.cancel(id);
        assert!(poll_until(Duration::from_secs(2), || {
            q.get(id)
                .map(|j| j.state == JobState::Cancelled)
                .unwrap_or(false)
        }));
        q.shutdown();
    }

    #[test]
    fn list_returns_every_job() {
        let q = JobQueue::new(config_no_persist()).unwrap();
        q.register_provider(EchoProvider);

        for i in 0..3 {
            q.submit("echo", serde_json::json!({"i": i})).unwrap();
        }
        // Wait a bit for completion.
        assert!(poll_until(Duration::from_secs(2), || {
            q.list().iter().filter(|j| j.state == JobState::Complete).count() == 3
        }));
        assert_eq!(q.list().len(), 3);
        q.shutdown();
    }

    #[test]
    fn shutdown_unblocks_idle_workers_quickly() {
        let q = JobQueue::new(config_no_persist()).unwrap();
        q.register_provider(EchoProvider);
        let started = Instant::now();
        q.shutdown();
        // Sub-second is plenty: workers wake on Condvar notify_all + shutdown
        // flag observation.
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[cfg(feature = "persist")]
    #[test]
    fn persist_replay_restores_completed_job() {
        let log_path = std::env::temp_dir().join(format!(
            "playa-jobs-replay-{}.jsonl",
            uuid::Uuid::new_v4()
        ));
        let _ = std::fs::remove_file(&log_path);

        // Phase 1: submit, run, complete, persist.
        let id = {
            let cfg = JobQueueConfig {
                thread_count: 1,
                files_dir: std::env::temp_dir().join("playa-jobs-replay-files"),
                persist_path: Some(log_path.clone()),
            };
            let q = JobQueue::new(cfg).unwrap();
            q.register_provider(EchoProvider);
            let id = q.submit("echo", serde_json::json!({"v": 1})).unwrap();
            assert!(poll_until(Duration::from_secs(2), || q
                .get(id)
                .map(|j| j.state == JobState::Complete)
                .unwrap_or(false)));
            q.shutdown();
            id
        };

        // Phase 2: fresh queue, replay log, verify state.
        let cfg = JobQueueConfig {
            thread_count: 1,
            files_dir: std::env::temp_dir().join("playa-jobs-replay-files"),
            persist_path: Some(log_path.clone()),
        };
        let q = JobQueue::new(cfg).unwrap();
        let resumed = q.replay_persisted().unwrap();
        // Completed jobs are not resumable.
        assert_eq!(resumed, 0);
        let restored = q.get(id).expect("job restored");
        assert_eq!(restored.state, JobState::Complete);
        q.shutdown();

        let _ = std::fs::remove_file(&log_path);
    }
}
