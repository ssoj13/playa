# playa-job-inpaint

`playa-jobs` provider for **image inpainting** via fal.ai. Currently
ships one endpoint:

- `fal-ai/flux-pro/v1.1/inpainting` (kind `inpaint.flux_pro_v1_1`)

Future variants (Runway gen-fill, Seedream inpaint, SD-based) plug in
as new `InpaintEndpoint` enum variants.

## Why this crate exists

Mirrors `playa-job-seedance`: each provider is a sibling crate of
`playa-jobs-core` that implements the `JobProvider` trait. Keeps fal /
HTTP quirks isolated from the engine; lets the workspace add
providers without bumping every crate.

Inpaint is conceptually different from video gen — the **inputs** are
base image + mask (both as data URLs or hosted URLs), the **output**
is one PNG per request. The provider surface is structurally
identical to Seedance: submit → poll → download.

## Surface

| File | Purpose |
|---|---|
| `lib.rs` | Re-exports + crate-level rustdoc |
| `http.rs` | `FalHttp` trait + `UreqFalHttp` prod impl. Duplicated from `playa-job-seedance::http` for crate independence — a future refactor can lift to a shared `playa-fal-http` if more providers land |
| `provider.rs` | `InpaintProvider` impl + `InpaintEndpoint` enum (one variant for v1) + `kinds` submodule. Submit → poll → download PNG → return `{png_path, image_url, bytes, fal_response}` |

## Crash-resume contract

Same as Seedance: `request_id`, `status_url`, `response_url`
persisted before `Submitting → AwaitingProvider`. `resume()` reads
them back and re-enters the poll loop without re-billing fal.

## Cost

Flux Pro v1.1 inpainting bills per-megapixel. At the time of writing
fal charges ~**$0.05 / megapixel**. A 1024×1024 (1 MP) inpaint is
~$0.05.

`InpaintProvider::estimate_cost_usd` honours two param shapes used in
the wild:

- `{ "width": <u64>, "height": <u64> }` — explicit pixels
- `{ "image_size": "WxH" }` — fal-style string

Returns `None` when neither is present (queue treats unknown
estimates as $0 against the daily cap; favours submit over false
rejection).

## Public surface (canonical)

```rust
use std::sync::Arc;
use playa_jobs_core::{EventBus, JobQueue, JobQueueConfig};
use playa_job_inpaint::{InpaintProvider, kinds};

let bus = Arc::new(EventBus::new());
let queue = JobQueue::new(JobQueueConfig::default(), Arc::clone(&bus))?;
queue.register_provider(InpaintProvider::flux_pro_v1_1(std::env::var("FAL_KEY")?));

let id = queue.submit(
    kinds::FLUX_PRO_V1_1_INPAINTING,
    serde_json::json!({
        "image_url": "data:image/png;base64,...",   // base image
        "mask_url":  "data:image/png;base64,...",   // white = inpaint, black = preserve
        "prompt":    "a cybernetic wolf",
        "seed":      42_u64,                         // resolved client-side for reproducibility
        "width":     1024_u64,
        "height":    1024_u64,
    }),
)?;
```

The provider accepts both **HTTP(S) URLs** and **data URLs**
(`data:image/png;base64,...`) for `image_url` / `mask_url` — the
playa-app submit flow base64-encodes local PNGs inline rather than
running a separate upload step.

## Tests

8 unit tests:
- `kind_string_stable` + canonical submit URL
- megapixel parsing for both param shapes + absent=None
- estimate_cost_usd at known rates
- full submit → poll → complete → download happy path via `MockHttp`
  scripted responses

## License

MIT.
