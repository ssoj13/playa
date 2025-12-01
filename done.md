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

## Build Status

- **Debug build**: OK (warnings only)
- **Release build**: OK (warnings only)
- **Tests**: 28/28 passed

---

# TODO - What's Left

## Potential Future Improvements

### 1. Comp Attrs Migration
- Migrate remaining Comp fields to Attrs system:
  - `name` -> `attrs.get_str("name")`
  - `start`, `end` -> `attrs.get_i32("start")`, `attrs.get_i32("end")`
  - `fps` -> `attrs.get_float("fps")`
  - `current_frame` -> `attrs.get_i32("current_frame")`
  - `play_start`, `play_end` -> `attrs.get_i32("play_start")`, `attrs.get_i32("play_end")`

### 2. Serialization Integration
- Ensure Attrs are properly serialized/deserialized in project save/load
- Test project persistence with new Attrs structure
- Verify backwards compatibility with old project files (migration logic)

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
