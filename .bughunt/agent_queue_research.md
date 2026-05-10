# Async / Queue Manager Research

Repo: `playa` (Rust workspace, eframe/egui + wgpu + crossbeam + rayon + std::thread).
**Tokio is NOT in the dep tree.** No `reqwest`/`hyper`/`futures-executor`. The `futures-*` crates in `Cargo.lock` come transitively (wgpu/wasm only). Async runtime would have to be added.

---

## Existing infrastructure inventory

| File:line | Pattern | Purpose | Lifecycle |
|---|---|---|---|
| `crates/playa-engine/src/core/workers.rs:34-133` | `Workers` work-stealing pool (`crossbeam::deque::{Injector, Worker}` + N OS threads, FIFO local deques, `AtomicU64` epoch) | Generic `Box<dyn FnOnce() + Send>` jobs. Frame loads + composing. Epoch cancel for scrub | Created in `PlayaApp` init, dropped on app exit; `Drop` signals `AtomicBool` shutdown w/ 500ms join timeout (`workers.rs:194-226`) |
| `crates/playa-engine/src/core/workers.rs:150-191` | `Workers::execute` / `execute_with_epoch` | Public enqueue API. Epoch-wrapped closure self-skips if epoch advanced before pickup | Fire-and-forget, no JobId, no result handle, no progress |
| `crates/playa-engine/src/entities/traits.rs:77` | `WorkerPool` trait | Single abstraction, `execute_with_epoch(epoch, Box<dyn FnOnce()>)` | Used by frame loader; that's it |
| `crates/playa-engine/src/core/cache_man.rs:32` (epoch field), `:80` "cancel all preload" | Epoch counter on `CacheManager` | `bump_epoch()` invalidates all pending preloads in one atomic store | App-lifetime |
| `crates/playa-engine/src/core/debounced_preloader.rs:27-108` | `DebouncedPreloader` (no thread, polled per-frame) | Coalesces scrubbing into single delayed preload. `tick()` returns `Option<Uuid>` when `Instant::now() >= trigger_at` | Cleared on cancel/tick |
| `crates/playa-engine/src/entities/frame.rs:84-105` | `FrameStatus { Placeholder, Header, Loading, Composing, Expired, Loaded, Error }` | Per-frame state machine, mutated by worker via `Mutex<FrameData>` | Per-frame, cache-managed |
| `crates/playa-engine/src/entities/gpu_blend_bridge.rs:29-160` | `GpuBlendBridge` worker→UI offload, `std::sync::mpsc::channel` + per-request reply channel | Worker enqueues `GpuBlendRequest`, blocks on `reply_rx.recv()`; UI drains in `update()` | Pair created at app boot; receiver lives on UI side |
| `crates/playa-engine/src/entities/gpu_blend_bridge.rs:131-153` | `drain_into_compositor` try_recv loop per UI frame | Batched UI-thread completion of worker offloads | Per-frame UI tick |
| `crates/playa-app/src/server/api.rs:27, 197-209` | REST API: `rouille::Server` + `mpsc::Sender<ApiCommand>` + `Arc<RwLock<SharedApiState>>` | HTTP thread → main thread command channel; main writes snapshots back | Server thread spawned in `ApiServer::start`, never joined |
| `crates/playa-app/src/server/api.rs:62-67`, `:432-451` | Screenshot one-shot: `crossbeam::bounded(1)` reply channel + `ctx.request_repaint()` to force frame | Cross-thread synchronous screenshot via UI rerender | One-shot per request |
| `crates/playa-ui/src/dialogs/encode/encode_ui.rs:42, 198, 720, 739, 762` | Per-encode `Arc<AtomicBool> cancel_flag` + `mpsc::channel<EncodeProgress>` + raw `thread::spawn` | One thread per export; progress polled in UI render via `rx.try_recv()` | Stop = set flag + push handle to `orphan_handles` reaped each tick (`encode_ui.rs:794-820`) |
| `crates/playa-ui/src/dialogs/encode/encode.rs:1135-1153` | `EncodeProgress { current_frame, total_frames, stage }`, `EncodeStage { Validating, Opening, Encoding, Flushing, Complete, Error(String) }` | Closest existing thing to a job state machine — but local to encode dialog, no JobId, no registry | Per-encode |
| `crates/playa-engine/src/entities/transform.rs:23, 358` | `rayon::par_iter` for pixel work | Data-parallel inside one job, not job-level | Per-call |
| `crates/playa-events/src/bus.rs:52-191` | `EventBus` — `Arc<RwLock<HashMap<TypeId, Vec<Callback>>>>` + `Arc<Mutex<VecDeque<BoxedEvent>>>`, MAX 1000, immediate callbacks + deferred `poll()` | Decoupled pub/sub, `Send + Sync` events, eviction on overflow | App-lifetime singleton |
| `crates/playa-app/src/app/mod.rs:167-292`, `app/run.rs:316, 426` | `api_command_rx: Option<mpsc::Receiver<ApiCommand>>` drained in `update`, `request_repaint()` on completion | Bridge HTTP thread → eframe main loop | Per-frame poll |

**Persistence reality:**
`crates/playa-app/src/runner.rs:99-107` — eframe `persistence: true` + `persistence_path: playa.json` (window only). `Project` JSON via `entities/project.rs:502` (`serde_json::to_string_pretty`). **No job-state file. No DB.**

---

## What's missing for long external tasks

- No persistent `Job` model with id, kind, state machine, result, error, started_at, owner.
- No way to enqueue **and later query** a task — `Workers::execute` returns `()`.
- No HTTP client. No `reqwest` / `ureq` / `hyper` in deps. Every provider would bring its own.
- No async runtime — long polling external API in `Workers` thread blocks one of N pool threads forever (pool sized for CPU-bound frame loads, not minutes-long IO waits). Mixing minute-scale jobs into the same pool **starves frame decoding** during scrub.
- No cancellation token primitive. `cancel_flag: Arc<AtomicBool>` is duplicated ad-hoc in encode_ui (`encode_ui.rs:42`, `:809` resets per run).
- No progress channel abstraction. Encode reinvents `mpsc::channel<EncodeProgress>` locally; nothing reusable.
- No persistence. Restart loses every in-flight job — fatal for jobs that already cost API credits (Seedance submitted, mp4 not yet downloaded).
- No cross-job notification. `EventBus` exists in `playa-events` but is **not currently used by background workers** to publish completion (gpu_blend_bridge ignores it, encode uses its own mpsc).
- No global registry/UI surface for "all jobs in flight" — only one encode dialog at a time.

---

## Crate boundary proposal

- **Name:** `playa-jobs` (sibling to `playa-events`, `playa-engine`).
- **Public types:**
  - `JobId(Uuid)`
  - `Job { id, kind: String, state: JobState, progress: Option<JobProgress>, error: Option<String>, params: serde_json::Value, created_at, updated_at }` — `Serialize + Deserialize` so it round-trips JSON.
  - `JobState { Pending, Submitting, AwaitingProvider, Downloading, Staging, Complete, Failed, Cancelled }` — exact set requested.
  - `JobProgress { stage: String, fraction: Option<f32>, msg: Option<String> }`
  - `trait JobProvider: Send + Sync + 'static { fn kind(&self) -> &'static str; fn run(&self, ctx: &JobContext, params: Value) -> Result<Value, JobError>; fn cancel_token(&self) -> CancelToken; }`
  - `JobContext { update: Box<dyn Fn(JobProgress) + Send>, cancel: CancelToken, http: Arc<HttpClient>, files_dir: PathBuf }`
  - `JobQueue { submit(kind, params) -> JobId, cancel(JobId), get(JobId), list(filter) -> Vec<Job>, subscribe() -> Receiver<JobEvent> }`
  - `JobEvent { Created(JobId), StateChanged(JobId, JobState), Progress(JobId, JobProgress), Completed(JobId, Value), Failed(JobId, String) }` — **emitted via `playa-events::EventBus`** so existing UI subscription mechanism just works.
- **Thread model:**
  - **Dedicated IO worker pool** separate from `Workers` (the engine's CPU pool). Jobs are minute-scale, must not occupy frame-load threads.
  - Pool size: small (`max(2, num_cpus/4)`) since each thread is mostly idle waiting on HTTP.
  - **Rationale:** matches existing repo style (raw threads + crossbeam, no tokio anywhere). Adding tokio would pull a runtime, contaminate every existing sync path, and force `Send` rewrites of the event bus and worker pool. Not worth it for ≤dozens of concurrent slow jobs.
- **Runtime choice:** **No tokio.** Use blocking `ureq` (tiny, sync, no runtime) for HTTP. Provider implementations are plain `fn run(...)`. Polling Seedance = `loop { sleep(15s); ureq::get(status_url); if cancel.cancelled() return Cancelled }`.
- **Persistence:** SQLite via `rusqlite` (already a small dep) OR — simpler for v1 — newline-delimited JSON at `~/.config/playa/jobs.jsonl` (append-only log + tombstones, replay on boot). Keep it behind a `persist` cargo feature. Default ON. Resume policy: jobs in `AwaitingProvider` or `Downloading` re-enter the queue at boot; provider impl is responsible for reconstructing from `params` (e.g. Seedance `task_id` lives in `params.task_id` after `Submitting → AwaitingProvider`).

---

## Provider model

```rust
pub trait JobProvider: Send + Sync + 'static {
    fn kind(&self) -> &'static str;          // "seedance.video", "ffmpeg.encode"
    fn run(
        &self,
        ctx: &JobContext,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, JobError>;
    /// Optional: hook for cleanup if the queue is restarted mid-flight.
    fn resume(&self, _ctx: &JobContext, _params: serde_json::Value) -> Result<serde_json::Value, JobError> {
        // default: re-run from scratch
        self.run(_ctx, _params)
    }
}
```

Seedance provider sketch:

```rust
struct SeedanceProvider { api_key: String, http: Arc<ureq::Agent> }

impl JobProvider for SeedanceProvider {
    fn kind(&self) -> &'static str { "seedance.video" }
    fn run(&self, ctx: &JobContext, params: Value) -> Result<Value, JobError> {
        ctx.set_state(JobState::Submitting);
        let task_id = self.http.post(SEED_URL).send_json(&params)?.into_json::<Resp>()?.task_id;
        ctx.persist_param("task_id", &task_id);            // crash-resume hook
        ctx.set_state(JobState::AwaitingProvider);
        loop {
            if ctx.cancel.is_cancelled() { return Err(JobError::Cancelled); }
            std::thread::sleep(Duration::from_secs(15));
            let s = self.http.get(&status_url(&task_id)).call()?.into_json::<Status>()?;
            ctx.update(JobProgress { stage: "polling".into(), fraction: Some(s.pct), msg: None });
            if s.done { break Ok(s.video_url) }?;
        }
        ctx.set_state(JobState::Downloading);
        let path = ctx.files_dir.join(format!("{}.mp4", ctx.job_id));
        download_to(&self.http, &video_url, &path, &ctx.cancel, &ctx.update)?;
        ctx.set_state(JobState::Staging);
        Ok(json!({ "mp4_path": path }))
    }
}
```

---

## UI integration

- **Subscribe via `EventBus`** (already in `playa-events/src/bus.rs:52`). `JobQueue` calls `event_bus.emit(JobEvent::StateChanged(...))` etc. UI components subscribe in their constructor; callbacks are invoked synchronously from job thread (bus is `Send + Sync`), they push into a local `Arc<Mutex<JobsView>>` and call `egui::Context::request_repaint()`.
- **No per-frame polling** — repaint is only triggered when something changes (matches existing pattern at `app/run.rs:316, 426` and `encode_ui.rs:347-348` "request_repaint while encoding"). For the rare case of polling-based UI panels, expose `JobQueue::list(filter)` cheap snapshot.
- **Layer-attach completion:** the final `JobState::Staging` step emits a domain event like `VideoJobCompleted { job_id, mp4_path, target_layer: Option<Uuid> }` via `EventBus`. Existing engine code (e.g. `Project::insert_layer`) subscribes once and applies the file to the target slot on the UI thread (avoids cross-thread compositor mutations — same pattern as `gpu_blend_bridge.rs`).
- **Cancellation:** `JobQueue::cancel(id)` flips a `CancelToken` (`Arc<AtomicBool>` like the encode one at `encode_ui.rs:42`, but owned by queue, not duplicated per-feature). Provider polls `ctx.cancel.is_cancelled()` between long ops.

---

## Risks / open questions

- **HTTP TLS in static Windows build.** vcpkg/MSVC + ureq+rustls works; ureq+native-tls hits OpenSSL build pain. Pick `ureq` with `rustls` feature.
- **Provider crash-resume contract.** Seedance task_id must be persisted **before** `Submitting → AwaitingProvider` transition or restart leaks credits. Persistence write must be sync within `set_state`.
- **Job-files directory lifecycle.** Where do downloaded mp4s land before "Staging"? Propose `~/.cache/playa/jobs/{job_id}/`. GC policy on `Complete` once attached: keep until job purged; `Cancelled/Failed`: delete on next startup.
- **Provider isolation vs registry.** Putting `SeedanceProvider` directly in `playa-jobs` couples job crate to vendor APIs. Better: `playa-jobs` exposes only `JobProvider` trait + queue; provider crates (`playa-job-seedance`, `playa-job-ffmpeg`) implement and register at app boot in `playa-app`.
- **EventBus capacity.** `MAX_QUEUE_SIZE = 1000` (`bus.rs:19`) — fine if UI polls every frame. If UI is hidden/minimized for long, queue evicts. Job state should be reconstructable from `JobQueue::get(id)` snapshot, never solely from event stream.
- **Existing `Workers` pool reuse?** Tempting but **don't** — IO-bound minute-long jobs would block CPU pool threads sized for frame decoding. Separate pool, separate epoch (or no epoch — Seedance jobs aren't auto-stale on scrub).
- **Tokio vs threads, revisit only if N grows.** `≤32` concurrent slow jobs on threads is fine. If providers ever need WebSocket / streaming / >100 concurrent jobs, reconsider — but that's a future-only concern.
