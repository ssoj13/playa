# playa-jobs

All-in-one **facade** crate for the playa jobs subsystem. Hosts that
just want one dep + sensible defaults pull this; advanced consumers
pick the individual `playa-jobs-*` crates they need.

## Why this crate exists

Six crates (`playa-jobs-core`, `playa-jobs-ui`, `playa-prefs`,
`playa-job-seedance`, `playa-job-inpaint`, `playa-events`) cooperate
to deliver the full async-jobs UX. Wiring them up one-by-one is
tedious. This crate:

- Re-exports the right types under namespaced modules so foreign code
  reads `playa_jobs::JobQueue` / `playa_jobs::ui::JobsPanel` /
  `playa_jobs::seedance::SeedanceProvider` instead of remembering
  six crate names.
- Provides cargo features that hide the optional crates behind
  on/off toggles.
- Exposes setup helpers (`setup_with_fal`, `register_default_prefs`)
  that build the conventional configuration in one call.

## Features

| Feature | Default | Pulls in |
|---|---|---|
| `ui` | ✅ | `playa-jobs-ui` + `playa-prefs` |
| `prefs` | ✅ | `playa-prefs` |
| `seedance` | ✅ | `playa-job-seedance` (Seedance i2v + t2v providers) |
| `inpaint` | ✅ | `playa-job-inpaint` (Flux Pro v1.1 inpainting provider) |
| `persist` | ✅ | `playa-jobs-core/persist` (JSONL crash-resume log) |

Disable any subset with `default-features = false` + selective opt-in:

```toml
playa-jobs = { path = "../playa-jobs", default-features = false, features = ["ui", "prefs"] }
```

Pure data / headless use:

```toml
playa-jobs = { path = "../playa-jobs", default-features = false, features = ["persist"] }
```

## Module map

| Path | Re-exports |
|---|---|
| `playa_jobs::*` | All of `playa_jobs_core` flat (Job, JobQueue, JobError, JobEvent, JobsSettings, EventBus, …) |
| `playa_jobs::ui` | `playa_jobs_ui::*` (JobsPanel, SubmitDialog, …) — cfg(ui) |
| `playa_jobs::prefs` | `playa_prefs::*` — cfg(prefs) |
| `playa_jobs::seedance` | `playa_job_seedance::*` — cfg(seedance) |
| `playa_jobs::inpaint` | `playa_job_inpaint::*` — cfg(inpaint) |
| `playa_jobs::secret` | API key lookup from env + `.env` files |

## Setup helpers (canonical)

```rust
use std::sync::Arc;
use playa_jobs::{EventBus, JobQueueConfig};

let event_bus = Arc::new(EventBus::new());

// One-liner queue + Seedance + Inpaint providers auto-registered
// when a FAL key is found in env / .env paths.
let queue = playa_jobs::setup_with_fal(
    event_bus,
    JobQueueConfig {
        thread_count: 4,
        files_dir: "~/.cache/playa/jobs".into(),
        persist_path: Some("~/.config/playa/jobs.jsonl".into()),
    },
    &[".env".into(), "../.env".into()],
)?;

// Hook the prefs panel into the host's PrefsRegistry:
playa_jobs::register_default_prefs(&mut registry, |s: &mut AppSettings| {
    &mut s.jobs
});
```

## How it relates to its siblings

```
                  ┌─→ playa-jobs-core     (always)
                  ├─→ playa-jobs-ui       (cfg(ui))
playa-jobs (facade)
                  ├─→ playa-prefs         (cfg(prefs))
                  ├─→ playa-job-seedance  (cfg(seedance))
                  └─→ playa-job-inpaint   (cfg(inpaint))
```

## Tests

10 integration tests using ONLY `playa_jobs::*` imports (no
`playa_jobs_core::` direct access). Verifies the facade actually
exposes the canonical workflow.

## License

MIT.
