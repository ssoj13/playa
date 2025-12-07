# Playa Bug Hunt Report - December 6, 2025

## Executive Summary

Comprehensive code analysis of the Playa project (Rust image sequence/video player with compositing).
**Total issues found: 65** across all modules.

| Severity | Count | Action Required |
|----------|-------|-----------------|
| CRITICAL | 5     | Immediate fix   |
| HIGH     | 12    | Fix ASAP        |
| MEDIUM   | 25    | Plan for sprint |
| LOW      | 23    | Backlog         |

Build status: **CLEAN** (0 warnings after recent fixes)

---

## Data Flow Architecture

```
                                   +------------------+
                                   |    PlayaApp      |
                                   | (main.rs:1200+)  |
                                   +--------+---------+
                                            |
              +-----------------------------+-----------------------------+
              |                             |                             |
     +--------v--------+          +---------v---------+          +--------v--------+
     |     Player      |          |     Project       |          |    EventBus     |
     | (player.rs)     |          | (project.rs)      |          | (event_bus.rs)  |
     | JKL, FPS, Loop  |          | Arc<RwLock<media>>|          | crossbeam queue |
     +--------+--------+          +---------+---------+          +--------+--------+
              |                             |                             |
              |                   +---------v---------+                   |
              |                   |       Comp        |                   |
              |                   | (comp.rs)         |                   |
              +------------------>| File/Layer mode   |<------------------+
                                  | children + attrs  |
                                  +---------+---------+
                                            |
              +-----------------------------+-----------------------------+
              |                             |                             |
     +--------v--------+          +---------v---------+          +--------v--------+
     |   GlobalCache   |          |    Compositor     |          |     Workers     |
     | (global_cache)  |          | CPU/GPU blend     |          | work-stealing   |
     | LRU + memory    |          +---------+---------+          | epoch cancel    |
     +--------+--------+                    |                    +--------+--------+
              |                   +---------v---------+                   |
              |                   |      Frame        |                   |
              +------------------>| (frame.rs)        |<------------------+
                                  | Rgba8/F16/F32     |
                                  +-------------------+

UI Widgets:
+------------+  +------------+  +------------+  +------------+
|  Viewport  |  |  Timeline  |  |  Project   |  |   Encode   |
| GPU render |  | layers/pan |  | media list |  | H264/H265  |
+------------+  +------------+  +------------+  +------------+
```

---

## CRITICAL Issues (5)

### C1. Workers: Threads Not Joined on Drop
- **File**: `src/core/workers.rs:191-198`
- **Impact**: Thread handles dropped without join - orphaned threads continue running, can access freed memory
- **Fix**: Add `handle.join()` loop in Drop impl

### C2. GlobalFrameCache: Deadlock Risk
- **File**: `src/core/global_cache.rs:136-156`
- **Impact**: `get()` holds cache lock during O(n) LRU scan, blocking ALL cache operations
- **Fix**: Release cache lock before LRU update, or use proper LRU structure (LinkedHashMap)

### C3. GPU Compositor: Double Texture Delete
- **File**: `src/entities/gpu_compositor.rs:753-771`
- **Impact**: `result_texture` may be deleted twice (once in loop, once explicitly) - OpenGL errors
- **Fix**: Track textures separately, clear vec before final cleanup

### C4. Renderer: Unchecked Texture Unwrap
- **File**: `src/widgets/viewport/renderer.rs:338, 383`
- **Impact**: `texture.unwrap()` panics if `gl.create_texture()` failed silently via `.ok()`
- **Fix**: Pattern match with error handling instead of unwrap

### C5. Encoder: Thread Handle Leak on Timeout
- **File**: `src/dialogs/encode/encode_ui.rs:603-612`
- **Impact**: JoinHandle dropped without join after 2s timeout - orphaned encoding thread
- **Fix**: Store handle for background cleanup, or use proper cancellation

---

## HIGH Priority Issues (12)

### H1. Duplicate Event Handlers
- **File**: `src/main_events.rs:616-631` and `863-881`
- **Description**: `SelectAllLayersEvent` and `ClearLayerSelectionEvent` handled TWICE. First handlers don't set `layer_selection_anchor`, second (complete) handlers are dead code.
- **Fix**: Remove first duplicate handlers (lines 616-631)

### H2. GPU Texture Leak in Error Path
- **File**: `src/entities/gpu_compositor.rs:753-771`
- **Description**: If `blend_textures` fails midway, intermediate textures leak
- **Fix**: Use RAII pattern for texture cleanup

### H3. Player: Division by Zero Risk
- **File**: `src/core/player.rs:468-479`
- **Description**: `range_size` could be 0 or negative if `play_end == play_start - 1`
- **Fix**: Add guard: `if range_size <= 0 { return; }`

### H4. EventBus: Unbounded Queue Growth
- **File**: `src/core/event_bus.rs:112-113`
- **Description**: Queue grows without limit - memory exhaustion possible
- **Fix**: Add capacity limit with oldest-event eviction

### H5. Encoder Binary: Always Passes None for active_comp
- **File**: `src/bin/encoder.rs:112`
- **Description**: Encoding can never start because `ready_to_encode` check is always false
- **Fix**: Pass actual active comp from player

### H6. SwsContext: Unwrap on Option
- **File**: `src/dialogs/encode/encode.rs:1568+`
- **Description**: Multiple `.unwrap()` on `self.ctx` - panics if None
- **Fix**: Return `Err()` or ensure ctx is never None

### H7. Missing Cancellation Checks in Encoding
- **File**: `src/dialogs/encode/encode.rs:960-1000`
- **Description**: No cancel check between CPU-heavy operations (crop, tonemap)
- **Fix**: Add `cancel_flag.load()` between major steps

### H8. Silent Progress Channel Failures
- **File**: `src/dialogs/encode/encode.rs:586, 610, 909+`
- **Description**: `progress_tx.send()` results discarded - thread continues if UI closed
- **Fix**: Check send result, return Cancelled if disconnected

### H9. Lock Poisoning Panics (Multiple)
- **Files**: `event_bus.rs:92+`, `global_cache.rs`, `project_ui.rs:87`, `status.rs:118`
- **Description**: `.expect("lock poisoned")` causes cascading panics
- **Fix**: Use `unwrap_or_else(|e| e.into_inner())` or graceful degradation

### H10. GlobalFrameCache: Memory Tracking Race
- **File**: `src/core/global_cache.rs:180-205`
- **Description**: Gap between frame insert and memory tracking - double-counting possible
- **Fix**: Hold both locks together

### H11. Hotkey F2 Documentation Mismatch
- **File**: `src/ui.rs:55` vs `src/main.rs:589`
- **Description**: Help says "F2 = Project panel" but actually triggers `ClearLayerSelectionEvent`
- **Fix**: Update help text or reassign hotkeys

### H12. Clone for CompositorType Loses GPU
- **File**: `src/entities/compositor.rs:37-42`
- **Description**: Clone always returns CPU variant, silently downgrades GPU
- **Fix**: Make !Clone and use Arc, or document prominently

---

## MEDIUM Priority Issues (25)

### M1. CacheManager: Relaxed Ordering Race
- **File**: `src/core/cache_man.rs:94-96`
- **Description**: Two atomics loaded with Relaxed, may see inconsistent state

### M2. GlobalFrameCache: O(n) LRU Operations
- **File**: `src/core/global_cache.rs:148-150`
- **Description**: `VecDeque` with `position()` + `remove()` is O(n)

### M3. Workers: Stealers Field Dead Code
- **File**: `src/core/workers.rs:34-35`
- **Description**: Field stored but never used after spawn

### M4. GlobalFrameCache: Capacity Field Unused
- **File**: `src/core/global_cache.rs:102-104`
- **Description**: Stored but never used for eviction

### M5. Unused `_radius` Parameter
- **File**: `src/main.rs:265`
- **Description**: Parameter not used in function

### M6. Double `handle_events()` Call
- **File**: `src/main.rs:1007, 1097`
- **Description**: Called twice per frame, may double-process events

### M7. Shell: Only Last Event Result Returned
- **File**: `src/shell.rs:84-124`
- **Description**: Multiple event results overwritten, only last returned

### M8. RefCell in Project Not Thread-Safe
- **File**: `src/entities/project.rs:46`
- **Description**: `RefCell<CompositorType>` is `!Sync`, can't share between threads

### M9. Integer Overflow Risk in Digit Calculation
- **File**: `src/dialogs/encode/encode_ui.rs:100-103`
- **Description**: Float log10 for digit counting has precision issues

### M10. Settings Not Persisted in Standalone Prefs Binary
- **File**: `src/bin/prefs.rs:31`
- **Description**: `changes_made` tracked but never saved

### M11. Dead Code - `frozen_image_size` Field
- **File**: `src/widgets/viewport/viewport.rs:353+`
- **Description**: Set but never read

### M12. Dead Code - Multiple Viewport Methods
- **File**: `src/widgets/viewport/viewport.rs:174-495`
- **Description**: ~8 methods marked dead_code or never called

### M13. GPU Texture Caching TODO
- **File**: `src/entities/gpu_compositor.rs:211`
- **Description**: Field exists but feature not implemented

### M14. Canvas-Sized Blending TODO
- **File**: `src/entities/gpu_compositor.rs:784`
- **Description**: Currently blends then crops, inefficient

### M15. Work-Stealing TODO
- **File**: `src/core/workers.rs:34`
- **Description**: Already works via clones, field is leftover

### M16. Capacity-Based Eviction TODO
- **File**: `src/core/global_cache.rs:102`
- **Description**: Only memory-based eviction implemented

### M17. Thread-Local Compositor May Miss GPU Check
- **File**: `src/entities/comp.rs:77-79`
- **Description**: No runtime assertion for `use_gpu=false` from background thread

### M18. Division by Zero Guard Returns Arbitrary Value
- **File**: `src/entities/comp.rs:676-678`
- **Description**: Returns `child_start` on near-zero speed instead of proper handling

### M19. Corrupted Russian Comment
- **File**: `src/widgets/viewport/viewport.rs:79`
- **Description**: Mojibake encoding issue

### M20. Deprecated CLI Arguments Still Processed
- **File**: `src/cli.rs:61-67` vs `src/main.rs:1377-1394`
- **Description**: Marked deprecated but still used

### M21. Continuous Repaint in Project Binary
- **File**: `src/bin/project.rs:250`
- **Description**: Unconditional `request_repaint()` wastes CPU

### M22. Unused Container Format Variable
- **File**: `src/dialogs/encode/encode.rs:622-625`
- **Description**: Computed but never used

### M23. Hardcoded Emoji in UI
- **File**: `src/dialogs/encode/encode_ui.rs:999`
- **Description**: May not render on all systems

### M24. Debug Assertion Performance Impact
- **File**: `src/widgets/ae/ae_ui.rs:105-106`
- **Description**: Iterates all attrs just for debug assertion

### M25. Inconsistent Error Handling in load_sequences
- **File**: `src/main.rs:212-258`
- **Description**: Returns Result but callers ignore with `let _ =`

---

## LOW Priority Issues (23)

### L1-L7: Dead Code Annotations
- `frame.rs:57,67,69,228,235,251` - PixelDepth variants, constructors
- `loader_video.rs:26` - fps field (actually used, remove annotation)
- `utils.rs:15,47` - utility functions
- `gpu_compositor.rs:212` - texture_cache

### L8-L12: Missing Features
- No unit tests in loader.rs
- Missing Default impl for HotkeyHandler
- Incomplete change detection in prefs binary
- StatusBar.update() does nothing

### L13-L17: Minor Code Issues
- Hardcoded test path `D:\_demo\Srcs\Kz\kz.0000.tif` (shell.rs:238)
- Redundant clone on Option<Uuid> (timeline_ui.rs:1121)
- Magic numbers in timeline rendering
- Silent shader directory error
- Player: FPS presets not documented

### L18-L23: Style/Consistency
- EventBus callback order not documented
- Empty lines after methods
- Flatten vs and_then pattern
- Unused parameters with underscore prefix
- Dead code annotations that could be removed

---

## Recommendations

### Immediate Actions (This Sprint)
1. **Fix C1**: Add thread join in Workers::Drop
2. **Fix C2**: Release cache lock before LRU operations
3. **Fix C3**: Fix GPU texture double-delete
4. **Fix C4**: Handle texture creation failure
5. **Fix H1**: Remove duplicate event handlers

### Short-Term (Next 2 Sprints)
1. Fix all HIGH issues (H2-H12)
2. Implement queue bounds in EventBus
3. Add cancellation checks in encoder
4. Fix lock poisoning with graceful degradation

### Medium-Term
1. Replace VecDeque LRU with LinkedHashMap for O(1)
2. Clean up dead code (stealers, capacity, viewport methods)
3. Implement GPU texture caching
4. Add missing tests for loader module

### Long-Term
1. Document thread-safety model
2. Consider proper error propagation vs panics
3. Implement canvas-sized blending
4. Complete work-stealing implementation

---

## Files Changed Summary

| Module | Files | Critical | High | Medium | Low |
|--------|-------|----------|------|--------|-----|
| core/ | 6 | 2 | 4 | 5 | 2 |
| entities/ | 9 | 1 | 2 | 5 | 8 |
| widgets/ | 20 | 1 | 2 | 5 | 5 |
| dialogs/ | 6 | 1 | 3 | 4 | 3 |
| main app | 8 | 0 | 1 | 6 | 5 |

---

## Approval Required

This plan requires approval before implementation.

**Estimated effort**:
- CRITICAL fixes: 4-6 hours
- HIGH fixes: 8-12 hours
- MEDIUM fixes: 16-24 hours
- Total: 28-42 hours

---

*Report generated by Claude Code Bug Hunt - December 6, 2025*
