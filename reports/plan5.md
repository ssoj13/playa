# Bug Report: Green Screen Order-Dependent Rendering Issue

## Summary

The rendering system has an order-dependent initialization bug where viewing clips/comps in different orders produces different results:

- **Working flow:** Clip first -> Comp -> Both render correctly
- **Broken flow:** Comp first -> Clip -> Green placeholder everywhere

## Root Causes Identified

### Problem 1: `update_comp()` vs `add_comp()` - Missing Cache Injection

**Location:** `src/main.rs:238` in `load_sequences()`

```rust
// CURRENT (BROKEN):
self.player.project.update_comp(comp);  // Does NOT set global_cache!

// vs add_comp() which properly injects cache:
pub fn add_comp(&mut self, mut comp: Comp) {
    if let Some(ref manager) = self.cache_manager {
        comp.set_cache_manager(Arc::clone(manager));  // Missing in update_comp!
    }
    if let Some(ref cache) = self.global_cache {
        comp.set_global_cache(Arc::clone(cache));      // Missing in update_comp!
    }
    ...
}
```

**Impact:** Clips added via `load_sequences()` never receive `global_cache`, breaking background preload entirely.

---

### Problem 2: `enqueue_frame()` Early Return Without Cache

**Location:** `src/entities/comp.rs:690-697`

```rust
fn enqueue_frame(...) {
    // Skip if already in global cache
    if let Some(ref global_cache) = self.global_cache {
        if global_cache.contains(&self.uuid, frame_idx) {
            return;
        }
    } else {
        return;  // <-- SILENT FAILURE: No global_cache = no preload!
    }
    ...
}
```

**Impact:** When a Comp has no `global_cache` (due to Problem 1), background preload silently aborts. No frames are ever queued for loading.

---

### Problem 3: `get_file_frame()` Returns Unloaded Placeholder

**Location:** `src/entities/comp.rs:853-866`

```rust
fn get_file_frame(&self, frame_idx: i32, project: &super::Project) -> Option<Frame> {
    ...
    // Cache miss: load frame from disk
    let frame_path = self.resolve_frame_path(seq_frame).unwrap_or_default();

    let frame = self.frame_from_path(frame_path);  // Creates UNLOADED placeholder!

    // Insert into global cache with frame_idx as key
    if let Some(ref global_cache) = project.global_cache {
        global_cache.insert(&self.uuid, frame_idx, frame.clone());  // Caches PLACEHOLDER!
    }

    Some(frame)  // Returns unloaded green frame!
}
```

The `frame_from_path()` method creates a green placeholder via `Frame::new_unloaded()` but **never calls `frame.load()`**:

```rust
fn frame_from_path(&self, path: PathBuf) -> Frame {
    let (w, h) = self.dim();
    let frame = Frame::new_unloaded(path);  // 1x1 green placeholder
    frame.crop(w, h, CropAlign::LeftTop);   // Expands to w x h green
    frame  // Returns WITHOUT loading pixels!
}
```

**Impact:** On synchronous cache miss, a green placeholder is cached and returned. The frame is never actually loaded from disk.

---

## Why Order Matters

### Scenario A: "Clip First" (Works)

1. User opens clip in Project panel
2. `set_active_comp(clip_uuid)` triggers `on_activate()` -> `set_current_frame()`
3. `CompEvent::CurrentFrameChanged` -> `enqueue_frame_loads_around_playhead()`
4. `signal_preload()` -> `enqueue_frame()` checks `self.global_cache`
5. **BUG:** Clip has no `global_cache` -> early return, no preload
6. Viewport calls `get_current_frame()` -> `get_file_frame()`
7. Uses `project.global_cache` (not `self.global_cache`)
8. Creates placeholder, caches it, returns green frame
9. **BUT:** Something in this flow triggers actual loading (need to trace)

### Scenario B: "Comp First" (Broken)

1. User opens Layer mode Comp first
2. `compose()` iterates children, calls `source.get_frame()` for each child clip
3. Child clip has no `global_cache` (Problem 1)
4. `get_file_frame()` creates placeholder via `frame_from_path()` (Problem 3)
5. Placeholder cached in `project.global_cache`
6. Green frame returned to compositor
7. Comp result = composited green frames
8. User navigates to child clip -> cache hit returns cached green placeholder
9. **Everything stays green forever**

---

## Unified Fix: Single Point of Initialization

### Architecture Analysis

There are TWO methods for adding comps to Project:

| Method | Signature | Cache Injection | comps_order |
|--------|-----------|-----------------|-------------|
| `add_comp()` | `&mut self` | YES | YES |
| `update_comp()` | `&self` | NO | NO |

**Semantic difference:**
- `add_comp()` - add NEW comp (injects cache + adds to order)
- `update_comp()` - UPDATE existing comp (no cache change needed)

**Current misuse:** `update_comp()` is used to add NEW comps in 2 places:

1. `main.rs:238` - `load_sequences()`
2. `main.rs:1324` - "New Comp" creation

Both manually call `comps_order.push()` after `update_comp()`, which is a code smell indicating they should use `add_comp()`.

---

## Fix Plan

### Fix 1 (CRITICAL): Use `add_comp()` for all new comps

**Location 1:** `src/main.rs:238` in `load_sequences()`

```rust
// BEFORE (broken):
self.player.project.update_comp(comp);
self.player.project.comps_order.push(uuid.clone());

// AFTER (fixed):
self.player.project.add_comp(comp);
// NOTE: add_comp() handles comps_order internally - remove manual push!
```

**Location 2:** `src/main.rs:1324` in "New Comp" creation

```rust
// BEFORE (broken):
comp.set_event_sender(self.comp_event_sender.clone());
self.player.project.update_comp(comp);
self.player.project.comps_order.push(uuid.clone());

// AFTER (fixed):
comp.set_event_sender(self.comp_event_sender.clone());
self.player.project.add_comp(comp);
// NOTE: add_comp() handles comps_order internally - remove manual push!
```

**Why this is the unified solution:**
- `add_comp()` is the SINGLE point of initialization for new comps
- It handles: cache_manager injection, global_cache injection, media insert, comps_order push
- No code duplication, no manual cache injection needed

---

### Fix 2: Add logging/warning in `enqueue_frame()` for missing cache

**File:** `src/entities/comp.rs`, `enqueue_frame()` method

```rust
fn enqueue_frame(...) {
    if let Some(ref global_cache) = self.global_cache {
        if global_cache.contains(&self.uuid, frame_idx) {
            return;
        }
    } else {
        log::warn!(
            "enqueue_frame called but comp {} has no global_cache - preload disabled",
            self.uuid
        );
        return;
    }
    ...
}
```

This helps diagnose issues without changing behavior.

---

### Fix 3: Synchronous loading fallback in `get_file_frame()`

**File:** `src/entities/comp.rs`, `get_file_frame()` method

Option A: Load synchronously on cache miss (blocks UI, but works):
```rust
fn get_file_frame(&self, frame_idx: i32, project: &super::Project) -> Option<Frame> {
    ...
    // Cache miss: load frame from disk
    let frame_path = self.resolve_frame_path(seq_frame)?;

    let frame = self.frame_from_path(frame_path.clone());

    // SYNCHRONOUSLY load pixels (fixes green screen issue)
    if let Err(e) = frame.load() {
        log::warn!("Sync load failed for {}: {:?}", frame_path.display(), e);
        return Some(self.placeholder_frame());
    }

    // Insert loaded frame into cache
    if let Some(ref global_cache) = project.global_cache {
        global_cache.insert(&self.uuid, frame_idx, frame.clone());
    }

    Some(frame)
}
```

Option B: Don't cache unloaded frames, let preload handle it:
```rust
fn get_file_frame(&self, frame_idx: i32, project: &super::Project) -> Option<Frame> {
    ...
    // Cache miss: return placeholder WITHOUT caching
    // Preload will eventually fill the cache with loaded frame
    let frame_path = self.resolve_frame_path(seq_frame)?;
    Some(self.frame_from_path(frame_path))
    // DO NOT cache unloaded frame!
}
```

---

### Fix 4: Use `project.global_cache` in `enqueue_frame()` as fallback

**File:** `src/entities/comp.rs`, `enqueue_frame()` method

```rust
fn enqueue_frame(&self, workers: &Arc<Workers>, project: &super::Project, epoch: u64, frame_idx: i32) {
    // Use self.global_cache if available, fallback to project.global_cache
    let global_cache = self.global_cache.clone()
        .or_else(|| project.global_cache.clone());

    let Some(global_cache) = global_cache else {
        log::warn!("No global_cache available for comp {}", self.uuid);
        return;
    };

    if global_cache.contains(&self.uuid, frame_idx) {
        return;
    }

    match self.mode {
        CompMode::File => {
            // ... use global_cache instead of self.global_cache.as_ref().unwrap()
        }
        CompMode::Layer => {
            // ... use global_cache instead of self.global_cache.as_ref().unwrap()
        }
    }
}
```

---

## Recommended Fix Order

1. **Fix 1 (Critical):** Replace `update_comp` with `add_comp` in both locations - this is the root cause
2. **Fix 2 (Diagnostics):** Add warning log for missing cache - helps catch future regressions
3. **Fix 3 Option A (Safety net):** Add synchronous load fallback - prevents green screen even if preload fails
4. **Fix 4 (Robustness):** Use `project.global_cache` as fallback in `enqueue_frame`

---

## Verification: Unified Initialization Points

After fix, cache initialization happens in EXACTLY these places:

| When | Where | Method |
|------|-------|--------|
| New comp added | `Project::add_comp()` | Injects cache before insert |
| Project loaded from JSON | `Project::rebuild_with_manager()` | Creates cache, calls `rebuild_runtime()` |
| After deserialization | `Project::rebuild_runtime()` | Sets cache for all existing comps |

**`update_comp()` should NEVER be used for new comps** - it's only for updating existing comps where cache is already set.

---

## Testing Checklist

After applying fixes:

- [ ] Load clip, scrub timeline -> frames load correctly
- [ ] Create new Comp, add clip as layer -> clip frames visible in Comp
- [ ] **Open Comp FIRST** before viewing clip -> no green screen
- [ ] Switch between Comp and Clip repeatedly -> consistent rendering
- [ ] Load project from JSON -> all frames render correctly
- [ ] Check cache stats in status bar -> hits/misses reasonable

---

## Files Modified

1. `src/main.rs` (2 changes):
   - Line 238: `load_sequences()` - replace `update_comp` + `comps_order.push` with `add_comp`
   - Line 1324: "New Comp" - replace `update_comp` + `comps_order.push` with `add_comp`
2. `src/entities/comp.rs` (optional safety nets):
   - `enqueue_frame()` - add warning log for missing cache
   - `get_file_frame()` - add sync load fallback OR don't cache unloaded frames
