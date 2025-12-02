# Playa Attrs Migration - Done

## Phase 1: Player Attrs Migration (Completed)

### Changes in `src/player.rs`:
- Added `Attrs` field to `Player` struct
- Migrated runtime fields to Attrs:
  - `is_playing` -> `attrs.get_bool("is_playing")`
  - `fps_base` -> `attrs.get_float("fps_base")`
  - `fps_play` -> `attrs.get_float("fps_play")`
  - `loop_enabled` -> `attrs.get_bool("loop_enabled")`
  - `active_comp` -> `attrs.get_json::<Uuid>("active_comp")`
- Added getter/setter methods:
  - `is_playing()` / `set_is_playing()`
  - `fps_base()` / `set_fps_base()`
  - `fps_play()` / `set_fps_play()`
  - `loop_enabled()` / `set_loop_enabled()`
  - `active_comp()` / `set_active_comp()`

### Changes in `src/main.rs`:
- Updated all direct field accesses to use new getter/setter methods
- Fixed borrow issues in selection validation

---

## Phase 2: Project Attrs Migration (Completed)

### Changes in `src/entities/project.rs`:
- Added `Attrs` field to `Project` struct
- Migrated serializable fields to Attrs:
  - `comps_order: Vec<Uuid>` -> `attrs.get_json::<Vec<Uuid>>("comps_order")`
  - `selection: Vec<Uuid>` -> `attrs.get_json::<Vec<Uuid>>("selection")`
  - `active: Option<Uuid>` -> `attrs.get_json::<Option<Uuid>>("active")`
  - `selection_anchor: Option<usize>` -> `attrs.get_json::<Option<usize>>("selection_anchor")`
- Added getter/setter methods:
  - `comps_order()` / `set_comps_order()` / `push_comps_order()` / `retain_comps_order()`
  - `selection()` / `set_selection()`
  - `active()` / `set_active()`

### Changes in `src/widgets/status/status.rs`:
- Updated to use `player.active_comp()` method

### Changes in `src/widgets/project/project_ui.rs`:
- Updated to use `project.comps_order()`, `project.selection()`, `project.active()` methods

---

## Phase 3: Comp Children Migration (Completed)

### Changes in `src/entities/comp.rs`:

#### Struct Changes:
- Changed `children: Vec<Uuid>` + `children_attrs: HashMap<Uuid, Attrs>` to:
  ```rust
  pub children: Vec<(Uuid, Attrs)>
  ```
- Each tuple: `(instance_uuid, child_attrs)` where `child_attrs` contains:
  - `"uuid"` (String): UUID of the source comp in project.media
  - `"start"`, `"end"` (Int): Local timeline bounds
  - `"play_start"`, `"play_end"` (Int): Work area bounds
  - Transform attrs: `"position"`, `"rotation"`, `"scale"`, `"pivot"`, `"opacity"`, etc.

#### Added Helper Methods:
```rust
// Immutable access
pub fn children_attrs_get(&self, uuid: &Uuid) -> Option<&Attrs>
pub fn children_uuids(&self) -> impl Iterator<Item = &Uuid>
pub fn children_uuids_vec(&self) -> Vec<Uuid>
pub fn children_len(&self) -> usize
pub fn children_is_empty(&self) -> bool
pub fn children_contains(&self, uuid: &Uuid) -> bool
pub fn children_get(&self, idx: usize) -> Option<&(Uuid, Attrs)>
pub fn children_uuid_at(&self, idx: usize) -> Option<Uuid>

// Mutable access
pub fn children_attrs_get_mut(&mut self, uuid: &Uuid) -> Option<&mut Attrs>
pub fn children_attrs_insert(&mut self, uuid: Uuid, attrs: Attrs)
pub fn children_attrs_remove(&mut self, uuid: &Uuid) -> Option<Attrs>
```

#### Updated Methods:
- `add_child()` - creates tuple with new Attrs
- `remove_child()` - removes tuple from Vec
- `has_child()` - uses `children_contains()`
- `get_children()` - returns `&[(Uuid, Attrs)]`
- `find_children_by_source()` - iterates tuples
- `uuid_to_idx()` / `idx_to_uuid()` - extracts UUID from tuples
- `play_range()` - accesses attrs from tuple
- `first_child_dim()` - destructures tuple
- `compose()` - iterates `(child_uuid, attrs)` tuples
- `move_child()`, `move_layers()`, `trim_layers()` - work with tuples
- `set_child_play_start()`, `set_child_play_end()` - use helper methods
- `get_child_edges_near()` - iterates tuples
- `rebound()` - iterates tuples

### Changes in `src/widgets/timeline/timeline_ui.rs`:
- Updated all `comp.children[idx]` accesses to destructure `(child_uuid, attrs)`
- Replaced `comp.children_attrs.get()` with direct attrs access from tuple
- Updated `compute_layer_selection()` calls to pass `children_uuids_vec()`
- Fixed selection and drag handling to use tuple destructuring

### Changes in `src/widgets/timeline/timeline_helpers.rs`:
- Updated `compute_all_layer_rows()` to destructure tuples
- Updated `find_free_row_for_new_layer()` to destructure tuples

### Changes in `src/main.rs`:
- Updated `AppEvent::RemoveLayer` handler
- Updated `AppEvent::UpdateLayerAttrs` handler to use `children_attrs_get_mut()`
- Updated `AppEvent::SetLayerPlayStart/End` handlers
- Updated attribute editor rendering to use `children_attrs_get()` / `children_attrs_get_mut()`

### Test Updates in `src/entities/comp.rs`:
- Fixed `test_recursive_composition()` - use `project.push_comps_order()`, proper RwLock access
- Fixed `test_dirty_tracking_on_attr_change()` - proper RwLock access, tuple destructuring
- Fixed `test_multi_layer_blending_placeholder_sources()` - tuple access, `get_frame()` signature

### Test Updates in `src/entities/project.rs`:
- Fixed `test_cascade_invalidation()` - children now pushed as tuples `(uuid, Attrs::new())`

---

## Phase 4: Attribute Key Renaming (Completed)

### Renamed Attribute Keys:
Following NLE conventions, attribute keys were renamed for clarity:

| Old Key | New Key | Description |
|---------|---------|-------------|
| `"start"` | `"in"` | Layer/comp in-point (start frame) |
| `"end"` | `"out"` | Layer/comp out-point (end frame) |
| `"play_start"` | `"trim_in"` | Work area / trim start |
| `"play_end"` | `"trim_out"` | Work area / trim end |

### Method Renames:
- `play_start()` → `trim_in()`
- `play_end()` → `trim_out()`
- `set_play_start()` → `set_trim_in()`
- `set_play_end()` → `set_trim_out()`
- `start()` → `_in()` (underscore prefix because `in` is a Rust keyword)
- `end()` → `_out()` (underscore prefix for consistency)
- `set_start()` → `set_in()`
- `set_end()` → `set_out()`

Note: Methods use underscore prefix (`_in`/`_out`) because `in` is a reserved Rust keyword. Attribute keys remain `"in"`/`"out"`.

### Files Updated:
- `src/entities/comp.rs` - All attribute access/set operations
- `src/widgets/timeline/timeline_ui.rs` - Layer rendering and interaction
- `src/widgets/timeline/timeline_helpers.rs` - Layout computation
- `src/main.rs` - Event handlers

---

## Phase 5: Current Frame Migration + Attrs Constants (Completed)

### Added Attribute Key Constants in `attrs.rs`:
```rust
pub const A_FRAME: &str = "frame";      // Current playback frame
pub const A_IN: &str = "in";            // In-point
pub const A_OUT: &str = "out";          // Out-point
pub const A_TRIM_IN: &str = "trim_in";  // Trim in-point
pub const A_TRIM_OUT: &str = "trim_out"; // Trim out-point
pub const A_FPS: &str = "fps";          // Frames per second
pub const A_NAME: &str = "name";        // Human-readable name
pub const A_SOURCE: &str = "source_uuid"; // Source comp UUID
pub const A_UUID: &str = "uuid";        // Entity UUID
```

### Migrated `current_frame` to Attrs:
- Removed `current_frame: i32` field from Comp struct
- Now stored as `attrs.get_i32(A_FRAME)`
- Added hot-path methods `frame()` / `set_frame()` with `#[inline]`
- All 88 usages across 12 files updated

### Method Renames:
- `comp.current_frame` → `comp.frame()`
- `comp.set_current_frame(x)` → `comp.set_frame(x)`

### Files Updated:
- `src/entities/attrs.rs` - Added constants
- `src/entities/comp.rs` - Removed field, added methods
- `src/main.rs` - Updated all usages
- `src/player.rs` - Updated all usages
- `src/widgets/status/status.rs` - Updated usages
- `src/widgets/timeline/timeline_ui.rs` - Updated usages
- `src/widgets/timeline/timeline_helpers.rs` - Updated usages

### Serialization:
- Attrs with `frame` properly serialize/deserialize
- Added `test_frame_serialization` test

---

## Build Status

- **Debug build**: OK (warnings only)
- **Release build**: OK (warnings only)
- **Tests**: 29/29 passed

---

# TODO - What's Left

## Potential Future Improvements

### 1. ~~Additional Comp Attrs Migration~~ (DONE)
- ~~`current_frame` -> `attrs.get_i32("frame")`~~ ✓

### 2. ~~Serialization Integration~~ (DONE)
- ~~Attrs properly serialized/deserialized~~ ✓
- ~~Test added for round-trip~~ ✓

### 3. Undo/Redo System
- Leverage Attrs dirty tracking for undo/redo
- Implement snapshot/restore based on Attrs state

### 4. Performance Optimization
- Consider caching frequently accessed Attrs values
- Profile Attrs access vs direct field access overhead
- Optimize hot paths if needed

### 5. Code Cleanup
- Remove unused warnings (dead code in workers.rs, cache_man.rs, etc.)
- Add `#[allow(dead_code)]` or remove unused methods
- Prefix unused parameters with `_`

### 6. Documentation
- Document new Attrs-based API
- Update any existing documentation to reflect changes
- Add examples for common patterns

### 7. UI Improvements
- Attribute Editor could show/edit all Attrs generically
- Consider exposing Attrs in debug UI for inspection
