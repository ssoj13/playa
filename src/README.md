# Root `src/` (package `playa`)

The **`playa`** crate at the workspace root exposes the application binary plus a slim **library surface** (`lib.rs` re-exports `playa-engine`, `playa-events`, `playa-ui`, `playa-app`, …). Most implementation code lives under **`crates/`**.

See **[AGENTS.md](../AGENTS.md)** for architecture, event flow, and cache behaviour.

```
src/
├── main.rs    # FFmpeg init (`playa_io::init_*`) → CLI → logging → playa_app::run_app
├── lib.rs    # Stable `playa::` API for tooling (e.g. playa-py) and aggregates
└── README.md # this note
```

**Where modules moved:**

| Area | Crate / path |
|------|----------------|
| PlayaApp, `main_events`, runner, CLI, server, shell, config | **`crates/playa-app`** |
| Engine (`core`, `entities`, loaders, CPU compositing, defaults, …) | **`crates/playa-engine`** |
| egui widgets, dialogs, viewport, menu composition | **`crates/playa-ui`** |
| Decode / FFmpeg / EXR façade | **`crates/playa-io`** |
| Shared typed events (`EventBus` types, tool mode, UI events…) | **`crates/playa-events`** |
| Build automation (`cargo xtask …`) | **`crates/xtask`** |

## Event-driven routing

Typed events traverse `EventBus`; application dispatch lives in **`crates/playa-app`** (`main_events` module connects playhead/UI actions to **`Project`** / **`CacheManager`**).

## Cache & workers

Workers run in **`playa-engine`**; frame cache epochs and LRU handling are documented in **AGENTS.md** (sections on `GlobalFrameCache`, `DebouncedPreloader`, `Workers`).

## Python bindings

**`crates/playa-py`** is a **separate workspace** (`[workspace.exclude]`). Build helpers:

```bash
python bootstrap.py python          # wheel / maturin flow
python bootstrap.py python-reqs      # developer Python deps (see script help)
python bootstrap.py python --install # optional; see bootstrap.py HELP_TEXT
```

**Usage:**
```python
import playa

playa.run(file="image.exr", autoplay=True, fullscreen=False)
playa.run(files=["a.exr", "b.exr"], loop_playback=True)
print(playa.version())
```
