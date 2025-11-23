# Playa Code Review and Refactoring Plan

## Project Overview
Playa is a Rust-based image sequence player application with support for EXR, PNG, JPEG, TIFF formats, video I/O, and basic editing capabilities. It uses eframe/egui for the GUI and has a modular architecture with compositions, entities, and event-driven design.

## Identified Issues

### Critical Issues
1. **Invalid Rust Edition**: Cargo.toml specifies `edition = "2024"`, but Rust 2024 edition does not exist yet. This will prevent compilation. Must be changed to `"2021"`.

### Code Quality Issues
2. **Unimplemented Features**: Multiple TODO comments in `main.rs` `handle_event` method indicating incomplete playback controls:
   - Step forward/backward
   - Previous/next clip navigation
   - Layer operations (remove selected layer, drag-and-drop)
   - Fullscreen toggle
   - Timeline zoom/pan reset

3. **Potentially Unused Code**:
   - `selected_seq_idx` field in `Player` struct is used in `jump_prev_sequence`/`jump_next_sequence` but may not be properly integrated with UI
   - Backup directories: `.bak`, `.orig`, `.orig2` contain potentially outdated code

4. **Large Files**: `main.rs` (76KB) and `comp.rs` are very large and should be split for maintainability

5. **Missing Tests**: While `comp.rs` has comprehensive tests, other modules lack test coverage

6. **Documentation Gaps**: Some functions and modules lack proper documentation

### Architecture Issues
7. **Event Handler Duplication**: Some event handling logic is duplicated or incomplete in `handle_event`

8. **Inconsistent State Management**: Some UI state (like `selected_media_uuid`) may not be consistently synchronized

## Comprehensive Refactoring Plan

### Phase 1: Critical Fixes (Immediate)
1. **Fix Rust Edition**
   - Change `edition = "2024"` to `edition = "2021"` in Cargo.toml

2. **Implement Missing Features**
   - Complete all TODO items in `handle_event`
   - Add step forward/backward logic
   - Implement clip navigation
   - Add fullscreen toggle functionality

### Phase 2: Code Cleanup (Week 1-2)
3. **Remove Dead Code**
   - Delete `.bak`, `.orig`, `.orig2` directories
   - Remove unused variables and functions
   - Clean up commented code

4. **Refactor Large Files**
   - Split `main.rs` into multiple modules:
     - `app.rs`: Main application struct and lifecycle
     - `event_handlers.rs`: All event handling logic
     - `ui_main.rs`: Main UI rendering
   - Split `comp.rs` into:
     - `comp.rs`: Core Comp struct
     - `comp_detection.rs`: Sequence detection logic
     - `comp_composition.rs`: Composition/rendering logic

### Phase 3: Testing and Documentation (Week 3)
5. **Add Comprehensive Tests**
   - Unit tests for `Player` playback logic
   - Integration tests for event handling
   - UI interaction tests
   - Performance tests for frame loading/caching

6. **Improve Documentation**
   - Add module-level documentation
   - Document all public APIs
   - Create architecture overview diagrams

### Phase 4: Performance and Architecture Improvements (Week 4-5)
7. **Optimize Dependencies**
   - Run `cargo udeps` to identify unused dependencies
   - Review optional features usage
   - Consider replacing heavy dependencies if alternatives exist

8. **State Management Refactoring**
   - Implement proper state synchronization between UI and core
   - Add validation for state transitions
   - Improve error handling throughout

9. **Performance Optimizations**
   - Profile frame loading bottlenecks
   - Optimize cache invalidation logic
   - Improve timeline rendering performance

### Phase 5: Feature Completion (Week 6-8)
10. **Complete Missing UI Features**
    - Implement drag-and-drop for layers
    - Add keyboard shortcuts for all operations
    - Improve timeline interaction (zoom, pan, snap)

11. **Enhance Compositing Engine**
    - Add GPU-accelerated compositing
    - Support more blend modes
    - Improve layer management

## Implementation Priority
- **High Priority**: Fix Rust edition, implement missing playback controls
- **Medium Priority**: Code cleanup, file splitting, basic testing
- **Low Priority**: Advanced features, performance optimizations

## Success Metrics
- All TODOs resolved
- Code compiles and runs without warnings
- Test coverage > 80%
- No unused dependencies
- Improved maintainability and readability

## Risk Assessment
- Changing Rust edition may require minor syntax updates
- Refactoring large files carries risk of introducing bugs
- Performance optimizations may affect stability

## Timeline Estimate
- Phase 1: 1-2 days
- Phase 2: 1-2 weeks
- Phase 3: 1 week
- Phase 4: 2 weeks
- Phase 5: 2-3 weeks

Total estimated time: 6-8 weeks for complete refactoring.