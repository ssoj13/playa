# Jobs Manager — design

Status: **proposal, awaiting approval**. Implementation order at the bottom.

Goal: Thinkbox Deadline-style jobs manager scoped to single-machine,
single-queue. Industrial UX (sortable table, filters, multi-select, bulk
actions, detail pane, stats footer) without the enterprise-scale infra
(distributed worker pool, license servers, dependency graphs). "Deadline
Lite" — what an experienced TD would expect to see.

## Persistence — current state

What survives a restart today (verified via `playa-jobs` 31/31 tests + live
fal.ai run):

| Item | Survives? | Where |
|---|---|---|
| Job snapshot (id, kind, params, state, progress, error, result) | ✅ | JSONL log at `dirs_next::config_dir()/playa/jobs.jsonl` |
| `created_at` / `updated_at` (unix seconds) | ✅ | persisted via `LogEntry::Updated.updated_at` |
| `request_id`, `status_url`, `response_url` | ✅ | written via `JobContext::persist_param` BEFORE Submitting → AwaitingProvider so a restart can resume polling without re-submit |
| Resumable states (`Pending`/`Submitting`/`AwaitingProvider`/`Downloading`) | ✅ auto re-enqueued | `JobQueue::replay_persisted` |
| Terminal states (`Complete`/`Failed`/`Cancelled`) | ✅ restored to map | `JobQueue::list()` shows them after replay |
| Final `mp4_path` | ✅ | path stored in `result.mp4_path`; file lives under `dirs_next::cache_dir()/playa/jobs/{job_id}/output.mp4` (NOT `%TEMP%`, so OS cleanup leaves it alone) |
| Mid-download progress | ❌ | partial file, resume re-downloads the mp4 from fal (free; signed URL valid 24h after `succeeded`) |
| Per-state time breakdown | ❌ NOT TRACKED | requires schema addition — see below |
| Per-job event log (every progress emit) | ❌ NOT TRACKED | only the last progress survives — see below |

Crash-race caveat — a process kill between `fal.tx.send()` succeeding (fal
billed) and the next `persist_param("request_id", ...)` write loses the
request_id. On resume the provider would re-submit (= double bill).
Mitigations:
- (a) Persist a `request_pending: true` marker BEFORE the HTTP call and
  remove it after persist_param succeeds. On resume, if `request_pending`
  is set, hit fal's job-listing endpoint to find any in-flight task tagged
  with our `end_user_id` (fal supports this field) and reconcile.
- (b) Status quo — accept the millisecond window. Probability is tiny.

Recommend (b) for v1, (a) as a future hardening if a real incident hits.

## Required schema additions (`Job`)

```rust
pub struct Job {
    // ... existing fields ...

    /// Per-state-transition log. Append-only. Each tuple = (state, unix_seconds_at_entry).
    /// First entry is always (Pending, created_at).
    /// Empty vec for jobs created before this field was added — UI handles None gracefully.
    #[serde(default)]
    pub state_history: Vec<(JobState, u64)>,

    /// Provider-supplied estimate of total runtime for ETA bars. None if the
    /// provider has no idea (e.g. AwaitingProvider with unknown queue depth).
    #[serde(default)]
    pub estimated_total_secs: Option<u32>,

    /// Provider-reported per-job cost in USD (provider populates on Complete).
    /// `Job.kind` + the provider's pricing table compute it; e.g. SeedanceProvider
    /// fills it as `0.3034 * duration_secs` after Complete.
    #[serde(default)]
    pub cost_usd: Option<f64>,
}
```

Persist additions: one new variant `LogEntry::StageEntered { id, state, at }`
written by the updater thread on every state transition. Replay folds it
into `state_history`. Net log size still O(events) but each event is tiny
(~80 bytes) vs the current full-snapshot Updated which carries the whole
progress + result + error blob (~1 KB). Total log size shrinks by ~10× in
typical sessions.

## Required `JobQueue` API additions

```rust
impl JobQueue {
    /// Drop a TERMINAL job from the in-memory map and append a tombstone to
    /// the persist log so replay does not resurrect it. No-op for non-terminal
    /// jobs (return Err so UI can keep its delete button greyed out).
    pub fn remove(&self, id: JobId) -> Result<(), JobError>;

    /// Re-submit a TERMINAL Failed/Cancelled job with the same kind + params.
    /// Returns the **new** JobId; the old job stays in the list as history.
    /// Errors if the original is missing or non-terminal.
    pub fn retry(&self, id: JobId) -> Result<JobId, JobError>;

    /// Cheap aggregate read for the stats footer.
    pub fn stats(&self) -> JobStats;

    /// Filter accessor for the table view.
    pub fn list_filtered(&self, filter: &JobFilter) -> Vec<Job>;
}

pub struct JobStats {
    pub by_state: HashMap<JobState, usize>,
    pub total_cost_usd: f64,
    pub today_cost_usd: f64,
    pub today_completed: usize,
    pub queue_depth: usize,        // Pending + AwaitingProvider count
    pub active_providers: usize,
}

pub struct JobFilter {
    pub state: Option<Vec<JobState>>,    // None = all
    pub kind_prefix: Option<String>,     // e.g. "seedance." matches both endpoints
    pub search: Option<String>,          // matches against prompt / error / id
    pub since: Option<u64>,              // unix seconds, exclude older
}
```

`remove()` and `retry()` write `LogEntry::Tombstone(id)` and a fresh
`LogEntry::Created(new_job)` respectively, so the persist contract stays
honest.

## UI layout (mocked, egui dock tab)

Adds new dock tab `Jobs` next to existing `Project` / `Attributes` /
`Timeline`.

```
┌─ Jobs ──────────────────────────────────────────────────────────────────┐
│ ┌──────────────────────────────────────────────────────────────────────┐│
│ │ 🔍 [search prompt/error/id....] State [▼ All] Provider [▼ All]      ││
│ │ Range [▼ Today] [Clear filters]                       [⟳ Refresh]   ││
│ └──────────────────────────────────────────────────────────────────────┘│
│ ┌──────────────────────────────────────────────────────────────────────┐│
│ │ ☐ │ Submitted   │ Elapsed │ Kind          │ State        │ Progress ││
│ │   │             │         │               │              │          ││
│ │ ☐ │ 12:34:01    │ 0:00:42 │ seedance.t2v  │ ⏳ Polling   │ q=3 (--) ││
│ │ ☐ │ 12:31:18    │ 0:03:04 │ seedance.t2v  │ ✓ Complete   │ —        ││
│ │ ☐ │ 12:28:55    │ 0:00:18 │ seedance.i2v  │ ✗ Failed     │ 401      ││
│ │ ☐ │ ...                                                             ││
│ │     [columns sortable; right-click → context menu;                   ││
│ │      shift-click range, ctrl-click multi-select]                     ││
│ └──────────────────────────────────────────────────────────────────────┘│
│ Selected (1): [⏹ Cancel] [↻ Retry] [🗑 Delete] [📁 Reveal mp4]         │
│ ┌─ Detail pane (resizable side panel) ────────────────────────────────┐│
│ │ Job: req-abc-123 (seedance.text_to_video)                           ││
│ │ State: ✓ Complete   Cost: $1.21 USD                                  ││
│ │                                                                      ││
│ │ Submitted: 12:31:18                                                  ││
│ │ Updated:   12:34:22  (elapsed 3m 04s)                                ││
│ │                                                                      ││
│ │ Time in state:                                                       ││
│ │   Pending          0:00:01                                           ││
│ │   Submitting       0:00:02                                           ││
│ │   AwaitingProvider 0:02:45  ████████████████░░  (88%)                ││
│ │   Downloading      0:00:14                                           ││
│ │   Staging          0:00:02                                           ││
│ │                                                                      ││
│ │ ▼ Params       ▼ Result        ▼ Error    ▼ Event log               ││
│ │ {                                                                    ││
│ │   "prompt": "a cyberpunk story of a red hood ...",                   ││
│ │   "resolution": "480p",                                              ││
│ │   "duration": "4",                                                   ││
│ │   ...                                                                ││
│ │ }                                                                    ││
│ │                                                                      ││
│ │ mp4: %LOCALAPPDATA%\playa\jobs\<uuid>\output.mp4 (1.2 MB)            ││
│ │ [▶ Preview]  [📋 Copy path]  [📁 Open folder]  [→ Drop on timeline] ││
│ └──────────────────────────────────────────────────────────────────────┘│
│ Footer: 4 active · 12 today · $14.52 today · $124.80 lifetime          │
└─────────────────────────────────────────────────────────────────────────┘
```

Behaviours:
- **Subscribe → repaint**: `JobQueue::subscribe(closure)` pushes events into a
  `Mutex<JobsViewState>` that the panel reads each frame. `request_repaint()`
  triggered when listener fires (matches existing pattern in
  `gpu_blend_bridge` drain).
- **Sort**: any column header click toggles asc/desc. Default = Submitted DESC.
- **Filter** is non-destructive: `JobQueue::list_filtered(&filter)` is cheap
  (<1ms for thousands of jobs); recomputed each frame.
- **Multi-select**: ctrl-click adds, shift-click range, ctrl-A all visible.
  Bulk actions enabled when ≥1 selected. `Cancel` enabled if any selected
  is non-terminal; `Retry` enabled if any is Failed/Cancelled; `Delete`
  enabled if **all** selected are terminal.
- **Right-click context menu** mirrors action bar + `Copy id`, `Copy params
  JSON`, `Copy error`, `Mark as priority` (future).
- **Detail pane** auto-shows when exactly one row selected. Multi-select
  shows aggregate (count, total cost, etc).
- **Preview button**: in-app `eframe::wgpu::Texture` from a single-frame
  decode via existing `playa-ffmpeg` — no external player launch. Hold
  button → scrub.
- **Drop on timeline**: drag-drop preview thumbnail onto active timeline
  comp = `app.project.with_comp(...)` adds a new layer with the mp4 as
  source. Equivalent to manual `Open File`.
- **Footer stats** fetched from `JobQueue::stats()` once per frame; cheap
  HashMap walk over <few-thousand jobs.

## "Generate via Seedance…" submit dialog (separate widget, opened by menu /
right-click)

```
┌─ Generate via Seedance ─────────────────────────────────────────────────┐
│ Endpoint:  ◉ Text-to-Video    ◯ Image-to-Video                          │
│                                                                          │
│ Prompt:                                                                  │
│ ┌──────────────────────────────────────────────────────────────────────┐│
│ │ a cyberpunk story of a red hood and wolf in a cybernetic future      ││
│ │                                                                      ││
│ └──────────────────────────────────────────────────────────────────────┘│
│ Image URL: [____________________________________]   [Browse local…]    │
│                                              (only when Image-to-Video) │
│                                                                          │
│ Resolution: ◉ 480p  ◯ 720p  ◯ 1080p  (1080p only for image-to-video)    │
│ Duration:   [4 ▼] s   (4..15 or auto)                                   │
│ Aspect:     [auto ▼]                                                    │
│ ☐ Generate audio                                                         │
│ Seed (optional): [_______]                                              │
│                                                                          │
│ Estimated cost: $1.21 USD (480p × 4 s, standard tier)                   │
│                                                                          │
│ ☐ Auto-attach completed mp4 to active comp as new layer                 │
│                                                                          │
│                                          [Cancel]  [Submit]              │
└─────────────────────────────────────────────────────────────────────────┘
```

- Cost recomputes live as user changes resolution/duration.
- `Browse local…` (image-to-video) opens file dialog → uploads to fal's
  storage endpoint via `POST https://fal.run/files/upload` → fills the URL
  field with the returned signed URL.
- "Auto-attach" remembers last setting in `AppSettings`.
- Submit disabled until prompt is non-empty (and image URL non-empty when
  i2v).
- Right-click on a layer → "Generate continuation…" pre-fills the dialog
  in i2v mode with the layer's source as `image_url`.

## Daily budget cap

`AppSettings::daily_budget_usd: Option<f64>` (default `None`). When set,
`JobQueue::submit` for any `seedance.*` kind first reads `today_cost_usd`
from `JobStats` and rejects the call with `JobError::Provider("daily budget
exceeded")` if exceeding. Settings panel exposes the slider; defaults
to `None` (no cap). Persist log replay reconstructs `today_cost_usd` from
the day's `Complete` entries.

## Implementation order

Each numbered step is a separate commit; UI work blocks on schema work
because the panel reads new fields.

| # | Wave | What | Tests | Estimate |
|---|---|---|---|---|
| 1 | 7a | `Job.state_history` + `LogEntry::StageEntered` + `JobQueue::stats` + `JobFilter` + `list_filtered` | unit tests in `playa-jobs` | ~1 h |
| 2 | 7b | `JobQueue::remove(id)` + `retry(id)` with persist tombstones + tests | unit tests | ~30 min |
| 3 | 7c | `SeedanceProvider` populates `Job.cost_usd` on Complete | unit test in mock | ~15 min |
| 4 | 7d | `playa-ui::widgets::jobs::jobs_ui` — table view, sort, filter, multi-select, action bar | egui-only, no link issues | ~2 h |
| 5 | 7e | Detail pane with state-history bar + params/result/error tabs | egui | ~1 h |
| 6 | 7f | "Generate via Seedance…" dialog under `playa-ui::dialogs::seedance` | egui | ~1.5 h |
| 7 | 7g | Add `Jobs` `DockTab` + register in default layout + menu wiring | thin glue | ~30 min |
| 8 | 7h | Local image upload helper (fal `POST /files/upload`) for image-to-video | unit test with mock | ~45 min |
| 9 | 7i | Auto-attach completed mp4 → layer; right-click "Generate continuation…" | event subscriber + dialog pre-fill | ~1 h |
| 10 | 7j | Daily budget cap + settings UI | small | ~30 min |
| 11 | 7k | Optional: in-panel mp4 preview via single-frame decode | ffmpeg + texture | ~1.5 h |

Total: ~10 h to "very polished". Minimum viable Jobs Manager (steps 1–5+7) ≈
~5 h.

## What I'd ship in waves

- **Wave 7 (backend)**: steps 1–3. Pushable independently; provider
  populates new fields; persist log handles new variant.
- **Wave 8 (UI core)**: steps 4–5+7. The Jobs tab itself. After this you
  have a working Deadline-Lite view. Submit still happens via example bin
  or hand-rolled code.
- **Wave 9 (submit ergonomics)**: steps 6, 8, 9. "Generate via Seedance…"
  dialog + local image upload + auto-attach. Closes the loop "click → mp4
  on timeline".
- **Wave 10 (guardrails & polish)**: steps 10, 11. Daily budget cap +
  in-panel preview.

After Wave 8 the question "is everything done and controllable" answers
itself: yes, with all expected manager controls, persistent across
restarts, with stats footer.

## Open questions for you

1. **Daily budget cap** default — `None` (no cap) for v1, configurable in
   settings? Or hardcode a soft warning at $20/day if no setting?
2. **Auto-attach mp4 as layer** — opt-in checkbox per submit, or global
   default in settings, or both?
3. **In-panel mp4 preview** — single-frame thumbnail (cheap), or full
   playback inside the panel? Full playback duplicates viewport. Probably
   thumbnail is enough; full preview = open in viewport.
4. **Multi-select bulk Cancel** — confirm dialog before cancelling N
   running jobs (each with non-trivial cost)?
5. **Job retention** — auto-delete Complete jobs older than X days? If
   yes, settings knob; default 30 days. If no, only manual delete.
6. **Layout default** — Jobs tab in same panel as Project/Attributes by
   default, or its own dock tab somewhere else?
