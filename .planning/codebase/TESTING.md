# Testing Patterns

**Analysis Date:** 2026-05-09
**Scope:** Workspace `playa` (5 lib crates + `xtask` + binary). All counts derived from grep over `crates/`.

---

## Test Framework

| Item | Value |
|------|-------|
| Runner | Built-in `cargo test` (libtest harness) |
| Assertion library | Built-in `assert!`, `assert_eq!`, `assert_ne!`, `assert!(x.is_some())`, `assert!((a - b).abs() < eps)` |
| Property testing | **Not used** (no `proptest`, `quickcheck`) |
| Snapshot testing | **Not used** (no `insta`) |
| Mocking | **Not used** (no `mockall`, `wiremock`, `mockito`) |
| Async testing helpers | **Not used** — there is no async runtime in the project |
| Serial / parallel control | **Not used** (no `serial_test`) |
| Parameterized tests | **Not used** (no `rstest`) |
| Doc tests | Present in rustdoc but most are gated `ignore` or `no_run` (loaders need fixtures, GL contexts, etc.) — see `crates/playa-engine/src/core/cache_man.rs:48` (`no_run`), `crates/playa-engine/src/entities/effects/blur.rs:14` (`ignore`) |
| Custom dev-deps | **None.** No crate declares `[dev-dependencies]`; tests use only what's already in `[dependencies]` |
| Coverage tooling | Not configured (no `cargo-tarpaulin`, `cargo-llvm-cov`, no `.codecov.yml`) |

The whole testing stack is the Rust standard library plus what crates already depend on (`uuid`, `half`, `glam`, `serde`, `log`).

---

## How Tests Are Run

Three equivalent entry points, in order of preference:

```bash
# 1. Repo-level wrapper (Python) — sets up vcpkg / VS env on Windows, then xtask
python bootstrap.py test

# 2. xtask wrapper — sets vcpkg env via env_setup, runs all workspace tests
cargo xtask test                # release profile (default)
cargo xtask test --debug        # debug profile
cargo xtask test --nocapture    # show println! output

# 3. Plain cargo (skip env bootstrap — only works if VCPKG_ROOT etc. already set)
cargo test --workspace
cargo test -p playa-engine
cargo test -p playa-engine effects::blur::tests::test_gaussian_kernel
```

`cargo xtask test` (`crates/xtask/src/main.rs:710`) builds the command:

```
cargo test --workspace [--release] -- [--nocapture] --show-output
```

The explicit `--workspace` is critical: the workspace's `default-members = ["."]` (`Cargo.toml:13`) means a bare `cargo test` would cover only the root re-export crate and miss every member's test module. Always use `--workspace` (or `-p <crate>`) when running tests directly.

---

## Test Layout

**Strictly inline** — every test lives in a `#[cfg(test)] mod tests { … }` block at the bottom of the source file it tests.

| Directory | Status |
|-----------|--------|
| `crates/*/tests/` (integration tests) | **Absent** — no integration test directories exist anywhere |
| `crates/*/benches/` | **Absent** — no Criterion or `#[bench]` benchmarks |
| `crates/*/examples/` | **Absent** — no example targets |
| `crates/*/test_data/` or `fixtures/` | **Absent** — no committed fixture assets |
| Doc tests | Present but mostly `ignore`/`no_run` |

Total `#[test]` count: **61** functions across **18** files. Total `#[cfg(test)] mod` blocks: **18** (one per file).

---

## Per-Crate Test Counts

| Crate | `#[test]` count | Files with tests |
|-------|----------------:|------------------|
| `playa-engine` | **49** | `entities/file_node.rs` (4), `core/global_cache.rs` (5), `entities/frame.rs` (5), `entities/camera_node.rs` (4), `entities/gpu_blend_bridge.rs` (2), `entities/comp_node.rs` (4), `entities/effects/blur.rs` (2), `core/debounced_preloader.rs` (3), `entities/effects/brightness.rs` (2), `entities/node_kind.rs` (3), `core/cache_man.rs` (3), `entities/project.rs` (5), `entities/effects/hsv.rs` (3), `entities/text_node.rs` (2), `entities/transform.rs` (2) |
| `playa-events` | **6** | `src/bus.rs` (6) — covers subscribe/emit, deferred queue, MAX_QUEUE_SIZE eviction, type-erased dispatch |
| `playa-app` | **5** | `src/config.rs` (5) — path resolution priority |
| `playa-ui` | **1** | `dialogs/encode/encode.rs` (1) |
| `playa-io` | **0** | None — feature-gated loaders not exercised |
| `xtask` | **0** | Build-tool crate, no logic to test |
| **Total** | **61** | **18 files** |

Engine logic is well covered (algorithms, math, FSM, cache); the GUI (`playa-ui`) and host (`playa-app`) are barely tested. The CLI / event router / API server have no automated tests.

---

## Test Structure (canonical pattern)

```rust
// crates/playa-engine/src/core/cache_man.rs:184
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_manager_creation() {
        let manager = CacheManager::new(0.5, 1.0);
        assert_eq!(manager.current_epoch(), 0);

        let (usage, _limit) = manager.mem();
        assert_eq!(usage, 0);
    }

    #[test]
    fn test_epoch_increment() {
        let manager = CacheManager::new(0.5, 1.0);
        let epoch1 = manager.increment_epoch();
        assert_eq!(epoch1, 1);
    }
}
```

Conventions observed across the test modules:

- Single `mod tests` at file bottom; always `use super::*;` for the symbols under test.
- Function names always start with `test_` (`test_gaussian_kernel`, `test_zero_radius_noop`, `test_cache_basic_operations`, `test_debounce_resets_timer`).
- One assertion per concept; multiple `assert!` calls per test are common.
- `unwrap()` / `expect()` are **allowed** in tests (the production-code rule does not apply).
- No setup/teardown frameworks. Each test builds the world it needs from scratch (`CacheManager::new`, `Frame::placeholder`, `Frame::new(w, h, PixelDepth::U8)`, `CompNode::new`, `Attrs::new`).
- Floating-point checks: explicit epsilon (`(sum - 1.0).abs() < 0.001`) — never `assert_eq!` on `f32`.

---

## Common Patterns

### Time-based tests

`DebouncedPreloader` tests use real `std::thread::sleep` to assert timer behavior — the only place in the codebase that does so:

```rust
// crates/playa-engine/src/core/debounced_preloader.rs:127
#[test]
fn test_trigger_after_delay() {
    let mut preloader = DebouncedPreloader::new(10); // 10ms
    let uuid = Uuid::new_v4();
    preloader.schedule(uuid);
    std::thread::sleep(Duration::from_millis(15));
    assert_eq!(preloader.tick(), Some(uuid));
}
```

Tests use short delays (10–50 ms) — keep this when adding new timing tests so the suite stays fast.

### Path-conditional tests

`crates/playa-engine/src/entities/file_node.rs:520-528` tests a sequence-detection on a specific Windows path and **early-returns** when the path is missing:

```rust
#[test]
fn test_cliven_sequence() {
    let test_path = std::path::Path::new(r"D:\_demo\Srcs\Cliven\cliven.0001.TGA");
    if !test_path.exists() {
        println!("Skipping test - path not found");
        return;
    }
    // ... real assertions
}
```

This is the closest the project gets to fixtures. Treat such tests as developer-only smoke tests; CI cannot run them. New tests should not depend on hardcoded local paths.

### Cache / Arc tests

`crates/playa-engine/src/core/global_cache.rs` tests build the full `CacheManager` + `GlobalFrameCache` graph and exercise `clear_comp` (O(1) drop), LRU eviction, `LastOnly` strategy, and per-comp counts. These are de-facto **integration tests inside the unit module** — no separate `tests/` directory is needed because everything is `pub` enough to construct from `super::*`.

### Effect tests

`effects/{blur,brightness,hsv}.rs` test files build `Frame::placeholder(w, h)` or `Frame::new(w, h, PixelDepth::U8)` plus a hand-built `Attrs` containing the effect's parameters, then call `apply(&frame, &attrs)` and assert on `Option::Some` and a few pixel values. The U8 path is exercised; F16/F32 paths share the same `f32` core but are not directly asserted.

---

## Pixel Fixtures

There are **no committed image / video fixtures** anywhere in the workspace. Encoder/loader code is therefore not unit-tested for round-trip correctness — only the surrounding logic (sequence detection, `VideoMetadata::from_file` guards) has tests, and the EXR / FFmpeg paths must be exercised manually.

`AGENTS.md` notes two encoder-related bug fixes (BUG-04: `denom != 0` guard; BUG-13: `as usize` truncation in `VideoMetadata::from_file`) — neither has a regression test.

---

## CI Workflows

Located in `.github/workflows/`:

| File | Purpose |
|------|---------|
| `main.yml` | Release pipeline — triggered by `v*` tags; coordinates `wait-for-cache`, `_build-platform.yml`, `_build-backend.yml` |
| `_build-backend.yml` | Reusable workflow building per backend (vfx-exr) |
| `_build-platform.yml` | Reusable workflow per OS (windows-msvc, ubuntu, macos arm64/x64) |
| `warm-cache.yml` | Periodic vcpkg + cargo cache warmer |

**Observation:** the workflows build and package, but I see no explicit `cargo test` step in the visible portion of `main.yml`. CI is **release-driven**, not test-driven. Tests run on developer machines via `python bootstrap.py test` or `cargo xtask test`. (Confirm by inspecting the build matrix files; the snippet read here covers only the orchestration job.)

`cargo xtask wipe-wf` (`crates/xtask/src/main.rs:348`) bulk-deletes Actions runs via `gh api` — used to clean up after CI churn, not part of the test loop.

---

## Known Testing Gaps

Derived from the layout, `TODO.md`, and `AGENTS.md`:

| Gap | Notes |
|-----|-------|
| Loader round-trips (EXR / image / video) | No fixtures committed; `playa-io` has zero `#[test]` |
| GUI behaviour (`playa-ui`) | One test in the entire crate |
| Event routing (`main_events::handle_app_event`) | No test |
| REST API (`server/api.rs`) | No test |
| Encoder hardware paths (NVENC / QSV / AMF) | No test infrastructure for HW codecs |
| Project save/load round-trip | No serde-roundtrip test for `Project::to_json` / `from_json` |
| `set_event_emitter` restoration after deserialization | Not asserted; this is the canonical bug source per `AGENTS.md` |
| Coordinate-space conversions (`space::to_math_rot`, ZYX Euler) | No regression test |
| GPU compositor parity vs CPU | Documented as future work in `crates/playa-engine/src/entities/compositor.rs` rustdocs |

`TODO.md` does **not** list testing as an explicit priority — it focuses on timecode, OTIO, OCIO/OIIO, Shotgrid, headless ops, and Python bindings. There is no testing roadmap.

---

## Adding a New Test

For a new module / function:

1. Add `#[cfg(test)] mod tests { use super::*; … }` at the bottom of the source file (do **not** create a sibling `tests/` directory).
2. Function names: `test_<what>` (`test_clamp_zero_radius`, `test_attrs_dirty_after_set`).
3. `unwrap()` / `expect()` are fine in tests; production code shouldn't.
4. Avoid hardcoded paths and external resources. If a path is unavoidable, gate behind `if !path.exists() { return; }` (see `file_node.rs` pattern).
5. Run `cargo xtask test --debug --nocapture` locally before pushing — this is the canonical reproduction of the suite.

---

*Testing analysis: 2026-05-09*
