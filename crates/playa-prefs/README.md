# playa-prefs

Pluggable preferences system for egui applications. Each subsystem
(jobs, viewport, cache, …) owns its own preferences UI and registers
a single render closure; a central **Preferences window** aggregates
them via a tree-view sidebar with search bar + Apply/OK/Cancel
buttons.

## Why this crate exists

Mature creative tools (After Effects, Houdini, Maya) let every module
expose its own preferences UI; one big monolithic settings panel is
the anti-pattern. `playa-prefs` provides the infrastructure: a
generic `PrefsRegistry<S>` over the host's settings type `S`, plus a
modal `PrefsWindow<S>` that drives the layout.

The crate is **hermetic**: no playa-engine, no playa-jobs, no
playa-ui. Just `egui`. Any egui app can use it.

## Module map

| File | Purpose |
|---|---|
| `lib.rs` | All of the public surface lives here (it's a small crate): `PrefsEntry<S>`, `PrefsRegistry<S>`, `PrefsWindow<S>`, `PrefsResult` enum, render impl |

## Public surface (canonical)

```rust
use playa_prefs::{PrefsEntry, PrefsRegistry, PrefsWindow, PrefsResult};

// Host AppSettings is any `Clone + PartialEq`:
let mut registry: PrefsRegistry<AppSettings> = PrefsRegistry::new();

registry.add(PrefsEntry {
    id: "jobs",
    label: "Jobs & Rendering",
    category: "Integrations",
    search_keywords: vec!["budget", "queue", "fal", "seedance"],
    render: Box::new(|ui, settings: &mut AppSettings| {
        playa_jobs::ui::prefs::render(ui, &mut settings.jobs);
    }),
});

let mut window = PrefsWindow::<AppSettings>::new();
// Open via menu / hotkey:
window.open_with(&app_settings);

// Each frame:
match window.show(ctx, &mut registry, &mut app_settings) {
    PrefsResult::Applied  => log::info!("Preferences applied"),
    PrefsResult::OkClosed => log::info!("Preferences applied + closed"),
    PrefsResult::Cancelled => log::info!("Cancelled"),
    PrefsResult::Open | PrefsResult::Closed => {}
}
```

## State machine

```
                          open_with(state)
                          ──────────────→
                  ┌──── working_copy = state.clone() ────┐
                  │     last_applied = state.clone()     │
                  │                                       │
       Closed ────┴─→  Open ──Apply──→  Open (dirty=false, working=state)
                       │ │
                       │ └──OK──→  Closed (working_copy committed back)
                       │
                       └─Cancel─→  Closed (working_copy discarded)
```

The Apply button is disabled until `working_copy != last_applied` —
dirty-tracking for free if `S: PartialEq`.

## Search

Search filters entries by case-insensitive substring across `label`,
`category`, and `search_keywords`. Empty query → all entries visible.

## Tests

15 unit tests covering registry CRUD, search filter cases, and the
PrefsWindow state machine transitions.

## License

MIT.
