# Playa Project Audit

## Summary

Overall, the project is well-structured and thoughtfully engineered for a responsive image-sequence player. The core architecture (Cache + worker threads + epoch cancellation + spiral preloading + LRU) is solid. UI is clean and idiomatic egui, and OpenGL upload path uses PBOs for streaming.

The items below focus on correctness edge cases, performance/UX polish, and simplifications that reduce complexity without losing functionality.

## Architecture Strengths

- Clear module boundaries: `frame`, `sequence`, `cache`, `player`, `ui`, `viewport`, `shaders`.
- Concurrency model: mpsc channels + worker pool + epoch cancellation to avoid stale loads.
- Memory model: unbounded `LruCache` + explicit memory-based eviction; atomic usage tracking.
- Viewport: explicit state (`ViewportState`) with fit/100%/manual modes and cursor-centric zoom.
- Shaders: embedded default plus on-disk overrides; runtime switching with recompilation.

## Correctness and Potential Issues

- Frame.load error identity and messaging
  - Code: `src/frame.rs:221`
  - When `try_claim_for_loading()` fails with `FrameStatus::Error`, `load()` returns `FrameError::UnsupportedFormat("Previously failed")`. This message is misleading and loses the original error cause.
  - Suggestion: introduce `FrameError::Busy` or propagate a stored last-error (requires storing it in `FrameData`). Minimal fix: return `FrameError::Image("Previously failed")` to avoid “unsupported format” confusion.

- Timeslider load indicator cache invalidation
  - Code: `src/timeslider.rs:43`, `src/timeslider.rs:68`
  - The cache key uses only `cached_frames_count()`. When frame statuses change (e.g., Loading → Loaded) without changing the count, the indicator may not refresh.
  - Suggestion: include more signals in the key, e.g., `(cached_frames_count, sequences_version)` or, better, add a `cache.loaded_events_counter` that increments on every successful frame load and use it as part of the cache key.

- Pattern handling inconsistency (`*` vs `%0Nd`)
  - Code: `src/sequence.rs:95` (formatting supports `%0Nd`), but `Sequence::new()` only treats `*` as pattern. `%04d` inputs fall into `init_from_file()` where `PathBuf::exists()` will likely fail.
  - Suggestion: either (a) support `%0Nd` in `new()` by branching on `pattern.contains('%')` and using a small glob expansion, or (b) remove `%` handling from `format_path` for a single, consistent pattern style.

- Redundant polling of loaded frames
  - Code: `src/main.rs:127` (calls `self.player.cache.process_loaded_frames()`), and `src/cache.rs:556` (`get_frame()` also calls `process_loaded_frames()`).
  - This double polling is safe but unnecessary. Consider keeping it in one place (UI loop) to reduce lock contention.

- Minor UX nuance for ESC/Q
  - Code: `src/main.rs:75`–`src/main.rs:98`
  - Combined handler: ESC and Q both enter the same branch. If both are pressed (rare), the nested check prefers ESC-exit-fullscreen over quit. Acceptable, but you may want separate explicit handlers for clarity.

- Cargo edition
  - Code: `Cargo.toml:7`
  - Uses `edition = "2024"`. Ensure your CI and minimum supported Rust version actually target the 2024 edition across platforms. If you intend broader compatibility per README (Rust 1.70+), consider 2021 edition unless 2024 is confirmed in your target toolchains.

## Concurrency and Performance

- LRU + memory eviction
  - Code: `src/cache.rs:657` (O(1) `pop_lru`), `src/cache.rs:598` (accounting). Looks good. `Frame::mem()` sizes are consistent for U8/F16/F32.

- Worker pool sizing
  - Code: `src/cache.rs:137`–`src/cache.rs:149`
  - Chooses 75% of cores. Sensible default. Consider making this configurable via settings/CLI for power users.

- PBO streaming path
  - Code: `src/viewport.rs:604`–`src/viewport.rs:736`
  - Uses double PBOs, `map_buffer_range` with `MAP_WRITE_BIT`. For very large frames, you can reduce stalls with orphaning (`glBufferData(..., NULL, STREAM_DRAW)`) or `MAP_INVALIDATE_BUFFER_BIT` when appropriate.
  - Allocation churn: `F16` → `u16` temporary `Vec<u16>` each upload. Consider a scratch buffer reused across frames for fewer allocs.

- Placeholder buffer creation
  - Code: `src/frame.rs:64`
  - Uses `extend_from_slice` in a loop; you can use `vec![0, 100, 0, 255; width*height]` or `resize` for fewer iterations.

## Simplifications (No Feature Loss)

- Remove terminal progress dependency
  - Code: `src/progress.rs`
  - The `indicatif::MultiProgress` terminals won’t be visible in the GUI app and add overhead. Replace with simple counters used by the UI’s `ProgressBar` (`src/status_bar.rs`) and remove `indicatif`.
  - Impact: smaller dependency tree, less background overhead.

- Unify sequence pattern model
  - Choose `*` patterns only, drop `%0Nd` rendering, or fully support `%0Nd` inputs. Keeping one approach simplifies mental model and docs.

- Centralize `process_loaded_frames()`
  - Remove the call from `Cache::get_frame()` and keep it in the UI loop to avoid duplicate channel polling under contention.

## API/UX Opportunities

- Expose cache memory budget in settings
  - Wire `AppSettings` → `Cache` to configure memory percentage/budget at runtime.

- Add FPS and loop presets to settings
  - Persist via `AppSettings` to reflect the actual controls you already expose in the UI.

- Viewport defaults
  - Persist cinema mode and last used shader in `AppSettings` so long sessions restore exactly as left.

## Build/Release Notes

- `xtask` flow is clean and cross-platform. Linux header patching steps are well explained in `README.md`.
- Consider an optional `--workers` flag or env var to override worker count without rebuilding.

## Security/Safety Notes

- Image decoding is delegated to `image` and `openexr` crates. Those have seen fuzzing, but malformed files may still be expensive to parse.
- Consider soft limits per-frame (e.g., maximum dimension) to avoid pathological cases allocating excessive memory.

## Quick-Win Patches

- Timeslider invalidation
  - Add an atomic `loaded_events_counter` in `Cache` that increments on successful frame load in `process_loaded_frames()`. Use that value plus `sequences_version()` in the timeslider’s temp cache key (`src/timeslider.rs`) to rebuild statuses more reliably.

- Frame.load error message
  - Change the “Previously failed” branch to return a clearer error (e.g., `FrameError::Image("Previously failed")`) and consider storing last error in `FrameData` for richer UI messages later.

- Remove `indicatif` progress
  - Replace `LoadProgress` with a simple struct tracking `(loaded_count, total)` for the UI and remove terminal progress entirely.

## Nice-to-Have Improvements (Longer-Term)

- Texture upload scratch buffers per format
  - Reuse typed scratch buffers for F16/F32 conversions; avoid per-frame allocations.

- Tuning preload strategy
  - Current spiral preload is great. Consider adaptive look-ahead based on FPS and historical decode latency.

- Robust shader diagnostics
  - On shader compilation/link errors (`src/viewport.rs:71`–`src/viewport.rs:128`), offer an on-screen message (in addition to logging) so users know a shader failed.

## File References

- `src/frame.rs:221` – misleading `UnsupportedFormat("Previously failed")` on prior errors.
- `src/timeslider.rs:43`, `src/timeslider.rs:68` – indicator cache keyed only by `cached_frames_count()`.
- `src/sequence.rs:95` – `%0Nd` formatter exists but isn’t accepted in `Sequence::new()` inputs.
- `src/cache.rs:556`, `src/cache.rs:593` – double processing of loaded frames (`get_frame()` vs UI loop).
- `src/viewport.rs:604`–`src/viewport.rs:736` – PBO path; consider orphaning/invalidate flags; reduce temp allocations.
- `Cargo.toml:7` – Rust edition "2024"; verify toolchain support.

---

If you’d like, I can implement any of the quick-win patches above (timeslider invalidation, clearer error handling, progress simplification) in small, focused changes.

