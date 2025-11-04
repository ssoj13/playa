# Changelog

## Unreleased

- Performance: faster placeholder initialization
  - Optimized `Frame::new()` to fill the placeholder RGBA pattern using chunked writes instead of repeated `extend_from_slice` calls.

- Sequence patterns: support for `%0Nd`
  - `Sequence::new()` now accepts printf-style patterns (e.g., `render.%04d.exr`) by internally globbing with `*` for discovery while retaining the original pattern for formatting.

- Centralized processing of loaded frames
  - Removed `Cache::get_frame()` internal call to `process_loaded_frames()`; processing now occurs in the UI loop only to reduce lock contention and duplicate polling.

- Timeslider: more reliable load-indicator invalidation
  - Added a monotonic `loaded_events_counter` to `Cache`, incremented on each successful frame load.
  - `timeslider` now rebuilds its cached frame-status map when any of the following change: cached frame count, loaded events counter, or sequences version.

- Clearer error on re-load after failure
  - `Frame::load()` no longer reports `UnsupportedFormat("Previously failed")` for prior errors.
  - It returns `FrameError::Image("Previously failed")`, avoiding misleading messaging and setting the stage for richer error propagation later.

- Simplified progress tracking (removed terminal progress dependency)
  - Replaced `indicatif`-based progress implementation with a lightweight in-memory tracker.
  - Removed `indicatif` from `Cargo.toml` dependencies.
  - UI progress remains via the existing `status_bar` progress bar.

---

Notes:
- These changes are internal/behavioral with no user-facing UI changes except the more responsive load indicator.
- If desired, we can expose the new counters via diagnostics or settings in a future update.
