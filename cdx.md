# Cache/Auto-caching Review

## Alignment with todo3–todo5
- LRU cache in `Comp` is present and timeline load indicator is drawn, but auto-caching/session logic and memory management are not functioning as described.
- Reserve-for-system and cache-percent sliders exist, yet the limit is not actually updated at runtime.
- Spiral/forward preload paths exist, but they do not populate the real cache and do not cancel stale work.

## Problems found
- **Memory settings are no-op** (`src/main.rs:1800-1807`): the limit update uses `Arc::get_mut` on `cache_manager`, but it is shared with `player/project/workers`, so `get_mut` returns `None` and `set_memory_limit` never runs. UI sliders do nothing.
- **Memory accounting is wrong** (`src/entities/comp.rs:731-758`, `src/main.rs:375-419`): cache inserts count only the 1x1 placeholder size; actual loads via `set_status(FrameStatus::Loaded)` never call `CacheManager::add_memory`, and unloads never call `free_memory`. Usage shown in the status bar is far below real RAM usage, so eviction does not protect against OOM.
- **Background preload writes to a cloned cache** (`src/entities/comp.rs:682-715`): `self.cache.clone()` clones the `RefCell<LruCache>` contents, so workers push loaded frames into a detached copy. Frames are dropped after the closure, but `CacheManager::add_memory` has already increased usage, causing accounting leaks and no usable preloaded data.
- **Epoch cancellation missing** (`src/entities/comp.rs:523-583`): `signal_preload` reads `current_epoch` instead of incrementing it, so stale requests are never cancelled; all preloads run regardless of timeline jumps.
- **Eviction checks use stale/underreported usage** (`src/entities/comp.rs:731-758`, `src/entities/comp.rs:696-710`): eviction runs only when `check_memory_limit()` is already true. Because usage is undercounted and prospective frame size is ignored, the cache can overshoot the limit and never evict until a later insert (or churn forever if usage is miscounted).
- **CacheManager limit cannot be adjusted under Arc** (`src/cache_man.rs:27-144`): `max_memory_bytes` is a plain `usize`, so mutation requires `&mut self`. With shared `Arc`, runtime limit changes are effectively impossible; also no adjustment/eviction is triggered when the limit drops.
- **Worker threads never shut down** (`src/workers.rs:189-194`): the drop impl logs but never sets `shutdown`, so worker threads spin forever after app drop/exit.
- **Preload indicator reflects only on-demand loads**: because background preload fills a cloned cache, the timeline bar and cache_statuses reflect only frames touched via `get_frame`, reducing its usefulness.
- **Unused/dead data**: `PreloadStrategy` in `cache_man.rs` is not stored/used; duplicated preload code paths could be centralized.

## Recommendations
1) Make runtime memory limits effective:
   - Store `max_memory_bytes` in `AtomicUsize` (or guard with a lock) so it can be updated through shared `Arc`; call it directly instead of `Arc::get_mut`.
   - Use prospective size: evict while `usage + incoming > limit` (>=), not only when already over.
   - Track actual bytes on load/unload: wrap `Frame::set_status` transitions (Loaded/Header) to call `add_memory` / `free_memory` based on returned sizes; ensure evictions and `clear_cache` also update usage.
2) Fix preload/cache wiring:
   - Do not clone the `RefCell<LruCache>`; move caching behind a thread-safe handle (e.g., `Arc<Mutex<LruCache<...>>>` or a dedicated background cache) so workers insert into the real cache.
   - Increment epoch in `signal_preload` to cancel stale requests; drop queued jobs when epoch mismatches.
   - Avoid counting memory for frames that are immediately dropped; tie accounting to the real cache insertions.
3) Enforce reserve sliders:
   - Apply `set_memory_limit` directly on the shared manager and trigger an eviction pass when the limit decreases.
   - Surface the effective limit/usage in the UI after the update to confirm the change took effect.
4) Clean shutdown:
   - Set `shutdown` in `Drop` and let workers exit their loop; optionally join handles for a clean teardown.
5) Reduce duplication and improve diagnostics:
   - Keep a single preload implementation reused for file comps and layer children.
   - Add logs/metrics for cache hit rate and eviction counts to validate memory management.
