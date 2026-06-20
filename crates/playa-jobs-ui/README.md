# playa-jobs-ui

egui widgets for the playa jobs subsystem: the **Jobs panel** (sortable
table with cancel / retry / delete / reveal actions), the **Submit
dialog** (Seedance text-to-video / image-to-video with live cost
preview + batch mode), and the **Preferences panel** that mutates
`JobsSettings`.

## Why this crate exists

UI bits live separately from the engine so headless / CI / batch hosts
can depend on `playa-jobs-core` without dragging in `eframe`. Hosts
that want the full UX pull this crate (transitively via the
`playa-jobs` facade with `ui` feature on by default).

## Module map

| File | Purpose |
|---|---|
| `lib.rs` | Re-exports |
| `panel.rs` | `JobsPanel` widget — `TableBuilder` of jobs with sort columns (state / kind / created / cost / progress), filter bar, multi-select, action bar. Returns `JobsAction { None / Cancel / Retry / Delete / RevealMp4 / OpenSubmit }` for the host to dispatch |
| `dialog.rs` | `SubmitDialog` modal — endpoint radio (TextToVideo / ImageToVideo), prompt textarea, image URL with `📸 Snapshot current frame` button, resolution / duration / aspect / audio / seed, auto-attach checkbox, batch-mode toggle, live cost estimate. Emits `SubmitDialogResult::Submit { kind, params_batch, auto_attach }` |
| `prefs.rs` | `pub fn render(ui: &mut Ui, settings: &mut JobsSettings)` — drop-in pref panel; host wires this into its Preferences modal via `playa_prefs::PrefsRegistry` |
| `state.rs` | View state for the panel (sort column, filter text, selection set) |

## Public surface (canonical)

```rust
use playa_jobs_ui::{JobsPanel, SubmitDialog, JobsAction, SubmitDialogResult};
use playa_jobs_core::JobQueue;

// Each frame:
let action = panel.ui(ui, &queue);
match action {
    JobsAction::Cancel(ids) => for id in ids { queue.cancel(id); },
    JobsAction::Retry(ids)  => for id in ids { let _ = queue.retry(id); },
    JobsAction::Delete(ids) => for id in ids { let _ = queue.remove(id); },
    JobsAction::RevealMp4(id) => { /* opener::open(...) */ },
    JobsAction::OpenSubmit => submit_dialog.open(),
    JobsAction::None => {}
}

// Submit dialog modal:
if let SubmitDialogResult::Submit { kind, params_batch, .. } =
       submit_dialog.show(ctx) {
    for params in params_batch {
        queue.submit(kind, params).ok();
    }
}
```

## How it relates to its siblings

```
playa-jobs-ui ──> playa-jobs-core   (Job, JobQueue types)
              ──> playa-prefs       (PrefsRegistry consumes prefs::render)
              ──> eframe / egui_extras
```

## Tests

24 unit tests covering the SubmitDialog state machine (validity,
batch mode prompt splitting, cost estimation, params building per
endpoint) and the JobsPanel filter/sort logic without egui rendering.

## License

MIT.
