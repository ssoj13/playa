# playa-jobs-core

Core data model + execution engine for long-running asynchronous jobs
in playa. **Provider-agnostic** — Seedance video gen, Flux Pro inpaint,
and any future hosted-AI/encode/upload task plugs in through the
`JobProvider` trait without touching this crate.

## Why this crate exists

Modal user actions (the user clicks "Generate", a job runs for 30
seconds to 3 minutes, the result lands as a new layer) are common
enough across the codebase that we want a single execution model:
- One queue, one persistence log, one progress event stream.
- Cancelable, resumable across crash / restart.
- Daily-budget enforcement at the submit boundary.
- Cost accounting per job.

`playa-jobs-core` owns those primitives. Providers (Seedance, Inpaint,
…) live in sibling crates. The UI surface lives in `playa-jobs-ui`.
A facade `playa-jobs` ties them together for hosts that want one-line
setup.

## Module map

| File | Purpose |
|---|---|
| `lib.rs` | Re-exports + crate-level rustdoc |
| `job.rs` | `Job` struct + state machine (`Pending → Submitting → AwaitingProvider → Downloading → Staging → Complete/Failed/Cancelled`) + `JobError` taxonomy + `JobProgress` |
| `queue.rs` | `JobQueue` (worker pool + updater thread + persistent log + budget gate). `submit`, `cancel`, `remove`, `retry`, `stats`, `list_filtered` |
| `provider.rs` | `JobProvider` trait (`run`, `resume`, `estimate_cost_usd`) + `JobContext` (per-job state writes from worker threads) |
| `event.rs` | `JobEvent` taxonomy (`Created`, `Updated`, `Progress`, `Completed`, …) — emitted through the shared `EventBus` |
| `persist.rs` | JSONL log writer + replay for crash-resume. Captures `Created` / `Updated` / `StageEntered` / `Cost` / `Tombstone` |
| `settings.rs` | `JobsSettings` (daily budget, auto-attach, retention) — persisted by host applications via their own settings store |
| `secret.rs` | API key lookup helpers (`PLAYA_FAL_KEY` / `FAL_KEY` / `FAL_API_KEY` env vars + `.env` paths) |
| `cancel.rs` | `CancelToken` — cooperative cancel signal threaded through `JobContext` |

## Public surface (canonical)

```rust
use playa_jobs_core::{JobQueue, JobQueueConfig, JobProvider, EventBus};
use std::sync::Arc;

let bus = Arc::new(EventBus::new());
let queue = JobQueue::new(
    JobQueueConfig {
        thread_count: 4,
        files_dir: "/tmp/jobs".into(),
        persist_path: Some("/tmp/jobs.jsonl".into()),
    },
    Arc::clone(&bus),
)?;

queue.register_provider(MyProvider::new(api_key));
let id = queue.submit("my.kind", serde_json::json!({"prompt": "..."}))?;

bus.subscribe::<playa_jobs_core::JobEvent, _>(|ev| log::info!("{ev:?}"));
```

`JobQueue` is `Clone` (internally `Arc`) — share freely across threads.

## How it relates to its siblings

```
playa-jobs-core ──┬── playa-job-seedance     (Seedance i2v/t2v provider)
                  ├── playa-job-inpaint      (Flux Pro v1.1 inpaint provider)
                  ├── playa-jobs-ui          (egui panel + submit dialog)
                  └── playa-jobs             (facade — one dep for hosts)

playa-jobs-core ──> playa-events             (EventBus types)
```

## Features

- `persist` (default on) — JSONL log + crash-resume. Off for tests
  that want pure in-memory queues.

## Tests

48 unit tests at the time of writing. Run with `cargo test
-p playa-jobs-core`. Tests cover the state machine, persist log
round-trip, budget enforcement, retry/remove API, and the
EventBus-mandatory `new(cfg, event_bus)` contract.

## License

MIT.
