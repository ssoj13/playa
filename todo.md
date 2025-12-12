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

### Step 1: Create new files
- [ ] `src/entities/node.rs` - Node trait + ComputeContext
- [ ] `src/entities/file_node.rs` - FileNode struct + impl
- [ ] `src/entities/comp_node.rs` - CompNode + Layer structs + impl
- [ ] `src/entities/node_kind.rs` - NodeKind enum + impl Node
- [ ] Update `src/entities/mod.rs` - export new modules

### Step 2: Migrate logic from Comp
- [ ] `Comp::load_file_frame()` -> `FileNode::compute()`
- [ ] `Comp::compose()` -> `CompNode::compute()`
- [ ] `Comp::children` -> `CompNode::layers`
- [ ] `Comp::get_frame()` routing logic -> `NodeKind::compute()`
- [ ] Dirty tracking: `is_dirty()`, `mark_dirty()`, `clear_dirty()`

### Step 3: Update Project
- [ ] Change `media: HashMap<Uuid, NodeKind>`
- [ ] Update `get_comp()` -> `get_node()` 
- [ ] Update `add_comp()` -> `add_node()`
- [ ] Update `del_comp()` -> `del_node()`
- [ ] Update `modify_comp()` -> `modify_node()`
- [ ] Add `iter_node(root, depth)` method
- [ ] Remove old `CompIterator`

### Step 4: Update all Comp usages

Files that use Comp and need updates:

#### Core files:
- [ ] `src/entities/project.rs` - media HashMap type, all comp methods
- [ ] `src/entities/comp.rs` - will be replaced/removed
- [ ] `src/entities/layer.rs` - may need updates for new Layer struct
- [ ] `src/entities/mod.rs` - exports

#### Main app:
- [ ] `src/main.rs` - dirty checking, compose calls, event handling
- [ ] `src/main_events.rs` - AddClipEvent, AddCompEvent, RemoveLayerEvent, etc.

#### Widgets:
- [ ] `src/widgets/project/project_ui.rs` - project panel showing media
- [ ] `src/widgets/timeline/timeline.rs` - layer data structures
- [ ] `src/widgets/timeline/timeline_ui.rs` - layer rendering, drag/drop
- [ ] `src/widgets/viewport/viewport_ui.rs` - frame display
- [ ] `src/widgets/node_editor/node_graph.rs` - node visualization

#### Events:
- [ ] `src/widgets/project/project_events.rs` - media events
- [ ] `src/widgets/timeline/timeline_events.rs` - layer events
- [ ] `src/entities/comp_events.rs` - may rename to node_events.rs

#### Other:
- [ ] `src/core/player.rs` - active comp, playback
- [ ] `src/core/global_cache.rs` - cache keys use comp uuid
- [ ] `src/core/workers.rs` - background frame loading
- [ ] `src/dialogs/encode/` - encoding uses comp

### Step 5: Serialization
- [ ] Implement Serialize/Deserialize for NodeKind, FileNode, CompNode, Layer
- [ ] Update Project serialization (arc_rwlock_hashmap helper)
- [ ] Test save/load .json projects

### Step 6: Tests
- [ ] Update existing tests in comp.rs
- [ ] Add tests for FileNode::compute()
- [ ] Add tests for CompNode::compute()
- [ ] Add tests for Project::iter_node()
- [ ] Add tests for dirty propagation

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

## Files to delete after migration
- [ ] `src/entities/comp.rs` (replaced by file_node.rs + comp_node.rs)
- [ ] Remove `mode` attribute and COMP_FILE/COMP_NORMAL constants from keys.rs
