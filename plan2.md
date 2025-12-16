# Plan 2: Node Unification & enum_dispatch

## Goal
Унифицировать Camera/Text nodes с File/Comp, применить enum_dispatch для NodeKind.

---

## Phase 1: Extend Node Trait (Core)

### 1.1 Add methods to `Node` trait with default implementations

**File:** `src/entities/node.rs`

```rust
// Add to trait Node:
fn play_range(&self, _use_work_area: bool) -> (i32, i32) {
    (0, self.frame_count().saturating_sub(1))
}

fn bounds(&self, use_trim: bool, _selection_only: bool) -> (i32, i32) {
    self.play_range(use_trim)
}

fn frame_count(&self) -> i32 {
    self.attrs().get_i32(A_SRC_LEN).unwrap_or(100)
}

fn dim(&self) -> (usize, usize) {
    let w = self.attrs().get_u32(A_WIDTH).unwrap_or(0) as usize;
    let h = self.attrs().get_u32(A_HEIGHT).unwrap_or(0) as usize;
    (w, h)
}

fn attach_schema(&mut self);
```

### 1.2 Override in FileNode/CompNode

- FileNode: `play_range()` -> delegates to `work_area_abs()`
- CompNode: `bounds()` -> uses selection logic
- Both: override `frame_count()`, `dim()` with existing implementations

---

## Phase 2: Update Camera/Text Schemas

### 2.1 CAMERA_SCHEMA additions

**File:** `src/entities/attr_schemas.rs`

Add to CAMERA_DEFS:
- `in` (Int, DAG_DISP) - default 0
- `out` (Int, DAG_DISP) - computed from src_len
- `src_len` (Int, DAG) - default 100
- `trim_in` (Int, DAG_DISP) - default 0
- `trim_out` (Int, DAG_DISP) - default 0
- `speed` (Float, DAG_DISP_KEY) - default 1.0
- `opacity` (Float, DAG_DISP_KEY) - default 1.0 (for camera fades)

### 2.2 TEXT_SCHEMA additions

**File:** `src/entities/attr_schemas.rs`

Add to TEXT_DEFS:
- `in` (Int, DAG_DISP) - default 0
- `out` (Int, DAG_DISP) - computed
- `src_len` (Int, DAG) - default 100 (or 1 for static)
- `trim_in` (Int, DAG_DISP) - default 0
- `trim_out` (Int, DAG_DISP) - default 0
- `speed` (Float, DAG_DISP_KEY) - default 1.0

---

## Phase 3: Update Camera/Text Constructors

### 3.1 CameraNode::new()

**File:** `src/entities/camera_node.rs`

```rust
// Add timing attrs initialization:
attrs.set(A_IN, AttrValue::Int(0));
attrs.set(A_SRC_LEN, AttrValue::Int(100));
attrs.set(A_TRIM_IN, AttrValue::Int(0));
attrs.set(A_TRIM_OUT, AttrValue::Int(0));
attrs.set(A_SPEED, AttrValue::Float(1.0));
attrs.set("opacity", AttrValue::Float(1.0));
```

### 3.2 TextNode::new()

**File:** `src/entities/text_node.rs`

```rust
// Add timing attrs initialization:
attrs.set(A_IN, AttrValue::Int(0));
attrs.set(A_SRC_LEN, AttrValue::Int(100));
attrs.set(A_TRIM_IN, AttrValue::Int(0));
attrs.set(A_TRIM_OUT, AttrValue::Int(0));
attrs.set(A_SPEED, AttrValue::Float(1.0));
```

---

## Phase 4: Implement Node trait methods in Camera/Text

### 4.1 CameraNode impl Node

```rust
fn play_range(&self, _use_work_area: bool) -> (i32, i32) {
    (self.attrs.layer_start(), self.attrs.layer_end())
}

fn frame_count(&self) -> i32 {
    self.attrs.get_i32(A_SRC_LEN).unwrap_or(100)
}

fn dim(&self) -> (usize, usize) {
    (0, 0) // Cameras have no dimensions
}

fn attach_schema(&mut self) {
    self.attrs.attach_schema(&CAMERA_SCHEMA);
}
```

### 4.2 TextNode impl Node

```rust
fn play_range(&self, _use_work_area: bool) -> (i32, i32) {
    (self.attrs.layer_start(), self.attrs.layer_end())
}

fn frame_count(&self) -> i32 {
    self.attrs.get_i32(A_SRC_LEN).unwrap_or(100)
}

fn dim(&self) -> (usize, usize) {
    let w = self.width().max(1) as usize;
    let h = self.height().max(1) as usize;
    (w, h)
}

fn attach_schema(&mut self) {
    self.attrs.attach_schema(&TEXT_SCHEMA);
}
```

---

## Phase 5: Apply enum_dispatch to NodeKind

### 5.1 Add enum_dispatch to Cargo.toml

```toml
[dependencies]
enum_dispatch = "0.3"
```

### 5.2 Refactor NodeKind

**File:** `src/entities/node_kind.rs`

```rust
use enum_dispatch::enum_dispatch;

#[enum_dispatch]
pub trait NodeOps {
    fn play_range(&self, use_work_area: bool) -> (i32, i32);
    fn bounds(&self, use_trim: bool, selection_only: bool) -> (i32, i32);
    fn frame_count(&self) -> i32;
    fn dim(&self) -> (usize, usize);
}

#[enum_dispatch(NodeOps)]
pub enum NodeKind {
    File(FileNode),
    Comp(CompNode),
    Camera(CameraNode),
    Text(TextNode),
}
```

### 5.3 Remove manual match implementations

Delete these methods from `impl NodeKind`:
- `play_range()` - now auto-generated
- `bounds()` - now auto-generated  
- `frame_count()` - now auto-generated
- `dim()` - now auto-generated

---

## Phase 6: Cleanup & Consistency

### 6.1 Rename `work_area_abs()` to `play_range()` in FileNode

For consistency with other nodes.

### 6.2 Add helper macro for as_* methods (optional)

```rust
macro_rules! as_variant {
    ($name:ident, $variant:ident, $type:ty) => {
        pub fn $name(&self) -> Option<&$type> {
            match self {
                NodeKind::$variant(n) => Some(n),
                _ => None,
            }
        }
    };
}
```

### 6.3 Remove redundant `is_file()`, `is_comp()` etc.

Replace with `matches!(node, NodeKind::File(_))` at call sites, or keep for convenience.

---

## Phase 7: Tests

### 7.1 Update existing tests

Ensure Camera/Text tests check timing attrs.

### 7.2 Add new tests

- Test Camera with custom duration (not default 100)
- Test Text with trim_in/trim_out
- Test speed affects play_range correctly

---

## Summary of Changes

| File | Changes |
|------|---------|
| `Cargo.toml` | Add `enum_dispatch = "0.3"` |
| `src/entities/node.rs` | Add trait methods with defaults |
| `src/entities/attr_schemas.rs` | Extend CAMERA_DEFS, TEXT_DEFS |
| `src/entities/camera_node.rs` | Add timing attrs, impl trait methods |
| `src/entities/text_node.rs` | Add timing attrs, impl trait methods |
| `src/entities/node_kind.rs` | Apply enum_dispatch, remove match blocks |
| `src/entities/file_node.rs` | Rename work_area_abs -> play_range (optional) |

---

## Benefits

1. **~50 lines removed** - match blocks in NodeKind
2. **Camera/Text become real layers** - editable duration, trimmable
3. **Consistency** - all nodes work the same way
4. **Performance** - enum_dispatch is zero-cost (static dispatch)
5. **Extensibility** - adding new node types is easier

---

## Risks

- Low: Schema changes require migration for saved projects with Camera/Text
- Mitigation: Old projects without timing attrs get defaults on load

---

## Order of Implementation

1. Phase 2 (schemas) - no breaking changes
2. Phase 3 (constructors) - no breaking changes
3. Phase 1 (trait) - add with defaults, existing code works
4. Phase 4 (impl methods) - override defaults
5. Phase 5 (enum_dispatch) - replace match blocks
6. Phase 6 (cleanup) - optional polish
7. Phase 7 (tests) - verify everything works
