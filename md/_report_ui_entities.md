# Playa Code Audit: UI & Entity Layer
**Date:** 2026-03-18  
**Scope:** `src/entities/` + `src/widgets/` (33 source files)  
**Auditor:** Code audit pass — research only, no modifications

---

## 1. Critical Issues

### 1.1 `NodeKind::fps()` hardcodes 24.0 for Camera and Text nodes instead of reading attrs
**File:** `src/entities/node_kind.rs` lines 162–169  
**Severity:** BUG — behavioral divergence from trait contract

The method manually dispatches and hardcodes 24.0 for `Camera` and `Text` variants:

```rust
pub fn fps(&self) -> f32 {
    match self {
        NodeKind::File(n) => n.fps(),
        NodeKind::Comp(n) => n.fps(),
        NodeKind::Camera(_) => 24.0,  // ignores attrs
        NodeKind::Text(_) => 24.0,    // ignores attrs
    }
}
```

`CameraNode` and `TextNode` both have `fps` in their schemas and the `Node` trait provides a default impl that reads from `attrs`. This method bypasses it entirely. `preview_source()` in `project.rs` calls `node.fps()` to set the preview comp's FPS — so Camera/Text nodes always produce a 24fps preview regardless of their actual fps attribute. The `Node` trait's `fps()` default would return the correct value from attrs.

**Root cause:** `#[enum_dispatch(Node)]` generates dispatch for all `Node` trait methods; these manually-written `NodeKind::fps()` / `_in()` / `_out()` / `frame()` methods are **freestanding methods on the struct**, not trait impls. They shadow the trait dispatch without participating in it. The comment on line 66 acknowledges the partial migration.

### 1.2 `CameraNode::use_poi()` default mismatch with constructor
**File:** `src/entities/camera_node.rs` lines 50 and 109  
**Severity:** BUG — silent semantic inconsistency

Constructor sets:
```rust
attrs.set_bool("use_poi", false);
```

Getter reads:
```rust
pub fn use_poi(&self) -> bool {
    self.attrs.get_bool("use_poi").unwrap_or(true)  // default is TRUE
}
```

When `use_poi` is explicitly stored as `false` in attrs (which is the case for all newly created cameras), `get_bool` returns `Some(false)` and `unwrap_or` is never reached — so this specific bug doesn't trigger on new nodes. However, any deserialized `CameraNode` that lacks the `use_poi` key entirely (e.g., created by an older version without the field) will silently get `true` instead of the intended default of `false`. This creates a migration hazard.

### 1.3 `contains_comp()` delegates to `contains_node()` without type check
**File:** `src/entities/project.rs` line 650–652  
**Severity:** BUG — misleading API

```rust
pub fn contains_comp(&self, uuid: Uuid) -> bool {
    self.contains_node(uuid)  // returns true even for FileNode/TextNode/CameraNode
}
```

Any caller that uses `contains_comp()` to verify a UUID refers specifically to a `CompNode` gets incorrect results if the UUID belongs to another node type.

### 1.4 `project.rs` line 343: silently discards error from `attrs.remove()`
**File:** `src/entities/project.rs` line 343  
**Severity:** MINOR — intentional but poorly scoped

```rust
let _ = self.attrs.remove("active");
```

`remove()` returns `Option<AttrValue>` (not `Result`), so this is technically not an error discard — it's discarding an `Option`. This is fine by itself, but the `let _ =` pattern is misleading because it looks like error suppression. Should be a plain expression statement: `self.attrs.remove("active");`.

---

## 2. NodeKind Enum Dispatch Analysis

### 2.1 What `#[enum_dispatch(Node)]` covers

`NodeKind` has `#[enum_dispatch(Node)]`. The `Node` trait in `node.rs` defines these methods with defaults:
- `fps()`, `_in()`, `_out()`, `frame()`, `work_area()`, `frame_count()`, `dim()`, `play_range()`, `bounds()`

All default implementations read from `self.attrs()`. Each concrete node type implements `Node` (or inherits defaults). `enum_dispatch` generates `NodeKind::fps()`, `NodeKind::_in()`, etc. as trait method dispatchers automatically.

### 2.2 The shadow methods — what is NOT using trait dispatch

`NodeKind` also has manually written **freestanding methods** (not `impl Node for NodeKind`):

| Method | Location | Problem |
|---|---|---|
| `fps()` | line 162 | Reimplements dispatch + hardcodes 24.0 for Camera/Text |
| `_in()` | line 180 | Reimplements dispatch, but correctly delegates |
| `_out()` | line 190 | Reimplements dispatch, but correctly delegates |
| `frame()` | line 200 | Reimplements dispatch, but correctly delegates |
| `is_file_mode()` | line 157 | Duplicate of `is_file()` at line 32 |

The comment at line 66 acknowledges: "fps/_in/_out/frame were not moved to trait." The correct fix is to remove these four methods entirely from the `NodeKind` impl block and call `<NodeKind as Node>::fps(&self)` where needed, or just rely on the trait method via `node.fps()`.

### 2.3 `is_file_mode()` is an exact duplicate of `is_file()`

`is_file()` (line 32) and `is_file_mode()` (line 157) both check `matches!(self, NodeKind::File(_))`. In `node_graph.rs` line 165, `is_file_mode()` is called. No semantic difference exists between the two.

### 2.4 `attach_schemas()` in `project.rs` uses manual match instead of a trait method

`project.rs` lines 224–229:
```rust
match node {
    NodeKind::File(f) => f.attach_schema(),
    NodeKind::Comp(c) => c.attach_schema(),
    NodeKind::Camera(c) => c.attach_schema(),
    NodeKind::Text(t) => t.attach_schema(),
}
```

If `attach_schema()` were added to the `Node` trait, this entire match would collapse to `node.attach_schema()`. Currently every time a new node type is added, this match must be manually extended (and currently so must `is_dirty`, `clear_dirty`, `is_renderable`, `name`, `uuid`, etc. — all of which are already in the `Node` trait).

---

## 3. Code Deduplication

### 3.1 Loop checkbox appears in two places
**Files:**  
- `src/widgets/timeline/timeline_ui.rs` ~line 106: Loop checkbox in `render_toolbar()`  
- `src/widgets/status/status.rs` ~line 106: Loop checkbox in status bar

Two separate Loop checkboxes exist. They likely bind to the same state, creating dual UI ownership without a clear canonical location. One should be removed, or both should be factored into a shared widget call.

### 3.2 `render_canvas()` in `ui.rs` is called three times with identical arguments
**File:** `src/ui.rs` lines 216–226, 235–245

The `CanvasOnly` and `Split` branches in `ui.rs` both call `render_canvas()` with the same arguments inside nearly identical `CentralPanel::default()` blocks. The only difference is `Split` mode also renders the outline panel first. This could be a single call with a conditional guard for the outline.

### 3.3 `get_config_dir()` and `get_data_dir()` in `config.rs` are structurally identical
**File:** `src/config.rs`

Both functions follow the pattern: get platform dir → append app name → create dir if not exist → return path. They differ only in whether they call `config_dir()` or `data_dir()`. A single helper `get_app_dir(base: PathBuf) -> PathBuf` would eliminate the duplication.

### 3.4 `prefs_to_map()` / `prefs_from_map()` bypass the attrs type system
**File:** `src/entities/project.rs` lines 234–281

Project preferences serialize to/from a manually constructed `HashMap<String, AttrValue>`. This is effectively a mini-serializer implemented by hand, bypassing `serde`. If `ProjectPrefs` derived `Serialize`/`Deserialize`, `attrs.set_json("prefs", &prefs)` / `attrs.get_json::<ProjectPrefs>("prefs")` would replace ~50 lines with 2.

### 3.5 Double-click handling in `timeline_outline` duplicates selection logic
**File:** `src/widgets/timeline/timeline_ui.rs` lines 471–501

On double-click, the outline panel both computes a full `compute_layer_selection()` for the click event AND dispatches `ProjectActiveChangedEvent`. The identical selection-computation path runs for both single and double clicks. The double-click handler re-runs `compute_layer_selection()` from scratch even though single click already ran it on the same frame.

---

## 4. Unused / Dead Code

### 4.1 `space.rs`: `src_to_object()` is dead code
**File:** `src/entities/space.rs` lines 71–77  
`#[allow(dead_code)]` explicitly marks it. This function has been unused long enough that the annotation was added rather than deleting the function.

### 4.2 `keys.rs`: `COMP_NORMAL`, `COMP_FILE`, legacy mode constants
**File:** `src/entities/keys.rs`

```rust
pub const COMP_NORMAL: i8 = 0;
pub const COMP_FILE: i8 = 1;
pub const A_MODE: &str = "mode";
```

These are remnants from when `CompNode` had a file-mode. `FileNode` now handles file-based media. A grep for callers would confirm whether these are still referenced or fully orphaned.

### 4.3 `ae_ui.rs`: `collect_changes` parameter is always `true`
**File:** `src/widgets/ae/ae_ui.rs`

`render_impl()` has a `collect_changes: bool` parameter but every call site passes `true`. The `false` branch (line 181) renders the editor read-only and discards results with `let _ = render_value_editor(...)`. If this mode is not used, the parameter and the dead branch should be removed. The `let _ =` on `render_value_editor()` result in the false branch is also an error-silent discard of a `bool`, which is harmless but indicates dead code.

### 4.4 `_saved_spacing` in `timeline_ui.rs` is assigned but never restored
**File:** `src/widgets/timeline/timeline_ui.rs` line 623

```rust
let _saved_spacing = ui.spacing().item_spacing.y;
ui.spacing_mut().item_spacing.y = 0.0;
```

The spacing is saved into `_saved_spacing` (underscore prefix means it's intentionally unused) but is never restored at the end of the function. The underscore suppresses the compiler warning. If the intent was to restore spacing after the function body, it should use an RAII guard or restore explicitly. If restoration is intentional to skip, the variable should be removed entirely.

### 4.5 `RwLockReadGuard` import in `node_graph.rs` (line 55) appears unused
**File:** `src/widgets/node_editor/node_graph.rs` line 55

The import is present but the read guard type is never mentioned in the visible portion. May be used in the unread tail of the file, but worth verifying — the compiler would warn on this if truly unused.

### 4.6 `StatusBar::update()` is a no-op
**File:** `src/widgets/status/status.rs`

```rust
pub fn update(&mut self, ctx: &egui::Context) {
    let _ = ctx;
}
```

The entire body discards `ctx`. This is either a stub that was never implemented or a method that lost its body after refactoring. Either way it should be removed from the trait/impl if nothing calls it, or implemented if something does.

### 4.7 Play icon in timeline toolbar is always "▶" regardless of playback state
**File:** `src/widgets/timeline/timeline_ui.rs` ~line 98

Comment says "Placeholder — real icon controlled by playback status" but the icon is unconditionally `"▶"`. There is no branch that switches it to a pause icon when playing. This is dead conditional logic (the condition was never written).

---

## 5. Performance Issues

### 5.1 `TextNode`: Two global `Mutex` locks per `compute()` call per frame
**File:** `src/entities/text_node.rs`

`FONT_SYSTEM: Mutex<FontSystem>` and `SWASH_CACHE: Mutex<SwashCache>` are global statics via `lazy_static!`. Every call to `render_text()` acquires both mutexes. In a timeline with multiple text layers, all workers serialize on these locks per frame. For single-threaded rendering this is only a structural concern, but with any async worker model it becomes a bottleneck. Both caches could be per-`TextNode` with an `Arc<Mutex<...>>` or passed via `ComputeContext`.

### 5.2 `TextNode::text()` and `font()` return `String` (heap allocation per call)
**File:** `src/entities/text_node.rs`

```rust
pub fn text(&self) -> String { ... }
pub fn font(&self) -> String { ... }
```

These accessor methods return owned `String` values, causing heap allocation on every call. They can return `&str` or `Cow<str>` since the underlying data is stored in `attrs` and the `Attrs` map is borrowed.

### 5.3 `renderer.rs`: F16 pixel path allocates a new `Vec<u8>` every frame
**File:** `src/widgets/viewport/renderer.rs` lines 320–321

```rust
let bytes_u8: Vec<u8> =
    bytemuck::cast_slice(self.f16_scratch.as_slice()).to_vec();
owned_bytes = Some(bytes_u8);
```

`self.f16_scratch` is reused across frames (cleared and extended), but the `bytes_u8` vector that follows is always newly allocated via `.to_vec()`. Since `bytemuck::cast_slice` returns a `&[u8]` slice over the existing buffer, this allocation is unnecessary — a second scratch buffer `f16_bytes_scratch: Vec<u8>` would eliminate the allocation.

### 5.4 `project.rs::gen_name()` iterates all media and all layers per name generation
**File:** `src/entities/project.rs` lines 764–787

Name generation holds the media read lock while scanning every node and every layer in every comp for name suffix conflicts. For large projects this is O(nodes * layers). This is called synchronously from the UI thread during media import. A simple counter-based suffix or a name registry would avoid the full scan.

### 5.5 `project.rs::preview_source()` calls `modify_comp()` which acquires a write lock, then immediately releases
**File:** `src/entities/project.rs` lines 450–496

The update path acquires the write lock via `modify_comp()`, clears layers, adds one layer, sets 4 attrs. Each `attrs.set()` call evaluates the schema dirty-flag check. If `A_IN`, `A_OUT`, `A_FPS`, `A_FRAME` are all non-DAG (as they should be for scrubbing), no dirty mark fires but the comparison overhead is present for each. This is minor but worth noting.

### 5.6 `attrs.rs::hash_filtered()` sorts keys on every call
**File:** `src/entities/attrs.rs` lines 700–723

```rust
let mut keys: Vec<&String> = self.map.keys().collect();
keys.sort_unstable();
```

This allocates a `Vec` and sorts it every time a hash is needed. If hashing is called frequently (e.g., cache key generation), a pre-sorted key list or a BTreeMap instead of HashMap would avoid this. Currently `HashMap` is used, so insertion order is non-deterministic, requiring the sort for determinism.

---

## 6. Recommendations (Priority Order)

### P1 — Fix behavioral bugs

1. **Remove `NodeKind::fps()`, `_in()`, `_out()`, `frame()` from the `NodeKind` impl block.** These four methods shadow the trait dispatch. Camera/Text nodes will then use the `Node` trait defaults which read from attrs correctly. This fixes the `fps = 24.0` hardcode bug for all callers including `preview_source()`.

2. **Fix `contains_comp()` to check node type.** Change to:
   ```rust
   pub fn contains_comp(&self, uuid: Uuid) -> bool {
       self.with_comp(uuid, |_| ()).is_some()
   }
   ```

3. **Fix `CameraNode` `use_poi` default mismatch.** Either change the constructor to `attrs.set_bool("use_poi", true)` to match the getter's `unwrap_or(true)`, or change the getter to `unwrap_or(false)` to match the constructor. The constructor intent (false) is more explicit, so getter should use `unwrap_or(false)`.

4. **Remove the `let _ =` on `attrs.remove("active")`.** Replace with a bare expression: `self.attrs.remove("active");`

### P2 — Remove enum_dispatch redundancy

5. **Remove `is_file_mode()`** — replace all call sites with `is_file()`.

6. **Add `attach_schema(&mut self)` to the `Node` trait** with a default that panics or no-ops, implement it in each node type, then collapse the match in `project.rs::attach_schemas()` to `node.attach_schema()`.

7. **Migrate the remaining freestanding `NodeKind` accessors** (`is_dirty`, `clear_dirty`, `is_renderable`) to the `Node` trait if they are not already there, removing more manual match blocks.

### P3 — Dead code removal

8. **Delete `space.rs::src_to_object()`** or remove the `#[allow(dead_code)]` only if it truly has a future use (add a TODO comment if so).

9. **Delete `COMP_NORMAL`, `COMP_FILE`, `A_MODE`** from `keys.rs` after verifying no remaining callers.

10. **Remove the `collect_changes: bool` parameter** from `ae_ui.rs::render_impl()` and hardcode the `true` path. Delete the `false` branch.

11. **Remove `StatusBar::update()` no-op** or implement it. If unused, remove its call sites too.

12. **Fix play icon** in `timeline_ui.rs` to actually toggle between `▶` and `⏸` based on playback state, or remove the misleading comment.

### P4 — Performance

13. **Eliminate F16 `Vec<u8>` allocation in `renderer.rs`** by adding a second scratch buffer `f16_bytes_scratch: Vec<u8>` to `ViewportRenderer` and reusing it instead of `.to_vec()`.

14. **Change `TextNode::text()` and `font()` to return `&str`** instead of `String`.

15. **Consider per-node `FontSystem`/`SwashCache`** in `TextNode` instead of global mutexes, or pass them via `ComputeContext`.

### P5 — Structural improvements

16. **Consolidate `prefs_to_map()` / `prefs_from_map()`**: derive `serde` on `ProjectPrefs` and use `attrs.set_json` / `attrs.get_json`.

17. **Unify `get_config_dir()` / `get_data_dir()`** in `config.rs` into a single generic helper.

18. **Use `CameraNode` config constants** (`DEFAULT_NEAR_CLIP`, `DEFAULT_FAR_CLIP`, `DEFAULT_FOV`) in `CameraNode::new()` instead of the hardcoded literals `1.0`, `10000.0`, `39.6`.

19. **Deduplicate the Loop checkbox**: decide on one canonical location (status bar or timeline toolbar) and remove the other.

20. **Fix `_saved_spacing`** in `timeline_ui.rs`: either restore the spacing after the function body or remove the assignment entirely (the `_` prefix suppresses the warning but doesn't fix the intent).

---

## 7. Summary Table

| ID | File | Line(s) | Category | Severity |
|----|------|---------|----------|----------|
| 1.1 | `node_kind.rs` | 162–169 | BUG | HIGH |
| 1.2 | `camera_node.rs` | 50, 109 | BUG | MEDIUM |
| 1.3 | `project.rs` | 650–652 | BUG | MEDIUM |
| 1.4 | `project.rs` | 343 | Style | LOW |
| 2.1–2.3 | `node_kind.rs` | 32, 157, 162–207 | Enum dispatch | MEDIUM |
| 2.4 | `project.rs` | 224–229 | Architecture | LOW |
| 3.1 | `timeline_ui.rs`, `status.rs` | ~106 | Duplication | LOW |
| 3.2 | `ui.rs` | 216–245 | Duplication | LOW |
| 3.3 | `config.rs` | — | Duplication | LOW |
| 3.4 | `project.rs` | 234–281 | Duplication | LOW |
| 3.5 | `timeline_ui.rs` | 471–501 | Duplication | LOW |
| 4.1 | `space.rs` | 71–77 | Dead code | LOW |
| 4.2 | `keys.rs` | — | Dead code | LOW |
| 4.3 | `ae_ui.rs` | ~99, 181 | Dead code | LOW |
| 4.4 | `timeline_ui.rs` | 623 | Dead code | LOW |
| 4.5 | `node_graph.rs` | 55 | Dead code | LOW |
| 4.6 | `status.rs` | — | Dead code | LOW |
| 4.7 | `timeline_ui.rs` | ~98 | Dead code | LOW |
| 5.1 | `text_node.rs` | — | Performance | MEDIUM |
| 5.2 | `text_node.rs` | — | Performance | LOW |
| 5.3 | `renderer.rs` | 320–321 | Performance | LOW |
| 5.4 | `project.rs` | 764–787 | Performance | LOW |
| 5.6 | `attrs.rs` | 700–723 | Performance | LOW |
