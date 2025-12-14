# Node Architecture Migration Plan

## Goal
Migrate from monolithic `Comp` (with COMP_FILE/COMP_NORMAL modes) to proper Node-based architecture where each node type is separate.

## Branch: dev3

---

## Architecture

### 1. Node Trait (`src/entities/node.rs`)

```rust
pub trait Node: Send + Sync {
    fn uuid(&self) -> Uuid;
    fn name(&self) -> &str;
    fn node_type(&self) -> &'static str;  // "File", "Comp"
    
    fn attrs(&self) -> &Attrs;
    fn attrs_mut(&mut self) -> &mut Attrs;
    
    /// Direct inputs (layers). Empty for FileNode.
    fn inputs(&self) -> &[Layer];
    
    /// Compute frame output. Result cached in global_cache[uuid][frame].
    fn compute(&mut self, frame: i32, ctx: &ComputeContext) -> Option<Frame>;
    
    fn is_dirty(&self) -> bool;
    fn mark_dirty(&self);
    fn clear_dirty(&self);
}

pub struct ComputeContext<'a> {
    pub project: &'a Project,
    pub cache: &'a GlobalFrameCache,
}
```

### 2. NodeKind Enum (`src/entities/node_kind.rs`)

```rust
pub enum NodeKind {
    File(FileNode),
    Comp(CompNode),
}

// Implement Node trait for NodeKind - delegates to inner node
impl Node for NodeKind { ... }
```

### 3. FileNode (`src/entities/file_node.rs`)

Replaces `COMP_FILE` mode. Reads image sequences and video files.

```rust
pub struct FileNode {
    pub attrs: Attrs,
    // attrs contains: uuid, name, file_mask, file_start, file_end, fps, width, height
}

impl Node for FileNode {
    fn inputs(&self) -> &[Layer] { &[] }  // no inputs
    
    fn compute(&mut self, frame: i32, ctx: &ComputeContext) -> Option<Frame> {
        // Current logic from Comp::load_file_frame()
        // - resolve file path from file_mask + frame
        // - load via Loader or video decoder
        // - return Frame
    }
}
```

### 4. CompNode (`src/entities/comp_node.rs`)

Replaces `COMP_NORMAL` mode. Composites multiple layers.

```rust
pub struct CompNode {
    pub attrs: Attrs,        // uuid, name, fps, in, out, trim_in, trim_out, etc.
    pub layers: Vec<Layer>,  // ordered inputs (MultiInput) - bottom to top
}

impl Node for CompNode {
    fn inputs(&self) -> &[Layer] { &self.layers }
    
    fn compute(&mut self, frame: i32, ctx: &ComputeContext) -> Option<Frame> {
        // Current logic from Comp::compose()
        // 1. Check dirty: self.is_dirty() || any layer.attrs.is_dirty() || any source.is_dirty()
        // 2. For each layer: get source node from ctx.project, call source.compute()
        // 3. Composite all frames with layer.attrs (opacity, blend_mode, transform)
        // 4. Return composed Frame
    }
}
```

### 5. Layer struct (in comp_node.rs)

Layer = instance of a source node with local attributes.

```rust
pub struct Layer {
    pub uuid: Uuid,          // instance uuid of THIS layer
    pub source_uuid: Uuid,   // uuid of source node in project.media
    pub attrs: Attrs,        // instance attrs: in, src_len, trim_in, trim_out, 
                             // opacity, visible, blend_mode, speed, transform
}
```

Key concept:
- Changing source node attrs affects ALL layers referencing it
- Changing layer.attrs affects ONLY this layer instance

### 6. Project.media change

```rust
// OLD:
pub media: Arc<RwLock<HashMap<Uuid, Comp>>>

// NEW:
pub media: Arc<RwLock<HashMap<Uuid, NodeKind>>>
```

### 7. Node iteration (`Project::iter_node`)

```rust
impl Project {
    /// Iterate node tree depth-first. depth=-1 means unlimited.
    pub fn iter_node(&self, root: Uuid, depth: i32) -> NodeIter;
}

pub struct NodeIter<'a> {
    project: &'a Project,
    stack: Vec<(Uuid, i32)>,  // (uuid, current_depth)
    max_depth: i32,           // -1 = unlimited
}

pub struct NodeIterItem {
    pub uuid: Uuid,
    pub depth: i32,
    pub is_leaf: bool,
}
```

Usage:
```rust
for item in project.iter_node(root, -1) { }   // full tree
for item in project.iter_node(root, 2) { }    // 2 levels deep
for item in project.iter_node(root, 1) { }    // direct children only
```

---

## Migration Steps

### Step 1: Create new files ✅ COMPLETE
- [x] `src/entities/node.rs` - Node trait + ComputeContext
- [x] `src/entities/file_node.rs` - FileNode struct + impl
- [x] `src/entities/comp_node.rs` - CompNode + Layer structs + impl
- [x] `src/entities/node_kind.rs` - NodeKind enum + impl Node
- [x] Update `src/entities/mod.rs` - export new modules

### Step 2: Migrate logic from Comp ✅ COMPLETE
- [x] `Comp::load_file_frame()` -> `FileNode::compute()`
- [x] `Comp::compose()` -> `CompNode::compute()`
- [x] `Comp::children` -> `CompNode::layers`
- [x] `Comp::get_frame()` routing logic -> `NodeKind::compute()`
- [x] Dirty tracking: `is_dirty()`, `mark_dirty()`, `clear_dirty()`

### Step 3: Update Project ✅ COMPLETE
- [x] Change `media: Arc<RwLock<HashMap<Uuid, NodeKind>>>`
- [x] `with_comp()` / `modify_comp()` work with NodeKind (downcast to CompNode)
- [x] `add_node()` method implemented
- [x] `del_comp()` works with NodeKind
- [x] Add `iter_node(root, depth)` method
- [x] Add `descendants(root)` helper
- [x] Add `is_ancestor(a, b)` helper

### Step 4: Update all Comp usages ✅ COMPLETE

#### Core files:
- [x] `src/entities/project.rs` - uses NodeKind
- [x] `src/entities/comp.rs` - DELETED (replaced by comp_node.rs)
- [x] `src/entities/layer.rs` - DELETED (Layer struct in comp_node.rs)
- [x] `src/entities/mod.rs` - exports updated

#### Main app:
- [x] `src/main.rs` - dirty checking, compose calls, event handling
- [x] `src/main_events.rs` - all events work with Node architecture

#### Widgets:
- [x] `src/widgets/project/project_ui.rs` - works with NodeKind
- [x] `src/widgets/timeline/timeline_ui.rs` - works with CompNode layers
- [x] `src/widgets/viewport/viewport_ui.rs` - frame display working
- [x] `src/widgets/node_editor/node_graph.rs` - visualizes node tree

#### Events:
- [x] `src/widgets/project/project_events.rs` - media events
- [x] `src/widgets/timeline/timeline_events.rs` - layer events
- [x] `src/entities/comp_events.rs` - comp/layer events
- [x] `src/widgets/node_editor/node_events.rs` - node editor events

#### Other:
- [x] `src/core/player.rs` - active comp, playback
- [x] `src/core/global_cache.rs` - cache uses node uuid
- [x] `src/core/workers.rs` - background frame loading
- [x] `src/dialogs/encode/` - encoding works

### Step 5: Serialization ✅ COMPLETE
- [x] Serialize/Deserialize for NodeKind, FileNode, CompNode, Layer
- [x] Project serialization with arc_rwlock_hashmap helper
- [x] Save/load .json projects working

### Step 6: Tests
- [ ] Add comprehensive tests (low priority - app is working)

---

## Key Concepts to Remember

1. **Layer = Instance**: Layer is an instance of source node with local attrs. Source changes affect all instances. Layer attrs are local.

2. **Dirty tracking**: Check `self.attrs.is_dirty() || layer.attrs.is_dirty() || source.is_dirty()` in compute()

3. **Cache**: Results stored in `global_cache[node_uuid][frame_idx] = Frame`

4. **MultiInput**: CompNode.layers is ordered Vec - bottom layer first, top layer last (render order)

5. **Timeline vs NodeEditor**: Both are views of same data structure
   - Timeline: shows layers over time, manipulate timing
   - NodeEditor: shows node connections, manipulate flow

6. **No more COMP_FILE/COMP_NORMAL**: FileNode and CompNode are separate types

---

## Files deleted during migration
- [x] `src/entities/comp.rs` - replaced by file_node.rs + comp_node.rs
- [x] `src/entities/layer.rs` - Layer struct moved to comp_node.rs
- [x] COMP_FILE/COMP_NORMAL modes removed - FileNode and CompNode are separate types
