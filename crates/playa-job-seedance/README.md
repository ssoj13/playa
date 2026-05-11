# playa-job-seedance

`playa-jobs` provider for **Seedance 2.0** video generation via fal.ai.
Implements both endpoints:

- `bytedance/seedance-2.0/image-to-video` (kind `seedance.image_to_video`)
- `bytedance/seedance-2.0/text-to-video` (kind `seedance.text_to_video`)

## Why this crate exists

`playa-jobs-core` is provider-agnostic. Each AI/upload/encoding
backend is a sibling crate implementing the `JobProvider` trait. This
keeps the dependency graph small (no fal-specific HTTP code in the
engine) and isolates rate/auth quirks per provider.

## Surface

| File | Purpose |
|---|---|
| `lib.rs` | Re-exports |
| `http.rs` | `FalHttp` trait + `UreqFalHttp` prod impl. Trait split for scripted-mock tests |
| `params.rs` | Typed param builders `SeedanceImageToVideoParams` / `SeedanceTextToVideoParams` (optional — `JobQueue::submit` accepts raw `serde_json::Value`) |
| `provider.rs` | `SeedanceProvider` impl with `SeedanceEndpoint` enum + `kinds` submodule. Submit → poll → download MP4 → return `{mp4_path, video_url, bytes, fal_response}` |

## Crash-resume contract

Before the `Submitting → AwaitingProvider` state transition, the
provider calls `JobContext::persist_param` for `request_id`,
`status_url`, and `response_url`. If the process restarts mid-poll,
`resume()` reads them back and re-enters the poll loop **without
re-billing fal**. See `playa-jobs-core` persist documentation for the
log replay path.

## Cost

At time of writing fal charges (standard tier):
- $0.3024 / second for image-to-video
- $0.3034 / second for text-to-video

`SeedanceProvider::estimate_cost_usd` parses `duration` from params
(integer for i2v, string for t2v per fal wire convention) and
multiplies by the per-second rate. Used by `JobQueue::submit` to
enforce the user's daily budget cap before the API call is made.

## Public surface (canonical)

```rust
use std::sync::Arc;
use playa_jobs_core::{EventBus, JobQueue, JobQueueConfig};
use playa_job_seedance::{SeedanceProvider, kinds};

let bus = Arc::new(EventBus::new());
let queue = JobQueue::new(JobQueueConfig::default(), Arc::clone(&bus))?;

let api_key = std::env::var("FAL_KEY")?;
queue.register_provider(SeedanceProvider::text_to_video(api_key.clone()));
queue.register_provider(SeedanceProvider::image_to_video(api_key));

let id = queue.submit(
    kinds::TEXT_TO_VIDEO,
    serde_json::json!({
        "prompt": "a wolf in a cyberpunk forest",
        "duration": "5",     // t2v wants string
        "resolution": "480p",
        "seed": 42_u64,
    }),
)?;
```

## Tests

19 unit tests via `MockHttp`-scripted responses — full state machine
exercised without network. Includes happy path (submit → 3 poll
ticks → download), unexpected status, missing fields in submit
response, cancel propagation, and crash-resume URLs round-trip.

## License

MIT.
