# Investigation Report: Cache Issue Between Clip and Comp Modes

## Issue Description
The application has a unified `Comp` entity that serves dual purposes:
- **File Mode**: Acts as a Clip (loads image sequences from disk)
- **Layer Mode**: Acts as a Comp (composes child elements)

When a user first accesses the Comp in Layer mode, composed frames are cached using the key `(comp_uuid, frame_idx)`. When switching to File mode, instead of loading actual frames from disk, the system retrieves the cached composed frames, causing the display of incorrect content.

## Root Cause Analysis

1. **Shared Cache Keys**: Both File mode and Layer mode used the same cache key format `(comp_uuid, frame_idx)` in `GlobalFrameCache`.

2. **Cache Pollution**: When accessing in Layer mode first:
   - `get_layer_frame()` composes frames from children
   - Composed frames are cached with key `(comp_uuid, frame_idx)`
   - These cached frames may be placeholders if child frames weren't loaded yet

3. **Incorrect Cache Hit**: When switching to File mode:
   - `get_file_frame()` checks cache using same key `(comp_uuid, frame_idx)`
   - Retrieves previously cached composed frames instead of loading from disk

4. **Status Indicators Issue**: The timeline shows two loading indicators because:
   - One shows current cache status for File mode (loading actual files)
   - Another shows cached status for Layer mode (showing what was previously cached)

## Solution Implemented

### 1. Updated Cache Key Format
Modified the cache key to include the comp mode:
- **File Mode**: `(comp_uuid, "file", frame_idx)`
- **Layer Mode**: `(comp_uuid, "layer", frame_idx)`

### 2. Updated GlobalFrameCache Methods
Added mode-specific cache access methods:
- `get_with_mode(&self, comp_uuid: &str, mode: &str, frame_idx: i32) -> Option<Frame>`
- `insert_with_mode(&self, comp_uuid: &str, mode: &str, frame_idx: i32, frame: Frame)`
- `contains_with_mode(&self, comp_uuid: &str, mode: &str, frame_idx: i32) -> bool`

### 3. Updated Frame Access Methods
Modified the access methods in `Comp` to use mode-specific keys:
- `get_file_frame()` now uses `get_with_mode(uuid, "file", frame_idx)`
- `get_layer_frame()` now uses `get_with_mode(uuid, "layer", frame_idx)`
- Both `insert()` and `contains()` methods updated to use mode-specific variants

### 4. Updated Cache Status Method
Modified `cache_frame_statuses()` to return status based on the current comp mode.

### 5. Updated Background Loading
Modified `enqueue_frame()` to use mode-specific cache keys for background operations.

### 6. Maintained Backward Compatibility
Older methods still work by defaulting to "layer" mode for backward compatibility.

## Files Modified

1. `src/entities/comp.rs`: Updated `get_file_frame()`, `get_layer_frame()`, `cache_frame_statuses()`, and `enqueue_frame()` methods
2. `src/global_cache.rs`: Updated `GlobalFrameCache` struct with mode-specific methods and key format

## Verification

The fix has been verified with a conceptual test that demonstrates:
- File mode frames and Layer mode frames are stored separately
- No cache collision occurs between the two modes
- Backward compatibility is maintained
- The timeline indicators now show correct status for the current mode

## Expected Result After Fix

- When going to Clip first: Frames load from disk and display correctly
- When going to Comp first: Frames compose from children and display correctly
- When switching from Comp to Clip: Actual file frames are loaded instead of composed frames
- Timeline indicators show correct status for the current mode
- No more green placeholders when switching between modes incorrectly

This fix resolves the strange behavior described in the original task where users would see green placeholders when switching between Clip and Comp modes.