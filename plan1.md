# Implementation Plan: Timeline Bug Fixes

## Overview
This plan outlines the steps to fix the identified bugs in the Playa timeline window. Tasks are prioritized by impact and complexity.

## Bug Fixes

### [ ] Bug 1: Fix Layer Control Horizontal Alignment
- [ ] Analyze current layout in `render_outline` (`src/widgets/timeline/timeline_ui.rs`)
- [ ] Implement vertical separators between columns
- [ ] Test alignment with varying name lengths
- [ ] Verify in split-view and outline-only modes

### [ ] Bug 2: Remove Redundant Reorder Handle
- [ ] Remove drag handle ("≡") allocation in `render_outline`
- [ ] Update item spacing if needed
- [ ] Test that canvas DnD still works for reordering
- [ ] Verify no regression in layer manipulation

### [ ] Bug 3: Fix Layer Disappearance During Drag
- [ ] Modify `timeline_width` calculation in `render_canvas` to account for all layer positions
- [ ] Add margin for smooth dragging (e.g., 100 frames)
- [ ] Test dragging layers beyond original boundaries
- [ ] Verify rendering with 3+ layers, especially upper ones
- [ ] Performance test with many displaced layers

### [ ] Bug 4: Implement Viewport Status Updates
- [ ] Identify viewport update loop location
- [ ] Add polling for current frame cache status
- [ ] Request repaint when status changes from loading to ready
- [ ] Test startup cache loading updates viewport automatically
- [ ] Verify no unnecessary repaints when status unchanged

### [ ] Bug 5: Synchronize Timeline Panel Drawing
- [ ] Add matching top spacing in `render_canvas` before ruler
- [ ] Ensure consistent vertical alignment between outline and canvas
- [ ] Test in split-view mode
- [ ] Verify ruler and status bar alignment

## Testing and Validation

### [ ] Comprehensive Testing
- [ ] Test all view modes (Split, Canvas Only, Outline Only)
- [ ] Test with various layer counts and configurations
- [ ] Test drag operations across boundaries
- [ ] Performance testing with large compositions
- [ ] Cross-platform testing (Windows/Linux/macOS)

### [ ] Regression Testing
- [ ] Verify existing functionality still works
- [ ] Check keyboard shortcuts
- [ ] Test timeline zoom and pan
- [ ] Validate layer attribute changes persist

## Code Quality

### [ ] Code Review
- [ ] Ensure changes follow existing patterns
- [ ] Add comments for complex logic
- [ ] Update any affected documentation

### [ ] Performance Considerations
- [ ] Profile timeline rendering performance
- [ ] Ensure no memory leaks in extended timeline calculations
- [ ] Optimize polling frequency for cache status

## Deployment

### [ ] Integration
- [ ] Merge changes to main branch
- [ ] Update changelog
- [ ] Notify team of changes

### [ ] User Acceptance
- [ ] Demonstrate fixes to stakeholders
- [ ] Collect feedback on timeline usability improvements