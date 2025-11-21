# Frame Flow (scrub → render)

1. **Scrub/transport input**
   - UI/timeline calls `Player::set_frame(frame)` (or `to_start/to_end/step` helpers).
   - `Player::set_frame` clamps to `comp.start()..=comp.end()` and forwards to `comp.set_current_frame`.
   - `Comp::set_current_frame` updates `current_frame` and emits `CompEvent::CurrentFrameChanged`.

2. **Event handling**
   - `PlayaApp::handle_comp_events` listens for `CurrentFrameChanged` and enqueues reads via `enqueue_frame_loads_around_playhead(10)`.

3. **Enqueue frame loads**
   - Active comp resolved; File vs Layer branch.
   - **File mode:** builds window `[current±radius]` respecting play_range; fetches frames via `comp.get_frame`. Frames without `file()` are skipped (placeholders only).
   - **Layer mode:** iterates children; maps global → local with `play_start`, respects child range intersection. Skips if `frame_idx` < 0, out of `source.play_frame_count()`, or frame has no backing file.
   - Workers call `frame.set_status(Loaded)`; `Frame::load` uses the stored filename to decode.

4. **Render path**
   - Viewport asks `Player::get_current_frame` → `Comp::get_frame`.
   - **File comp:** interprets `frame_idx` as 0-based within the clip; outside work area or sequence range returns a sized green placeholder. Resolves paths via `file_mask`/padding and caches by `(comp_hash, seq_frame)`.
   - **Layer comp:** filters by play_range, recurses into children, blends via `project.compositor`.
   - Viewport uploads the resulting buffer; status drives the cache indicator.

## Issues observed (logs: `Failed to get frame {n} (frame_idx m)`)
- File comps used absolute play_range (start bound from on-disk numbering). Local frames (0-based) were rejected early, so `get_frame` returned `None` and loaders logged failures.
- Loader tried to set `Loaded` on frames without filenames, leaving permanent green placeholders.
- Comp bounds stagnated (end stayed at historical 1370) causing timeline overhang; `rebound()` was not shrinking to children.
- Drag-drop preview duplicated implementations (project drop vs layer move), leading to a misaligned bar pinned to the ruler.

## Fixes/mitigations in this pass
- `Comp::get_frame` (File) now: local work-area check, placeholder out-of-range, resolves path with padding, caches by sequence frame, sizes placeholder from comp attrs.
- Load queue skips frames without `file()`; layer loading iterates i32 ranges (supports negatives) without clamping to 0.
- `rebound()` recalculates start/end strictly from children and resets to 0/0 when empty.
- Unified drop preview via a single helper used for both project drops and internal moves.

## Residual risks / next steps
- If `file_mask` is missing or mis-parsed, placeholder will render silently; verify masks on import.
- When comp width/height attrs are absent, placeholders fall back to 64×64; consider propagating metadata earlier.
- Timeline pan is not auto-recentered when comp bounds change; confirm expected UX for negative starts.
