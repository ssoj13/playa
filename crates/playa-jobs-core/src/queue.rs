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

use playa_events::EventBus;

use crate::cancel::CancelToken;
use crate::event::JobEvent;
use crate::job::{Job, JobError, JobId, JobState, now_secs};
#[cfg(feature = "persist")]
use crate::persist::{Log as PersistLog, LogEntry};
use crate::provider::{JobContext, JobProvider, UpdateMsg};

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

/// Aggregate statistics returned by [`JobQueue::stats`].
#[derive(Debug, Clone, Default)]
pub struct JobStats {
    pub by_state: HashMap<JobState, usize>,
    pub total_cost_usd: f64,
    pub today_cost_usd: f64,
    pub today_completed: usize,
    pub queue_depth: usize,
    pub active_providers: usize,
}

/// Filter passed to [`JobQueue::list_filtered`]. All fields combine with AND.
#[derive(Debug, Clone, Default)]
pub struct JobFilter {
    /// Allowed states. `None` = any.
    pub state: Option<Vec<JobState>>,
    /// `j.kind.starts_with(prefix)` — matches both Seedance endpoints with
    /// `"seedance."` for example.
    pub kind_prefix: Option<String>,
    /// Case-insensitive substring search across job id, kind, params.prompt
    /// (when present), and error string.
    pub search: Option<String>,
    /// Drop jobs created before this Unix-seconds timestamp.
    pub since: Option<u64>,
}

#[derive(Clone)]
pub struct JobQueue {
    inner: Arc<Inner>,
}

struct Inner {
    jobs: RwLock<HashMap<JobId, Job>>,
    cancel_tokens: RwLock<HashMap<JobId, CancelToken>>,
    providers: RwLock<HashMap<String, Arc<dyn JobProvider>>>,
    /// Canonical event sink — required (not optional). All [`JobEvent`]s
    /// flow through here. Consumers subscribe via
    /// `event_bus.subscribe::<JobEvent, _>(...)`. Removing the legacy
    /// `JobQueue::subscribe()` public API forces callers to use the unified
    /// pub/sub system; no ad-hoc closures.
    event_bus: Arc<EventBus>,
    files_dir: PathBuf,
    /// Daily USD cap. `None` = enforcement disabled (default). When set,
    /// [`JobQueue::submit`] rejects requests whose
    /// `today_cost_usd + provider.estimate_cost_usd(&params)` would exceed
    /// the cap. The host (e.g. PlayaApp) writes this from its persisted
    /// [`crate::JobsSettings`] each frame.
    budget_cap: RwLock<Option<f64>>,
    work_queue: (Mutex<VecDeque<JobId>>, Condvar),
    update_tx: Sender<UpdateMsg>,
    shutdown: AtomicBool,
    workers: Mutex<Vec<JoinHandle<()>>>,
    updater: Mutex<Option<JoinHandle<()>>>,
    #[cfg(feature = "persist")]
    persist: Option<Arc<PersistLog>>,
}

impl JobQueue {
    /// Construct a new job queue. Caller passes the application's shared
    /// [`EventBus`]; all [`JobEvent`]s emitted by the queue flow through
    /// it. UI consumers subscribe via
    /// `event_bus.subscribe::<JobEvent, _>(|ev| ...)`.
    pub fn new(config: JobQueueConfig, event_bus: Arc<EventBus>) -> std::io::Result<Self> {
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
            event_bus,
            files_dir: config.files_dir,
            budget_cap: RwLock::new(None),
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

    /// Access the underlying [`EventBus`] passed to [`Self::new`].
    /// Consumers subscribe to [`JobEvent`]s through this:
    /// `queue.event_bus().subscribe::<JobEvent, _>(|ev| ...)`.
    pub fn event_bus(&self) -> &Arc<EventBus> {
        &self.inner.event_bus
    }

    /// Drop a TERMINAL job from the in-memory map. Persists a
    /// [`LogEntry::Tombstone`] so replay does not resurrect it. Errors with
    /// [`JobError::Provider`] when the id is unknown or non-terminal.
    pub fn remove(&self, id: JobId) -> Result<(), JobError> {
        let mut jobs = self.inner.jobs.write().unwrap();
        let Some(job) = jobs.get(&id) else {
            return Err(JobError::Provider(format!("job {id} not found")));
        };
        if !job.state.is_terminal() {
            return Err(JobError::Provider(format!(
                "cannot remove non-terminal job {id} (state={:?})",
                job.state
            )));
        }
        jobs.remove(&id);
        drop(jobs);
        self.inner.cancel_tokens.write().unwrap().remove(&id);
        #[cfg(feature = "persist")]
        if let Some(p) = &self.inner.persist {
            let _ = p.append(&LogEntry::Tombstone(id));
        }
        Ok(())
    }

    /// Re-submit a TERMINAL Failed/Cancelled job with the same kind +
    /// params. Returns the **new** [`JobId`]; the original stays in the
    /// list for history. Errors with [`JobError::Provider`] when the id is
    /// unknown or the state is not retryable (Complete, or any
    /// non-terminal state).
    pub fn retry(&self, id: JobId) -> Result<JobId, JobError> {
        let (kind, params) = {
            let jobs = self.inner.jobs.read().unwrap();
            let Some(job) = jobs.get(&id) else {
                return Err(JobError::Provider(format!("job {id} not found")));
            };
            if !matches!(job.state, JobState::Failed | JobState::Cancelled) {
                return Err(JobError::Provider(format!(
                    "can only retry Failed or Cancelled jobs (state={:?})",
                    job.state
                )));
            }
            (job.kind.clone(), job.params.clone())
        };
        self.submit(kind, params)
    }

    /// Cheap aggregate read for status-bar / footer / budget enforcement.
    /// Walks the job map once; constant memory.
    pub fn stats(&self) -> JobStats {
        let jobs = self.inner.jobs.read().unwrap();
        let providers = self.inner.providers.read().unwrap();
        let now = now_secs();
        let day_start = now - (now % 86_400);

        let mut s = JobStats {
            active_providers: providers.len(),
            ..JobStats::default()
        };
        for j in jobs.values() {
            *s.by_state.entry(j.state).or_default() += 1;
            if !j.state.is_terminal() {
                s.queue_depth += 1;
            }
            if let Some(c) = j.cost_usd {
                s.total_cost_usd += c;
                if j.updated_at >= day_start {
                    s.today_cost_usd += c;
                }
            }
            if matches!(j.state, JobState::Complete) && j.updated_at >= day_start {
                s.today_completed += 1;
            }
        }
        s
    }

    /// Filter the in-memory job map. Returned vector is a clone of matching
    /// jobs (callers must not block writers).
    pub fn list_filtered(&self, filter: &JobFilter) -> Vec<Job> {
        let jobs = self.inner.jobs.read().unwrap();
        let q = filter
            .search
            .as_ref()
            .map(|s| s.to_ascii_lowercase())
            .filter(|s| !s.is_empty());
        jobs.values()
            .filter(|j| {
                if let Some(states) = &filter.state
                    && !states.contains(&j.state)
                {
                    return false;
                }
                if let Some(prefix) = &filter.kind_prefix
                    && !j.kind.starts_with(prefix)
                {
                    return false;
                }
                if let Some(since) = filter.since
                    && j.created_at < since
                {
                    return false;
                }
                if let Some(q) = &q {
                    let id = j.id.to_string().to_ascii_lowercase();
                    let kind = j.kind.to_ascii_lowercase();
                    let prompt = j
                        .params
                        .get("prompt")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_ascii_lowercase())
                        .unwrap_or_default();
                    let err = j
                        .error
                        .as_deref()
                        .map(|s| s.to_ascii_lowercase())
                        .unwrap_or_default();
                    if !id.contains(q)
                        && !kind.contains(q)
                        && !prompt.contains(q)
                        && !err.contains(q)
                    {
                        return false;
                    }
                }
                true
            })
            .cloned()
            .collect()
    }

    /// Set (or clear) the daily USD budget cap. Pass `None` to disable
    /// enforcement; pass `Some(x)` to reject submissions that would push
    /// the day's cumulative cost past `x`. Cheap atomic write — safe to
    /// call every frame from the UI thread.
    pub fn set_budget_cap(&self, cap_usd: Option<f64>) {
        *self.inner.budget_cap.write().unwrap() = cap_usd;
    }

    /// Read the active daily budget cap, if any.
    pub fn budget_cap(&self) -> Option<f64> {
        *self.inner.budget_cap.read().unwrap()
    }

    pub fn submit(
        &self,
        kind: impl Into<String>,
        params: serde_json::Value,
    ) -> Result<JobId, JobError> {
        let kind = kind.into();
        let provider = {
            let providers = self.inner.providers.read().unwrap();
            match providers.get(&kind) {
                Some(p) => Arc::clone(p),
                None => return Err(JobError::UnknownProvider(kind)),
            }
        };

        // Pre-insert budget enforcement. Compute estimate BEFORE inserting
        // the job into the map so a rejection is observable to the caller
        // and leaves no orphan in `list()`. `today_cost_usd` walks the
        // current map; the lock is dropped before the (potential) insert
        // below to avoid holding two locks at once.
        if let Some(cap) = self.budget_cap() {
            let today_spent = self.stats().today_cost_usd;
            let estimated = provider.estimate_cost_usd(&params).unwrap_or(0.0);
            if today_spent + estimated > cap {
                return Err(JobError::Provider(format!(
                    "daily budget exceeded (${today_spent:.2} spent today + ${estimated:.2} estimated > ${cap:.2} cap)"
                )));
            }
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
        self.event_bus.emit(event);
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
            UpdateMsg::Cost(id, usd) => {
                let at = now_secs();
                {
                    let mut jobs = inner.jobs.write().unwrap();
                    if let Some(j) = jobs.get_mut(&id) {
                        j.cost_usd = Some(usd);
                        j.updated_at = at;
                    }
                }
                #[cfg(feature = "persist")]
                if let Some(p) = &inner.persist {
                    let _ = p.append(&LogEntry::Cost { id, usd, at });
                }
            }
            UpdateMsg::Final(id, outcome) => {
                let (state, result, error) = match outcome {
                    Ok(value) => (JobState::Complete, Some(value), None),
                    Err(JobError::Cancelled) => (JobState::Cancelled, None, None),
                    Err(e) => (JobState::Failed, None, Some(e.to_string())),
                };
                let at = now_secs();
                {
                    let mut jobs = inner.jobs.write().unwrap();
                    if let Some(j) = jobs.get_mut(&id) {
                        j.state = state;
                        j.result = result.clone();
                        j.error = error.clone();
                        j.updated_at = at;
                        // Append the terminal transition to state_history
                        // (skip if last entry already at this state — paranoia
                        // against double-write paths).
                        if j.state_history.last().map(|(s, _)| *s) != Some(state) {
                            j.state_history.push((state, at));
                        }
                    }
                }
                #[cfg(feature = "persist")]
                if let Some(p) = &inner.persist {
                    let _ = p.append(&LogEntry::StageEntered { id, state, at });
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
    let at = now_secs();
    {
        let mut jobs = inner.jobs.write().unwrap();
        if let Some(j) = jobs.get_mut(&id) {
            j.state = state;
            j.updated_at = at;
            if j.state_history.last().map(|(s, _)| *s) != Some(state) {
                j.state_history.push((state, at));
            }
        }
    }
    #[cfg(feature = "persist")]
    if let Some(p) = &inner.persist {
        // Compact stage-transition entry — replay folds into state_history.
        let _ = p.append(&LogEntry::StageEntered { id, state, at });
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

    /// Construct a queue + its EventBus together so test sites can subscribe.
    fn make_queue(cfg: JobQueueConfig) -> (JobQueue, Arc<EventBus>) {
        let bus = Arc::new(EventBus::new());
        let q = JobQueue::new(cfg, Arc::clone(&bus)).unwrap();
        (q, bus)
    }

    #[test]
    fn unknown_provider_rejected_at_submit() {
        let (q, _bus) = make_queue(config_no_persist());
        let err = q.submit("nonexistent", serde_json::json!({})).unwrap_err();
        assert!(matches!(err, JobError::UnknownProvider(_)));
        q.shutdown();
    }

    #[test]
    fn echo_round_trip_completes() {
        let (q, bus) = make_queue(config_no_persist());
        q.register_provider(EchoProvider);

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = Arc::clone(&counter);
        bus.subscribe::<JobEvent, _>(move |ev| {
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
        let (q, _bus) = make_queue(config_no_persist());
        q.register_provider(SlowProvider);

        let id = q.submit("slow", serde_json::json!({})).unwrap();
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
        let (q, _bus) = make_queue(config_no_persist());
        q.register_provider(EchoProvider);

        for i in 0..3 {
            q.submit("echo", serde_json::json!({"i": i})).unwrap();
        }
        assert!(poll_until(Duration::from_secs(2), || {
            q.list().iter().filter(|j| j.state == JobState::Complete).count() == 3
        }));
        assert_eq!(q.list().len(), 3);
        q.shutdown();
    }

    #[test]
    fn shutdown_unblocks_idle_workers_quickly() {
        let (q, _bus) = make_queue(config_no_persist());
        q.register_provider(EchoProvider);
        let started = Instant::now();
        q.shutdown();
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn state_history_appends_on_each_transition() {
        let (q, _bus) = make_queue(config_no_persist());
        q.register_provider(EchoProvider);
        let id = q.submit("echo", serde_json::json!({})).unwrap();
        assert!(poll_until(Duration::from_secs(2), || q
            .get(id)
            .map(|j| j.state == JobState::Complete)
            .unwrap_or(false)));
        let job = q.get(id).unwrap();
        let states: Vec<JobState> = job.state_history.iter().map(|(s, _)| *s).collect();
        // Always starts with Pending (Job::new), ends with Complete via Final.
        assert_eq!(states.first(), Some(&JobState::Pending));
        assert_eq!(states.last(), Some(&JobState::Complete));
        // EchoProvider hits Submitting between.
        assert!(states.contains(&JobState::Submitting));
        // Timestamps monotonic non-decreasing.
        let times: Vec<u64> = job.state_history.iter().map(|(_, t)| *t).collect();
        for w in times.windows(2) {
            assert!(w[0] <= w[1], "state_history timestamps must be non-decreasing");
        }
        q.shutdown();
    }

    #[test]
    fn cost_usd_populated_via_report_cost() {
        struct CostReporter;
        impl JobProvider for CostReporter {
            fn kind(&self) -> &'static str {
                "test.cost"
            }
            fn run(
                &self,
                ctx: &JobContext,
                _params: serde_json::Value,
            ) -> Result<serde_json::Value, JobError> {
                ctx.report_cost(1.21);
                Ok(serde_json::json!({}))
            }
        }
        let (q, _bus) = make_queue(config_no_persist());
        q.register_provider(CostReporter);
        let id = q.submit("test.cost", serde_json::json!({})).unwrap();
        assert!(poll_until(Duration::from_secs(2), || q
            .get(id)
            .map(|j| j.state == JobState::Complete)
            .unwrap_or(false)));
        let job = q.get(id).unwrap();
        assert_eq!(job.cost_usd, Some(1.21));
        q.shutdown();
    }

    #[test]
    fn remove_rejects_non_terminal() {
        struct StuckProvider;
        impl JobProvider for StuckProvider {
            fn kind(&self) -> &'static str {
                "test.stuck"
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
                Ok(serde_json::json!({}))
            }
        }
        let (q, _bus) = make_queue(config_no_persist());
        q.register_provider(StuckProvider);
        let id = q.submit("test.stuck", serde_json::json!({})).unwrap();
        assert!(poll_until(Duration::from_millis(500), || q
            .get(id)
            .map(|j| j.state == JobState::AwaitingProvider)
            .unwrap_or(false)));
        let err = q.remove(id).unwrap_err();
        assert!(format!("{err}").contains("non-terminal"));
        q.cancel(id);
        q.shutdown();
    }

    #[test]
    fn remove_succeeds_on_terminal_job() {
        let (q, _bus) = make_queue(config_no_persist());
        q.register_provider(EchoProvider);
        let id = q.submit("echo", serde_json::json!({})).unwrap();
        assert!(poll_until(Duration::from_secs(2), || q
            .get(id)
            .map(|j| j.state == JobState::Complete)
            .unwrap_or(false)));
        q.remove(id).unwrap();
        assert!(q.get(id).is_none());
        q.shutdown();
    }

    #[test]
    fn retry_rejects_non_failed_cancelled() {
        let (q, _bus) = make_queue(config_no_persist());
        q.register_provider(EchoProvider);
        let id = q.submit("echo", serde_json::json!({})).unwrap();
        assert!(poll_until(Duration::from_secs(2), || q
            .get(id)
            .map(|j| j.state == JobState::Complete)
            .unwrap_or(false)));
        let err = q.retry(id).unwrap_err();
        assert!(format!("{err}").contains("can only retry Failed or Cancelled"));
        q.shutdown();
    }

    #[test]
    fn retry_returns_new_id_with_same_params() {
        struct FailingProvider;
        impl JobProvider for FailingProvider {
            fn kind(&self) -> &'static str {
                "test.fails"
            }
            fn run(
                &self,
                _ctx: &JobContext,
                _params: serde_json::Value,
            ) -> Result<serde_json::Value, JobError> {
                Err(JobError::Provider("boom".into()))
            }
        }
        let (q, _bus) = make_queue(config_no_persist());
        q.register_provider(FailingProvider);
        let id = q.submit("test.fails", serde_json::json!({"x": 1})).unwrap();
        assert!(poll_until(Duration::from_secs(2), || q
            .get(id)
            .map(|j| j.state == JobState::Failed)
            .unwrap_or(false)));
        let new_id = q.retry(id).unwrap();
        assert_ne!(new_id, id);
        assert_eq!(q.get(new_id).unwrap().params, serde_json::json!({"x": 1}));
        // Original kept for history.
        assert!(q.get(id).is_some());
        q.shutdown();
    }

    #[test]
    fn stats_counts_by_state_and_active_providers() {
        let (q, _bus) = make_queue(config_no_persist());
        q.register_provider(EchoProvider);
        for i in 0..3 {
            q.submit("echo", serde_json::json!({"i": i})).unwrap();
        }
        assert!(poll_until(Duration::from_secs(2), || {
            q.list().iter().filter(|j| j.state == JobState::Complete).count() == 3
        }));
        let s = q.stats();
        assert_eq!(s.by_state.get(&JobState::Complete).copied(), Some(3));
        assert_eq!(s.queue_depth, 0);
        assert_eq!(s.active_providers, 1);
        q.shutdown();
    }

    #[test]
    fn list_filtered_respects_kind_prefix_and_search() {
        let (q, _bus) = make_queue(config_no_persist());
        q.register_provider(EchoProvider);
        q.submit("echo", serde_json::json!({"prompt": "alpha"})).unwrap();
        q.submit("echo", serde_json::json!({"prompt": "beta"})).unwrap();
        // Wait for both to complete so list has stable contents.
        assert!(poll_until(Duration::from_secs(2), || {
            q.list().iter().filter(|j| j.state == JobState::Complete).count() == 2
        }));

        let alpha_only = q.list_filtered(&JobFilter {
            search: Some("alpha".into()),
            ..Default::default()
        });
        assert_eq!(alpha_only.len(), 1);

        let kind_match = q.list_filtered(&JobFilter {
            kind_prefix: Some("echo".into()),
            ..Default::default()
        });
        assert_eq!(kind_match.len(), 2);

        let kind_miss = q.list_filtered(&JobFilter {
            kind_prefix: Some("seedance.".into()),
            ..Default::default()
        });
        assert_eq!(kind_miss.len(), 0);

        let state_filter = q.list_filtered(&JobFilter {
            state: Some(vec![JobState::Failed]),
            ..Default::default()
        });
        assert_eq!(state_filter.len(), 0);
        q.shutdown();
    }

    #[test]
    fn event_bus_accessor_returns_same_handle() {
        let (q, bus) = make_queue(config_no_persist());
        // event_bus() returns a borrowed Arc — same underlying allocation as
        // the one passed to new().
        assert!(Arc::ptr_eq(q.event_bus(), &bus));
        q.shutdown();
    }

    /// Provider that reports a fixed estimate per submit. Used to drive the
    /// budget gate without exercising the actual run loop.
    struct EstimatingProvider(f64);
    impl JobProvider for EstimatingProvider {
        fn kind(&self) -> &'static str {
            "test.estimate"
        }
        fn run(
            &self,
            _ctx: &JobContext,
            params: serde_json::Value,
        ) -> Result<serde_json::Value, JobError> {
            Ok(params)
        }
        fn estimate_cost_usd(&self, _params: &serde_json::Value) -> Option<f64> {
            Some(self.0)
        }
    }

    #[test]
    fn submit_rejected_when_estimate_would_exceed_budget_cap() {
        let (q, _bus) = make_queue(config_no_persist());
        q.register_provider(EstimatingProvider(2.0));
        q.set_budget_cap(Some(1.0));
        let err = q
            .submit("test.estimate", serde_json::json!({}))
            .unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("daily budget exceeded"), "got: {msg}");
        // Reject must not insert a tombstone-orphan into the map.
        assert_eq!(q.list().len(), 0);
        q.shutdown();
    }

    #[test]
    fn submit_succeeds_when_budget_disabled_even_with_high_estimate() {
        let (q, _bus) = make_queue(config_no_persist());
        q.register_provider(EstimatingProvider(99999.0));
        // Default cap is None (disabled).
        assert!(q.budget_cap().is_none());
        q.submit("test.estimate", serde_json::json!({})).unwrap();
        q.shutdown();
    }

    #[test]
    fn submit_succeeds_when_provider_has_no_estimate_impl() {
        // EchoProvider does NOT override estimate_cost_usd — default returns
        // None. With a cap set, that should be treated as $0 against the
        // cap (favouring submit over false rejection).
        let (q, _bus) = make_queue(config_no_persist());
        q.register_provider(EchoProvider);
        q.set_budget_cap(Some(0.01));
        q.submit("echo", serde_json::json!({})).unwrap();
        q.shutdown();
    }

    #[cfg(feature = "persist")]
    #[test]
    fn persist_replay_restores_completed_job() {
        let log_path = std::env::temp_dir().join(format!(
            "playa-jobs-replay-{}.jsonl",
            uuid::Uuid::new_v4()
        ));
        let _ = std::fs::remove_file(&log_path);

        let id = {
            let cfg = JobQueueConfig {
                thread_count: 1,
                files_dir: std::env::temp_dir().join("playa-jobs-replay-files"),
                persist_path: Some(log_path.clone()),
            };
            let (q, _bus) = make_queue(cfg);
            q.register_provider(EchoProvider);
            let id = q.submit("echo", serde_json::json!({"v": 1})).unwrap();
            assert!(poll_until(Duration::from_secs(2), || q
                .get(id)
                .map(|j| j.state == JobState::Complete)
                .unwrap_or(false)));
            q.shutdown();
            id
        };

        let cfg = JobQueueConfig {
            thread_count: 1,
            files_dir: std::env::temp_dir().join("playa-jobs-replay-files"),
            persist_path: Some(log_path.clone()),
        };
        let (q, _bus) = make_queue(cfg);
        let resumed = q.replay_persisted().unwrap();
        assert_eq!(resumed, 0);
        let restored = q.get(id).expect("job restored");
        assert_eq!(restored.state, JobState::Complete);
        q.shutdown();

        let _ = std::fs::remove_file(&log_path);
    }
}
