# Bug Hunt Report: Playa Timeline Window Analysis

## Executive Summary

This report presents a comprehensive analysis of the Playa timeline window, focusing on the identified bugs and issues outlined in task.md. The investigation involved deep code review of the timeline rendering logic, interaction handling, and related components. Key findings include alignment issues in layer controls, unnecessary UI elements, rendering clipping problems, viewport update inefficiencies, and timeline panel synchronization issues.

## Project Structure Overview

The Playa project is a Rust-based application using the egui framework for UI. The timeline functionality is implemented across several modules:

- `src/ui.rs`: Top-level UI rendering, including `render_timeline_panel`
- `src/widgets/timeline/`: Timeline-specific widgets and logic
  - `timeline_ui.rs`: Rendering functions for outline and canvas
  - `timeline.rs`: State, configuration, and geometry calculations
- `src/bin/timeline.rs`: Standalone timeline window for testing
- Related components: Player, EventBus, Cache management

## Dataflow Analysis

The application follows a clear dataflow pattern:

1. **UI Rendering**: `render_timeline_panel` orchestrates timeline display
   - Toolbar: Transport controls, zoom, snap settings
   - Outline (left panel): Layer list with controls (visibility, opacity, blend, speed)
   - Canvas (right panel): Visual timeline bars with drag-and-drop

2. **User Interactions**: Mouse/keyboard events detected in render functions
   - Hover detection for tool cursors
   - Click/drag initiation for layer manipulation
   - Keyboard shortcuts for playback control

3. **Event Dispatch**: Interactions emit events to EventBus
   - Layer attribute changes (visibility, opacity, etc.)
   - Layer reordering and repositioning
   - Timeline pan/zoom updates

4. **State Updates**: Events processed by Player/Project
   - Cache invalidation and recomputation
   - Frame status updates

5. **Viewport Rendering**: Based on current frame and cache status
   - Displays cached frames or loading indicators

## Identified Bugs and Issues

### Bug 1: Layer Control Horizontal Alignment
**Description**: Layer control elements in the outline panel are not properly aligned horizontally due to varying name lengths.

**Root Cause**: While individual elements use fixed widths (drag: 20px, checkbox: 20px, name: 150px, opacity: 60px, blend: 90px, speed: 50px), the layout may not ensure consistent column positioning across rows.

**Impact**: Poor visual consistency in the timeline interface.

### Bug 2: Unnecessary Reorder Handle in Outline
**Description**: Left-side reorder box ("≡") is redundant since drag-and-drop reordering is available in the canvas panel.

**Root Cause**: UI includes both outline-based reordering and canvas-based DnD, creating confusion.

**Impact**: Cluttered interface and potential user confusion.

### Bug 3: Layer Disappearance During Drag Operations
**Description**: Layers disappear when dragged beyond timeline boundaries, especially with 3+ layers and upper layers.

**Root Cause**: `timeline_width` calculation in `render_canvas` is based on `total_frames`, but doesn't account for layers positioned outside this range. The `allocate_painter` rect clips rendering to `timeline_width`, causing out-of-bounds layers to disappear.

**Code Location**: `src/widgets/timeline/timeline_ui.rs:439-440`
```rust
let timeline_width = (total_frames as f32 * config.pixels_per_frame * state.zoom).max(available_for_timeline);
```

**Impact**: Loss of visual feedback during layer manipulation, breaking workflow.

### Bug 4: Viewport Not Updating on Frame Cache Status Changes
**Description**: Application startup fills frame cache, but viewport doesn't update until timeline is moved. Shows "loading frame N" persistently.

**Root Cause**: No mechanism to detect cache status changes for the current frame and trigger viewport repaint.

**Impact**: Poor user experience with stale loading indicators.

### Bug 5: Timeline Panel Drawing Inconsistencies
**Description**: Timeline appears oddly drawn, possibly with incorrect offsets, not cleanly split by a resizable divider.

**Root Cause**: Potential height synchronization issues between outline and canvas panels. Outline includes top spacing (`ui.add_space(20.0 + status_bar_height + 4.0 + 24.0)`), while canvas ruler starts immediately, causing misalignment.
This is a magic numbers magic and should be resolved.

**Code Location**: `src/widgets/timeline/timeline_ui.rs:174-182` (outline spacing) vs. no equivalent in canvas.

**Impact**: Visual inconsistency in split-view mode.

## Incomplete Code and TODOs

Limited TODOs found in codebase:

1. `src/bin/viewport.rs:39`: Settings usage in viewport standalone (minor)
2. `src/entities/gpu_compositor.rs:244`: GPU texture caching implementation
3. `src/entities/gpu_compositor.rs:813`: Proper canvas-sized blending
4. `src/main.rs:267`: Frame preload radius implementation (minor)

No FIXME comments found. Overall code quality appears solid with minimal incomplete features.

## Proposed Solutions

### Solution 1: Improve Layer Control Alignment
- **Option A**: Implement a proper grid/table layout for outline controls to ensure column alignment
- **Option B**: Add vertical separators between columns (similar to Attribute Editor)
- **Option C**: Increase blend mode column width to 100px to accommodate longer text

**Recommendation**: Option B - draggable solid vertical separators provide clear visual separation and alignment cues.

### Solution 2: Remove Redundant Reorder Handle
- Remove the drag handle ("≡") from outline panel since canvas DnD provides reordering
- Simplifies UI and reduces confusion

### Solution 3: Fix Layer Rendering Clipping
- Dynamically calculate `timeline_width` to encompass all layer positions:
```rust
let all_starts: Vec<i32> = comp.children.iter().map(|(_, attrs)| attrs.full_bar_start()).collect();
let all_ends: Vec<i32> = comp.children.iter().map(|(_, attrs)| attrs.full_bar_end()).collect();
let min_start = all_starts.iter().min().unwrap_or(&0);
let max_end = all_ends.iter().max().unwrap_or(&total_frames);
let extended_frames = (max_end - min_start).max(total_frames);
let timeline_width = (extended_frames as f32 * config.pixels_per_frame * state.zoom).max(available_for_timeline);
```
- Add margin (e.g., 100 frames) for smooth dragging beyond boundaries

### Solution 4: Implement Viewport Status Polling
- **Option A**: In viewport update loop, check `cache.get_frame_status(current_frame)` and request repaint on status change
- **Option B**: Add event-based notification when frame cache status updates
- **Option C**: Implement dirty flag system for viewport invalidation

**Recommendation**: Option A - simple polling in update loop, low overhead.

### Solution 5: Synchronize Timeline Panel Heights
- Add matching top spacing in canvas panel before ruler rendering:
```rust
// In render_canvas, before ruler
ui.add_space(20.0 + status_bar_height + 4.0 + 24.0);
```
- Ensure consistent vertical alignment between panels

## Conclusion

The timeline window implementation is well-structured but contains several UI/UX bugs that impact usability. The most critical issues are layer disappearance during drag operations (Bug 3) and viewport update delays (Bug 4). Proposed solutions are production-grade and maintain the application's performance characteristics.

## Implementation Plan

See `plan1.md` for detailed implementation steps and task breakdown.
