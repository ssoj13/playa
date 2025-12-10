# Playa Bug Hunt & Architecture Analysis Report

## Executive Summary

This document provides comprehensive analysis of 5 tasks from `task.md`:
1. Cyclic dependency bug in comp drag-and-drop
2. Depth-first iterator for Comp
3. Layer storage architecture
4. Multi-layer tracks consideration
5. Node editor crate evaluation

---

## Task 1: Cyclic Dependency Bug (CRITICAL)

### Problem Description
Dragging a file comp onto itself causes infinite recursion and application freeze.

### Root Cause Analysis
**Location:** `src/main_events.rs:479-506` (AddLayerEvent handler)

```rust
// Current flow (NO VALIDATION):
AddLayerEvent { comp_uuid, source_uuid, ... }
  -> comp.add_child_layer(source_uuid, ...)  // No cycle check!
```

**Recursive call chain that causes freeze:**
```
comp.get_frame() -> compose() -> child.get_frame() -> compose() -> ...
```

When `source_uuid == comp_uuid`, `compose()` at line `comp.rs:1441` calls:
```rust
source.get_frame(source_frame, project, use_gpu)
```
Where `source == self`, creating infinite recursion.

### Solution: Two-Level Protection

#### Level 1: Prevention (in AddLayerEvent handler)
**File:** `src/main_events.rs` around line 479

```rust
if let Some(e) = downcast_event::<AddLayerEvent>(&event) {
    // NEW: Prevent self-reference
    if e.source_uuid == e.comp_uuid {
        log::warn!("Cannot add composition to itself: {}", e.source_uuid);
        return Some(result);
    }

    // NEW: Prevent ancestor cycles (source is ancestor of comp)
    if project.is_ancestor(e.source_uuid, e.comp_uuid) {
        log::warn!("Cannot create cyclic dependency: {} -> {}", e.source_uuid, e.comp_uuid);
        return Some(result);
    }

    // ... existing code
}
```

**New method in Project:**
```rust
/// Check if `potential_ancestor` is an ancestor of `comp_uuid` (would create cycle)
pub fn is_ancestor(&self, potential_ancestor: Uuid, comp_uuid: Uuid) -> bool {
    let media = self.media.read().expect("media lock poisoned");
    let mut current = comp_uuid;
    let mut visited = HashSet::new();

    while let Some(comp) = media.get(&current) {
        if !visited.insert(current) {
            return false; // Already visited, no cycle through this path
        }
        if let Some(parent) = comp.get_parent() {
            if parent == potential_ancestor {
                return true;
            }
            current = parent;
        } else {
            break;
        }
    }
    false
}
```

#### Level 2: Detection in compose() (User's Suggested Approach)
**File:** `src/entities/comp.rs`

This handles corrupted data (cycles already in saved projects):

```rust
// New signature with visited tracking
pub fn get_frame_safe(
    &self,
    frame_idx: i32,
    project: &super::Project,
    use_gpu: bool,
    visited: &mut HashSet<Uuid>
) -> Option<Frame> {
    let my_uuid = self.get_uuid();

    // Cycle detection
    if !visited.insert(my_uuid) {
        log::warn!("Cycle detected in composition graph at comp {}, skipping branch", my_uuid);
        return Some(self.placeholder_frame()); // Return placeholder, don't crash
    }

    let result = if self.is_file_mode() {
        self.get_file_frame(frame_idx, project)
    } else {
        self.get_layer_frame_safe(frame_idx, project, use_gpu, visited)
    };

    visited.remove(&my_uuid); // Allow visiting from different branches
    result
}

// Public entry point creates new visited set
pub fn get_frame(&self, frame_idx: i32, project: &super::Project, use_gpu: bool) -> Option<Frame> {
    let mut visited = HashSet::new();
    self.get_frame_safe(frame_idx, project, use_gpu, &mut visited)
}
```

### Implementation Complexity: LOW
- ~30 lines of code
- No architecture changes
- Backward compatible

---

## Task 2: Depth-First Iterator for Comp

### Current State
`Comp` has basic child iteration:
```rust
pub fn children_uuids(&self) -> impl Iterator<Item = &Uuid>  // Line 404
pub fn get_children(&self) -> &[(Uuid, Attrs)]               // Line 2101
```

### Proposed Iterator Design

```rust
/// Depth-first iterator over composition hierarchy
pub struct CompDepthFirstIter<'a> {
    project: &'a Project,
    stack: Vec<(Uuid, usize)>,  // (comp_uuid, depth)
    visited: HashSet<Uuid>,
    max_depth: Option<usize>,
}

impl<'a> CompDepthFirstIter<'a> {
    pub fn new(root: Uuid, project: &'a Project) -> Self {
        Self {
            project,
            stack: vec![(root, 0)],
            visited: HashSet::new(),
            max_depth: None,
        }
    }

    pub fn with_max_depth(mut self, depth: usize) -> Self {
        self.max_depth = Some(depth);
        self
    }
}

/// Yielded item includes depth for hierarchy display
pub struct CompIterItem {
    pub uuid: Uuid,
    pub depth: usize,
    pub is_leaf: bool,  // true if file comp or no children
}

impl<'a> Iterator for CompDepthFirstIter<'a> {
    type Item = CompIterItem;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some((uuid, depth)) = self.stack.pop() {
            // Cycle protection
            if !self.visited.insert(uuid) {
                continue;
            }

            // Depth limit
            if let Some(max) = self.max_depth {
                if depth > max { continue; }
            }

            let media = self.project.media.read().ok()?;
            let comp = media.get(&uuid)?;

            let is_leaf = comp.is_file_mode() || comp.children.is_empty();

            // Push children in reverse order (so first child is processed first)
            if !is_leaf && self.max_depth.map_or(true, |m| depth < m) {
                for (child_uuid, attrs) in comp.children.iter().rev() {
                    if let Some(source_str) = attrs.get_str("uuid") {
                        if let Ok(source_uuid) = Uuid::parse_str(source_str) {
                            self.stack.push((source_uuid, depth + 1));
                        }
                    }
                }
            }

            return Some(CompIterItem { uuid, depth, is_leaf });
        }
        None
    }
}

// Usage example:
impl Comp {
    pub fn iter_depth_first<'a>(&self, project: &'a Project) -> CompDepthFirstIter<'a> {
        CompDepthFirstIter::new(self.get_uuid(), project)
    }
}
```

### Use Cases
1. **Cache invalidation cascade** - replace manual `invalidate_cascade()` loop
2. **Preload planning** - gather all source comps for preloading
3. **Cycle detection** - detect cycles during validation
4. **Export** - collect all dependencies for project export
5. **UI tree display** - build hierarchy for outline view

### Recommendation: IMPLEMENT
- Clean Rust idiom
- Reusable across codebase
- Prevents cycle bugs by design

---

## Task 3: Layer Storage Architecture

### Current Implementation
```rust
// comp.rs line 108
pub children: Vec<(Uuid, Attrs)>
```

Where:
- `Uuid` = instance UUID (unique per layer placement)
- `Attrs` = HashMap<String, AttrValue> with all layer properties

### Analysis

**Pros of current approach:**
- Simple, flat structure
- Fast iteration
- Direct serde support
- Flexible attribute schema

**Cons:**
- No type safety for layer-specific attrs
- Attribute keys are strings (typo-prone)
- No validation of required attrs
- Mixed concerns (timing, transform, blend in one bag)

### Proposed: Typed Layer Struct

**New file: `src/entities/layer.rs`**

```rust
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use super::Attrs;

/// Typed layer representation with validated fields
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Layer {
    /// Instance UUID (unique per placement)
    pub instance_uuid: Uuid,

    /// Source comp UUID (what this layer references)
    pub source_uuid: Uuid,

    /// Display name
    pub name: String,

    /// Timing in parent timeline
    pub timing: LayerTiming,

    /// Visual properties
    pub visual: LayerVisual,

    /// Transform properties
    pub transform: LayerTransform,

    /// Extra attributes (extensible)
    #[serde(default)]
    pub extra: Attrs,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LayerTiming {
    pub in_frame: i32,      // Placement start in parent
    pub src_len: i32,       // Source duration (frames)
    pub trim_in: i32,       // Local trim start
    pub trim_out: i32,      // Local trim end
    pub speed: f32,         // Playback speed multiplier
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LayerVisual {
    pub visible: bool,
    pub opacity: f32,
    pub blend_mode: BlendMode,
    pub solo: bool,
    pub mute: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LayerTransform {
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
    pub pivot: [f32; 3],
}

impl Layer {
    /// Computed: layer visible start in parent coords
    pub fn start(&self) -> i32 {
        self.timing.in_frame + (self.timing.trim_in as f32 / self.timing.speed).round() as i32
    }

    /// Computed: layer visible end in parent coords
    pub fn end(&self) -> i32 {
        let visible_src = (self.timing.src_len - self.timing.trim_in - self.timing.trim_out).max(1);
        let visible_timeline = (visible_src as f32 / self.timing.speed).round() as i32;
        self.start() + visible_timeline - 1
    }

    /// Create from legacy Attrs (migration)
    pub fn from_attrs(instance_uuid: Uuid, attrs: &Attrs) -> Self {
        // ... migration code
    }

    /// Convert back to Attrs (backward compat)
    pub fn to_attrs(&self) -> Attrs {
        // ... conversion code
    }
}

// Dirty tracking can be per-subsystem:
impl LayerTiming {
    pub fn mark_dirty(&mut self) { /* ... */ }
}
```

### Migration Strategy
1. Add `Layer` struct alongside existing `Vec<(Uuid, Attrs)>`
2. Add `from_attrs()` / `to_attrs()` converters
3. Gradually migrate code to use typed access
4. Eventually replace `children: Vec<(Uuid, Attrs)>` with `children: Vec<Layer>`
5. Serde handles both formats during transition

### Recommendation: IMPLEMENT INCREMENTALLY
- Start with Layer struct definition
- Add helper methods that use it internally
- Keep backward compatibility with Attrs

---

## Task 4: Multi-Layer Tracks

### Current Model
```
Timeline:
  Layer 0 (row 0): [clip A]
  Layer 1 (row 1):         [clip B]
  Layer 2 (row 0):                   [clip C]  <- same row, no overlap
```

Layers are stored in `Vec<(Uuid, Attrs)>`, rows computed via `compute_layer_rows()` greedy algorithm.

### Question: Multiple Clips Per Track?

**Option A: Current (greedy auto-layout)**
```
children: Vec<Layer>
rows: computed dynamically via greedy algorithm
```
- Pros: Simple, automatic, no user management
- Cons: User has less control, can't "pin" to specific track

**Option B: Explicit Tracks**
```rust
pub struct Track {
    pub name: String,
    pub locked: bool,
    pub color: Option<Color32>,
    pub layers: Vec<Layer>,  // Multiple layers per track
}

// Comp becomes:
pub tracks: Vec<Track>
```
- Pros: User-controlled organization, DAW-like workflow
- Cons: More complexity, migration needed

**Option C: Hybrid (track_id attribute)**
```rust
// Add to LayerTiming or separate:
pub track_id: Option<usize>,  // None = auto-assign
```
- Pros: Backward compatible, opt-in organization
- Cons: Need to handle gaps, track management UI

### Recommendation: Keep Current + Add track_id Later
The current greedy layout works well. Add `track_id: Option<usize>` to Layer for users who want explicit control, but keep auto-layout as default.

---

## Task 5: Node Editor Crates for egui

### Research Results

| Crate | Stars | egui Version | Status | Best For |
|-------|-------|--------------|--------|----------|
| **egui-snarl** | 485 | ^0.33 | Active | Production compositor |
| egui-graph-edit | 25 | 0.31 | Active | Max flexibility |
| egui_graphs | 640 | ^0.33 | Active | Graph visualization |
| egui_node_graph | - | 0.19 | Deprecated | Don't use |

### Recommendation: egui-snarl 0.9.0

**Why:**
1. **egui 0.33 compatible** - matches your Cargo.toml
2. **Active development** - last update Jan 2025
3. **Built-in serde** - save/load node graphs
4. **Production quality** - used in real apps
5. **Beautiful wires** - professional look

**Integration example for Playa:**

```toml
# Cargo.toml addition
egui-snarl = { version = "0.9", features = ["serde"] }
```

```rust
// src/widgets/node_editor/mod.rs
use egui_snarl::{Snarl, SnarlViewer};

#[derive(Clone, Serialize, Deserialize)]
pub enum CompNode {
    Source { comp_uuid: Uuid },
    Blend { opacity: f32, mode: String },
    Output { name: String },
}

// Each Comp becomes a node
// Connections = child relationships
// Natural fit for existing architecture!
```

**Architecture fit:**
- Each `Comp` = one node
- `children` = input connections
- Node graph serializes to/from `Project`
- UI shows both timeline AND node view of same data

---

## Data Flow Diagram

```
                    ┌─────────────────────────────────────────┐
                    │              Project                     │
                    │  ┌────────────────────────────────────┐ │
                    │  │ media: HashMap<Uuid, Comp>         │ │
                    │  │                                    │ │
                    │  │   Comp (Layer Mode)                │ │
                    │  │   ├─ attrs: Attrs                  │ │
                    │  │   ├─ children: Vec<(Uuid, Attrs)>  │ │
                    │  │   │   ├─ instance_uuid             │ │
                    │  │   │   └─ source_uuid (in attrs)────┼─┼─── References other Comp
                    │  │   └─ layer_selection               │ │
                    │  │                                    │ │
                    │  │   Comp (File Mode)                 │ │
                    │  │   ├─ attrs: Attrs                  │ │
                    │  │   │   ├─ file_mask                 │ │
                    │  │   │   ├─ file_start/file_end       │ │
                    │  │   └─ children: [] (empty)          │ │
                    │  └────────────────────────────────────┘ │
                    │                                         │
                    │  global_cache: GlobalFrameCache         │
                    │  compositor: Mutex<CompositorType>      │
                    └─────────────────────────────────────────┘
                                      │
                                      ▼
                    ┌─────────────────────────────────────────┐
                    │           Render Pipeline               │
                    │                                         │
                    │  get_frame(frame_idx)                   │
                    │       │                                 │
                    │       ├─ File mode: load from disk      │
                    │       │                                 │
                    │       └─ Layer mode: compose()          │
                    │           │                             │
                    │           ├─ For each child:            │
                    │           │   └─ child.get_frame() ◄────┼── RECURSION POINT
                    │           │                             │    (cycle bug here)
                    │           └─ Blend all frames           │
                    └─────────────────────────────────────────┘
```

---

## Implementation Priority

| Task | Priority | Effort | Impact |
|------|----------|--------|--------|
| 1. Cycle detection | HIGH | Low | Fixes crash |
| 2. DFS Iterator | MEDIUM | Low | Code quality |
| 3. Layer struct | MEDIUM | Medium | Type safety |
| 5. Node editor | LOW | High | New feature |
| 4. Multi-tracks | LOW | Medium | UX improvement |

---

## Checklist

- [ ] Task 1: Add cycle check in AddLayerEvent handler
- [ ] Task 1: Add visited set to compose() for runtime detection
- [ ] Task 2: Implement CompDepthFirstIter
- [ ] Task 3: Create Layer struct in new file
- [ ] Task 3: Add migration helpers from_attrs/to_attrs
- [ ] Task 4: Consider track_id attribute (future)
- [ ] Task 5: Evaluate egui-snarl integration (future)

---

## Notes for Context Recovery

**Key files:**
- `src/entities/comp.rs` - Main Comp struct, compose() at line 1374
- `src/main_events.rs` - AddLayerEvent handler at line 479
- `src/entities/attrs.rs` - Attrs HashMap wrapper
- `src/entities/keys.rs` - Attribute key constants
- `src/widgets/timeline/timeline_ui.rs` - Timeline rendering

**Current egui version:** 0.33 (from Cargo.toml)

**Test files:** `comp.rs` has tests starting at line 2374
