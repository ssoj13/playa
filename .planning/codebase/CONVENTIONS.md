# Coding Conventions

**Analysis Date:** 2026-05-09
**Scope:** Workspace `playa` (edition 2024, Rust 1.95+ stable). Authoritative source: `AGENTS.md` "Coding rules" + "Surprises and gotchas". This document captures what is **actually used** in the code, not textbook Rust ideals.

---

## Toolchain & Edition

| Item | Value | Where |
|------|-------|-------|
| Rust edition | `2024` | `Cargo.toml`, every `crates/*/Cargo.toml` |
| Workspace version | `0.1.142` (synced across `playa`, `playa-app`, `playa-engine`, `playa-events`, `playa-io`, `playa-ui`) | `Cargo.toml`, `crates/*/Cargo.toml` |
| MSRV | Not declared (`rust-version =` not set) | — |
| Resolver | `2` | `Cargo.toml` |
| Profile.release | `strip = false`, `lto = false`, `codegen-units = 1` commented out — optimized for **link speed**, not binary size | `Cargo.toml:72-74` |
| `[lints]` table | **Not used.** No clippy denylist; no `unsafe_code = "deny"`; no MSRV check. Discipline is by convention only | `Cargo.toml`, all crate manifests |

> RUST.md describes an aspirational `[lints]` block (deny `unwrap_used`, etc.), but the project does **not** wire it up.

---

## Crate Layout & Library Path

Every workspace crate explicitly sets `[lib] path` to a snake-cased filename matching the crate. This pattern is uniform — match it for any new crate.

```toml
# crates/playa-engine/Cargo.toml
[lib]
name = "playa_engine"
path = "src/lib.rs"
```

| Crate | `[lib] name` | Notes |
|-------|--------------|-------|
| `playa-engine` | `playa_engine` | Domain types + core infra |
| `playa-app` | `playa_app` | `eframe`-based shell |
| `playa-events` | `playa_events` | EventBus + typed events |
| `playa-io` | `playa_io` | Loaders behind `exr` / `ffmpeg` features |
| `playa-ui` | `playa_ui` | egui widgets/dialogs |

The root `playa` crate is a **thin re-export aggregator** (`src/lib.rs` re-exports the surfaces of the four library crates); the binary lives at `src/main.rs`.

---

## Workspace Dependency Hygiene

Dependencies shared across more than one crate **must** flow through `[workspace.dependencies]` in the root `Cargo.toml` and be referenced as `dep = { workspace = true }`. Inspected manifests use this pattern uniformly: `anyhow`, `bytemuck`, `clap`, `egui-wgpu`, `egui_dock`, `env_logger`, `glam`, `half`, `image`, `log`, `serde`, `serde_json`, `uuid`, `wgpu`, `playa-ffmpeg`.

Crate-specific deps (e.g. `cosmic-text`, `crossbeam`, `enum_dispatch`, `lru`, `rayon`, `sysinfo`, `rouille`, `regex`, `rfd`, `dirs-next`, `scanseq`, `egui_dnd`, `egui-snarl`, `transform-gizmo-egui`) are pinned **only** in the crate that uses them. Don't promote a dep to the workspace until at least two crates need it.

`AGENTS.md` rule: **don't grow `Cargo.toml` needlessly** — review new dependencies critically.

---

## Error Handling

| Layer | Idiom |
|-------|-------|
| Domain (`playa-engine`) | Custom `FrameError` (defined in `crates/playa-engine/src/entities/frame.rs`); referenced in `entities/loader.rs`, `entities/file_node.rs`. `playa-io`'s `IoError` (`crates/playa-io/src/error.rs`) is mapped to `FrameError` at the boundary |
| Application (`playa-app`, `xtask`) | `anyhow::Result` + `.context("...")?` (e.g. `crates/playa-app/src/runner.rs`, `crates/xtask/src/main.rs`) |
| Effects (`entities/effects/*.rs`) | Return `Option<Frame>` (returns `None` on processing failure) — see `effects/blur.rs:32`, `effects/brightness.rs:31`, `effects/hsv.rs:33` |
| Errors at FFI / OS boundary | Propagate via `?`; never silently `let _ = …` fallible ops |

**Rules from `AGENTS.md`:**
- Production code **avoids** `unwrap()` / `expect()`. There are still 71 `.unwrap()` occurrences workspace-wide (most legitimate: in `#[cfg(test)] mod tests`, `Mutex` / `RwLock` lock acquisition with poison recovery, or one-time deserialization of trusted constants). Don't add new ones to fresh code.
- `unwrap_or_else(|e| e.into_inner())` is the canonical recovery for `PoisonError` after a `Mutex` / `RwLock` lock — used 52× across 9 files (e.g. `crates/playa-events/src/bus.rs:99`, `crates/playa-engine/src/core/global_cache.rs`, `crates/playa-engine/src/entities/comp_node.rs`).
- Don't swallow errors silently — at minimum `log::warn!` or `log::error!`.

**Tokio is not in the project**, so `anyhow` is fine to mix with `std::sync::*` and `crossbeam` channels.

---

## Logging

- `log` crate (workspace dep) for all instrumentation. `tracing` is **not** used.
- 141 call sites across 28 files use `log::{error, warn, info, debug, trace}!`.
- `env_logger` initialized in `crates/playa-app/src/runner.rs`; CLI flags `-v..-vvv` map to `warn`/`info`/`debug`/`trace`. Default level is `info` for `playa::*`, lower for deps.
- Module-level `use log::{info, trace};` (only what's used) is the prevailing import style — see `crates/playa-engine/src/core/cache_man.rs:8`, `crates/playa-engine/src/entities/frame.rs:28`.
- Use the level table from `AGENTS.md`:
  - `error!` — broken state, user/recovery required.
  - `warn!` — degraded but recovered.
  - `info!` — lifecycle events (cache init, project load, encode start).
  - `debug!` — developer context.
  - `trace!` — hot-path detail (frame load, epoch tick, evict).

---

## Naming Conventions

Follows RFC 430 with a project-specific bias toward **short, domain-appropriate names** (per CLAUDE.md / RUST.md):

| Item | Style | Examples |
|------|-------|----------|
| Types / enums / traits | `UpperCamelCase` | `CacheManager`, `FrameStatus`, `PixelBuffer`, `NodeKind`, `TonemapMode`, `DebouncedPreloader`, `GpuBlendBridge` |
| Functions / methods | `snake_case`, prefer **short** names | `mark_dirty`, `take_dirty`, `mem`, `tick`, `compose_internal`, `try_claim_for_loading`, `set_event_emitter`, `convolve_axis` |
| Constants | `UPPER_SNAKE_CASE` | `MAX_QUEUE_SIZE`, `A_WIDTH`, `A_HEIGHT`, `*_SCHEMA`, `FLAG_DAG`, `FLAG_DISPLAY` |
| Modules | `snake_case` (single word where possible) | `cache_man` (not `cache_manager`), `comp_node`, `gpu_blend_bridge`, `attr_schemas`, `main_events` |
| Method prefixes | `to_*` (owned), `as_*` (zero-cost), `into_*` (consuming), `from_*` (constructor), `is_* / has_*` (bool), `iter / iter_mut` | Used uniformly |
| Field names | `snake_case`, often very short (`fps`, `_in`, `_out`, `dim`, `attrs`) | per `enum_dispatch` Node trait |

**Project-specific preferences (from CLAUDE.md / RUST.md / AGENTS.md):**
- Prefer `get_tr` over `extract_translation`, `calc_bbox` over `compute_bounding_box`. Domain abbreviations (`tr`, `xform`, `bbox`, `comp`, `dim`, `_in/_out`, `fx`) are first-class — readers are expected to know CG terminology.
- Match the neighbors when you cannot decide.
- `enum_dispatch`-routed names like `fps`, `_in`, `_out`, `frame` on `NodeKind` are **forbidden** to duplicate in `impl NodeKind` blocks (they would shadow the trait dispatch).

---

## Documentation & Comments

- **Module-level rustdoc (`//!`) is required** at the top of every non-trivial file. Format observed across the codebase:
  1. One-liner purpose.
  2. **`# Why`** (or "Why:") section — when it isn't obvious.
  3. **Algorithm / data flow** — short numbered list (1, 2, 3) or ASCII flowchart.
  4. Cross-references to consumers ("Used by: …").
- Public types/functions: rustdoc with `# Examples` (often `ignore` or `no_run` to avoid pulling test fixtures), `# Parameters`, `# Returns` — see `crates/playa-engine/src/entities/effects/blur.rs`, `crates/playa-engine/src/core/cache_man.rs:39-52`, `crates/playa-engine/src/entities/frame.rs:1-26`.
- ASCII diagrams in rustdocs are encouraged for state machines, data flow, layouts (see `AGENTS.md` and `frame.rs` `FrameStatus` FSM, `core/global_cache.rs` LRU diagram). Use box-drawing `┌─┐ │ └─┘` or `+-+` and arrows `→ ▼ ▶`.
- Inline comments explain **why** (non-obvious context), not **what**. Code that translates a domain concept (CW+ vs CCW+ rotation, ZYX Euler order, TOCTOU race) **must** carry a comment explaining the intent.
- Do not add organizational comments that paraphrase the code.

---

## Cloning & Sharing

- **`Arc::clone(&x)` instead of `x.clone()`** — explicit, costs nothing, makes refcount bumps grep-able. 31 explicit `Arc::clone(...)` sites; this is the project standard for shared `Arc<T>` (`AtomicU64`, `CacheManager`, projects, frame buffers, channel ends).
- `Vec::clone` / `String::clone` / `Frame::clone` — fine to write `.clone()` (no convention to fight).
- For large pixel data, `Frame` wraps an `Arc<PixelBuffer>` so cloning a frame is O(1).

---

## Concurrency Discipline

- **No Tokio, no async runtime.** Workers are `std::thread`, queues are `crossbeam`, HTTP is `rouille` (synchronous). Don't introduce an async runtime unless there's a clear, agreed-upon need.
- Heavy work goes to `Workers::execute(job)` (`crates/playa-engine/src/core/workers.rs`). UI thread must never block on I/O / decode / compose — except the GPU blend drain which is intentionally on the UI thread (GL current).
- Shared state pattern: `Arc<RwLock<HashMap<…, Arc<Inner>>>>` — readers (workers) snapshot the outer map and immediately release the lock; the inner `Arc` lets compute run unblocked. See `Project::media`, `GlobalFrameCache.cache`.
- Locks are recovered, not panicked: `lock().unwrap_or_else(|e| e.into_inner())` for any `Mutex`/`RwLock` whose poison should not crash the app.
- Atomics: `AtomicU64` (epoch), `AtomicUsize` (memory bookkeeping), `AtomicBool` (dirty flag). `Ordering::Relaxed` is fine for counters; `AcqRel`/`Acquire` reserved for paired memory tracking and compare-exchange loops (see `cache_man.rs::free_memory`).

---

## Serde Discipline

- 77 `#[serde(skip)]` annotations across 10 files. Runtime-only fields (event emitters, channels, GL handles, cache managers, schemas, compositor backends) **must** be `#[serde(skip)]` and rebuilt after deserialization.
- The reciprocal restoration step is **mandatory**:
  - After `Project::from_json` or eframe's persisted-state load → call `project.set_event_emitter(event_bus.emitter())`.
  - Schemas attached to `Attrs` and the `cache_manager` reference must be re-wired the same way.
  - Without this, mutations silently fail to invalidate the cache (the canonical "stale frame" bug).
  - See `AGENTS.md` "Surprises and gotchas" + `crates/playa-engine/src/entities/project.rs:7-32`.
- Containers default-friendly: `#[derive(Deserialize)]` types use `#[serde(default)]` so older saved JSON survives schema growth — e.g. `ProjectPrefs` (`project.rs:44`), persistent `PlayaApp`.

---

## Mutation Discipline (Project / Comp / Layer)

This is the most important project-specific rule. From `AGENTS.md`:

- **Project state changes only via `project.modify_comp(uuid, |comp| { … })`**. The closure may set non-DAG attrs (frame, selection) without dirtying; setting DAG attrs (transform, opacity, blend mode) flips `dirty=true`, and `modify_comp` automatically emits `AttrsChangedEvent` → cache eviction + worker epoch bump.
- Direct mutation of `comp.layers.push/insert/remove`, `layer.attrs.set` from raw structs **bypasses** the schema-driven dirty bit. If you have to do it, call `comp.attrs.mark_dirty()` in the same `modify_comp` transaction. Otherwise the UI shows stale frames.
- `set_event_emitter` must be active for the auto-emit to fire (see Serde Discipline above).

---

## Schema Discipline

`crates/playa-engine/src/entities/attr_schemas.rs` is the **single source of truth** for which attributes invalidate the render cache. Every `Attrs`-bearing type has a `*_SCHEMA` constant.

Required when adding any attribute:

1. Add a row in the relevant schema (`PROJECT_SCHEMA`, `COMP_SCHEMA`, `LAYER_SCHEMA`, `CAMERA_SCHEMA`, `FRAME_SCHEMA`, `FX_*_SCHEMA`, `PLAYER_SCHEMA`).
2. Pick flags carefully:
   - `FLAG_DAG` — **mandatory** for anything that affects pixels.
   - `FLAG_DISPLAY` — show in Attribute Editor.
   - `FLAG_KEYABLE` — can be animated.
   - `FLAG_READONLY` — computed, not user-set.
   - `FLAG_INTERNAL` — hide from UI.
3. The `Attrs::set` setter consults the schema and toggles `dirty` based on `FLAG_DAG`.

Compose existing schemas (`IDENTITY`, `TIMING`, `TRANSFORM`) when defining a new node schema — don't redefine common keys.

---

## Refactor / Edit Discipline

- **Minimal diff.** Don't refactor along the way. No formatting-only commits.
- Match the surrounding style and naming when in doubt.
- New `NodeKind`: scaffold per `AGENTS.md` "Adding a NodeKind" (5 steps: file, enum variant, schema, `is_renderable/is_listed`, `add_child_layer` if needed).
- New event: scaffold per `AGENTS.md` "Adding an event" (4 steps: struct in `*_events.rs`, `event_bus.emit`, `downcast_event` handler in `crates/playa-app/src/main_events.rs::handle_app_event` or `app/events.rs::handle_events`, mutate via `modify_comp`).
- New effect: scaffold per `AGENTS.md` "Adding an effect" (5 steps: `entities/effects/foo.rs` with `apply(&Frame, &Attrs) → Option<Frame>`, `EffectType` variant, `FX_FOO_SCHEMA` with `FLAG_DAG | FLAG_DISPLAY | FLAG_KEYABLE`, match arms in `effects::schema()` and `effects::apply()`).

---

## Pixel-Format DRY Pattern

U8 / F16 / F32 branches must **not** duplicate business logic. The convention across `entities/effects/blur.rs`, `entities/effects/brightness.rs`, `entities/effects/hsv.rs`, `entities/transform.rs`:

1. **Decode** each branch's pixel into `f32` (with `to_f32()` / `as f32 / 255.0`).
2. Call a **single shared `f32` core function** for the actual math.
3. **Encode** back to the original pixel format (`F16::from_f32`, `(v * 255.0) as u8`, identity for `f32`).

Example (`effects/blur.rs:65-107`): `to_f32_buffer` and `from_f32_buffer` wrap an `f32` separable convolution. Don't write three near-identical copies of an algorithm — refactor through `f32`.

For per-pixel transforms, see `transform::sample_bilinear<T>(decode: impl Fn(T) -> f32)` with a `rayon` macro for the parallel arms.

---

## CLI / Config Conventions

- `clap = { workspace = true, features = ["derive"] }` is the standard. Subcommand and option doc-comments become `--help` text — keep them user-facing, not developer-facing.
- App config paths flow through `crates/playa-app/src/config.rs` with three-priority resolution: CLI `--config-dir` → `PLAYA_CONFIG_DIR` env → `dirs-next` platform default. New config knobs go through this helper, never `std::env::var` directly.
- `Path` / `PathBuf` everywhere; never hardcode separators.

---

## Imports

- Group order observed: `std`, then external crates, then `super::` / `crate::`. Blank line between groups is common but not strictly enforced.
- Import only what is used: `use log::{info, trace};` not `use log::*`.
- Re-exports flow through `lib.rs` of each crate; the root `playa` crate aggregates them. Cross-crate symbols come from `playa_engine::entities::…` etc., not from internal module paths.

---

## What's Explicitly Forbidden

- `unsafe` blocks without a `// SAFETY:` comment (none exist outside auto-generated code; keep it that way).
- Adding Tokio or any `async` runtime.
- Modifying `Comp.layers` directly without `mark_dirty()`.
- Duplicating `fps / _in / _out / frame` in `impl NodeKind` (would shadow `enum_dispatch`).
- Simplifying `(**event).as_any()` to `event.as_any()` in `event_bus::downcast_event` (would route through `Box<dyn Event>`'s blanket impl and break dispatch).
- Custom LRU implementations on `IndexSet` (use `lru::LruCache`).
- Force-killing `bun` processes (per global CLAUDE.md).

---

*Convention analysis: 2026-05-09*
