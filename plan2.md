# Playa Architecture Plan v2

## Overview

Updated plan incorporating user feedback on all 5 tasks. This document serves as implementation guide and context recovery reference.

---

## Task 1: Cyclic Dependency Prevention & Detection

### 1.1 Prevention: `Comp::check_collisions()`

Centralized method using depth-first iterator to detect cycles BEFORE adding layers.

**File:** `src/entities/comp.rs`

```rust
/// Check for cyclic dependencies in composition hierarchy
///
/// # Arguments
/// * `potential_child` - UUID of comp being added as child
/// * `hier` - if true, check entire hierarchy; if false, check only direct children
///
/// # Returns
/// * `true` if adding `potential_child` would create a cycle
pub fn check_collisions(&self, potential_child: Uuid, project: &Project, hier: bool) -> bool {
    let my_uuid = self.get_uuid();

    // Direct self-reference
    if potential_child == my_uuid {
        return true;
    }

    if !hier {
        // Only check if potential_child is already a direct child
        return self.children.iter().any(|(_, attrs)| {
            attrs.get_str(A_UUID)
                .and_then(|s| Uuid::parse_str(s).ok())
                .map_or(false, |uuid| uuid == potential_child)
        });
    }

    // Full hierarchy check: would adding potential_child create a cycle?
    // This happens if my_uuid appears anywhere in potential_child's subtree
    let media = project.media.read().expect("media lock");
    let mut stack = vec![potential_child];
    let mut visited = HashSet::new();

    while let Some(current) = stack.pop() {
        if current == my_uuid {
            return true; // Cycle detected!
        }

        if !visited.insert(current) {
            continue; // Already visited
        }

        if let Some(comp) = media.get(&current) {
            for (_, attrs) in &comp.children {
                if let Some(source_str) = attrs.get_str(A_UUID) {
                    if let Ok(source_uuid) = Uuid::parse_str(source_str) {
                        stack.push(source_uuid);
                    }
                }
            }
        }
    }

    false
}
```

### 1.2 Visual Feedback: Red Drop Preview

**File:** `src/widgets/timeline/timeline_ui.rs` around line 1076

Modify `draw_drop_preview()` to show red indicator when cycle detected:

```rust
fn draw_drop_preview(
    &self,
    ui: &mut egui::Ui,
    ctx: &PreviewContext,
    would_cycle: bool,  // NEW parameter
) {
    let color = if would_cycle {
        egui::Color32::from_rgba_unmultiplied(255, 60, 60, 180) // Red
    } else {
        egui::Color32::from_rgba_unmultiplied(100, 180, 255, 180) // Blue
    };

    // ... existing preview drawing with color
}
```

**Cycle check during drag (in timeline_ui.rs):**

```rust
// When handling GlobalDragState::ProjectItem
let would_cycle = if let Some(comp) = media.get(&self.comp_uuid) {
    comp.check_collisions(dragged_uuid, project, true)
} else {
    false
};

if would_cycle {
    // Draw red preview, don't allow drop
    self.draw_drop_preview(ui, &ctx, true);
} else {
    // Normal blue preview
    self.draw_drop_preview(ui, &ctx, false);
}
```

### 1.3 AddLayerEvent Handler Update

**File:** `src/main_events.rs` around line 479

```rust
if let Some(e) = downcast_event::<AddLayerEvent>(&event) {
    // Check for cycles before adding
    let would_cycle = {
        let media = project.media.read().expect("media lock");
        media.get(&e.comp_uuid)
            .map_or(false, |comp| comp.check_collisions(e.source_uuid, project, true))
    };

    if would_cycle {
        log::warn!("Blocked cyclic dependency: {} -> {}", e.source_uuid, e.comp_uuid);
        return Some(result);
    }

    // ... existing add_child_layer code
}
```

### 1.4 Detection in compose(): Two Render Modes

**Prefs setting:** `src/entities/prefs.rs`

```rust
#[derive(Clone, Serialize, Deserialize)]
pub struct SystemPrefs {
    // ... existing fields
    pub planned_render_mode: bool,  // false = recursive (current), true = planned
}
```

**compose() modifications:** `src/entities/comp.rs`

```rust
/// Compose frame - supports two modes based on prefs
pub fn compose(
    &self,
    frame_idx: i32,
    project: &Project,
    use_gpu: bool,
) -> Option<Frame> {
    if project.prefs.system.planned_render_mode {
        self.compose_planned(frame_idx, project, use_gpu)
    } else {
        self.compose_recursive(frame_idx, project, use_gpu, &mut HashSet::new())
    }
}

/// Recursive mode with cycle detection via visited set
fn compose_recursive(
    &self,
    frame_idx: i32,
    project: &Project,
    use_gpu: bool,
    visited: &mut HashSet<Uuid>,
) -> Option<Frame> {
    let my_uuid = self.get_uuid();

    if !visited.insert(my_uuid) {
        log::warn!("Cycle detected during render at comp {}, skipping", my_uuid);
        return Some(self.placeholder_frame());
    }

    // ... existing compose logic, passing visited to child.compose_recursive()

    visited.remove(&my_uuid);
    result
}

/// Planned mode: collect render plan first, then execute
fn compose_planned(
    &self,
    frame_idx: i32,
    project: &Project,
    use_gpu: bool,
) -> Option<Frame> {
    // Phase 1: Build render plan (DFS, detect cycles)
    let plan = self.build_render_plan(frame_idx, project)?;

    // Phase 2: Execute plan bottom-up
    self.execute_render_plan(&plan, project, use_gpu)
}

/// Render plan entry
struct RenderPlanEntry {
    comp_uuid: Uuid,
    frame_idx: i32,
    depth: usize,
}

fn build_render_plan(&self, frame_idx: i32, project: &Project) -> Option<Vec<RenderPlanEntry>> {
    let mut plan = Vec::new();
    let mut visited = HashSet::new();

    self.collect_render_plan(frame_idx, project, 0, &mut plan, &mut visited)?;

    // Sort by depth descending (leaves first)
    plan.sort_by(|a, b| b.depth.cmp(&a.depth));
    Some(plan)
}
```

---

## Task 2: Depth-First Iterator

**File:** `src/entities/comp.rs` (new section)

```rust
/// Depth-first iterator over composition hierarchy
pub struct CompDfsIter<'a> {
    project: &'a Project,
    stack: Vec<(Uuid, usize)>,  // (comp_uuid, depth)
    visited: HashSet<Uuid>,
    max_depth: Option<usize>,
}

impl<'a> CompDfsIter<'a> {
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

pub struct CompIterItem {
    pub uuid: Uuid,
    pub depth: usize,
    pub is_leaf: bool,
}

impl<'a> Iterator for CompDfsIter<'a> {
    type Item = CompIterItem;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some((uuid, depth)) = self.stack.pop() {
            if !self.visited.insert(uuid) {
                continue; // Cycle protection
            }

            if let Some(max) = self.max_depth {
                if depth > max { continue; }
            }

            let media = self.project.media.read().ok()?;
            let comp = media.get(&uuid)?;

            let is_leaf = comp.is_file_mode() || comp.children.is_empty();

            if !is_leaf {
                for (_, attrs) in comp.children.iter().rev() {
                    if let Some(source_str) = attrs.get_str(A_UUID) {
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

impl Comp {
    pub fn iter_dfs<'a>(&self, project: &'a Project) -> CompDfsIter<'a> {
        CompDfsIter::new(self.get_uuid(), project)
    }
}
```

**Use cases:**
- `check_collisions()` - cycle detection
- `build_render_plan()` - planned render mode
- Cache invalidation cascade
- Node editor hierarchy display

---

## Task 3: Layer & Track Architecture

### 3.1 Layer Structure

**New file:** `src/entities/layer.rs`

```rust
use super::attrs::Attrs;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Layer wraps Attrs entirely - all properties stored in Attrs
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Layer {
    /// Instance UUID (unique per placement)
    pub instance_uuid: Uuid,
    /// All layer attributes (source_uuid, timing, transform, etc.)
    pub attrs: Attrs,
}

impl Layer {
    pub fn new(instance_uuid: Uuid, attrs: Attrs) -> Self {
        Self { instance_uuid, attrs }
    }

    /// Get source comp UUID
    pub fn source_uuid(&self) -> Option<Uuid> {
        self.attrs.get_str(A_UUID)
            .and_then(|s| Uuid::parse_str(s).ok())
    }

    /// Get layer name
    pub fn name(&self) -> &str {
        self.attrs.get_str(A_NAME).unwrap_or("Untitled")
    }

    /// Get in-frame (start position in parent timeline)
    pub fn in_frame(&self) -> i32 {
        self.attrs.get_int(A_IN).unwrap_or(0)
    }

    /// Get source length
    pub fn src_len(&self) -> i32 {
        self.attrs.get_int(A_SRC_LEN).unwrap_or(100)
    }

    /// Get trim in
    pub fn trim_in(&self) -> i32 {
        self.attrs.get_int(A_TRIM_IN).unwrap_or(0)
    }

    /// Get trim out
    pub fn trim_out(&self) -> i32 {
        self.attrs.get_int(A_TRIM_OUT).unwrap_or(0)
    }

    /// Visible start in parent coords
    pub fn start(&self) -> i32 {
        self.in_frame() + self.trim_in()
    }

    /// Visible end in parent coords
    pub fn end(&self) -> i32 {
        let visible_len = self.src_len() - self.trim_in() - self.trim_out();
        self.start() + visible_len.max(1) - 1
    }

    /// Check if layer is visible
    pub fn visible(&self) -> bool {
        self.attrs.get_bool(A_VISIBLE).unwrap_or(true)
    }

    /// Check if layer is muted
    pub fn muted(&self) -> bool {
        self.attrs.get_bool(A_MUTED).unwrap_or(false)
    }
}
```

### 3.2 Track Structure

```rust
/// Track contains multiple non-overlapping layers
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Track {
    /// Track name
    pub name: String,
    /// Track locked state
    pub locked: bool,
    /// Track color (for UI)
    #[serde(default)]
    pub color: Option<[u8; 4]>,
    /// Layers in this track (sorted by start time)
    pub layers: Vec<Layer>,
}

impl Track {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            locked: false,
            color: None,
            layers: Vec::new(),
        }
    }

    /// Check if layer can be placed without overlap
    pub fn can_place(&self, start: i32, end: i32) -> bool {
        !self.layers.iter().any(|layer| {
            let layer_start = layer.start();
            let layer_end = layer.end();
            // Overlap check
            start <= layer_end && end >= layer_start
        })
    }

    /// Add layer to track (maintains sorted order)
    pub fn add_layer(&mut self, layer: Layer) {
        let start = layer.start();
        let pos = self.layers.iter()
            .position(|l| l.start() > start)
            .unwrap_or(self.layers.len());
        self.layers.insert(pos, layer);
    }

    /// Get layer at specific frame
    pub fn layer_at_frame(&self, frame: i32) -> Option<&Layer> {
        self.layers.iter().find(|l| frame >= l.start() && frame <= l.end())
    }
}
```

### 3.3 Updated Comp Structure

```rust
// In comp.rs, replace:
// pub children: Vec<(Uuid, Attrs)>

// With:
pub tracks: Vec<Track>,
```

### 3.4 Migration

```rust
impl Comp {
    /// Migrate from old children format to tracks
    pub fn migrate_to_tracks(&mut self) {
        if !self.tracks.is_empty() || self.children.is_empty() {
            return;
        }

        // Use greedy algorithm to assign layers to tracks
        let mut tracks: Vec<Track> = Vec::new();

        for (instance_uuid, attrs) in std::mem::take(&mut self.children) {
            let layer = Layer::new(instance_uuid, attrs);
            let start = layer.start();
            let end = layer.end();

            // Find track that can fit this layer
            let track_idx = tracks.iter()
                .position(|t| t.can_place(start, end))
                .unwrap_or_else(|| {
                    tracks.push(Track::new(format!("Track {}", tracks.len() + 1)));
                    tracks.len() - 1
                });

            tracks[track_idx].add_layer(layer);
        }

        self.tracks = tracks;
    }

    /// Get all layers across all tracks (for compatibility)
    pub fn all_layers(&self) -> impl Iterator<Item = &Layer> {
        self.tracks.iter().flat_map(|t| &t.layers)
    }

    /// Get all layers mutable
    pub fn all_layers_mut(&mut self) -> impl Iterator<Item = &mut Layer> {
        self.tracks.iter_mut().flat_map(|t| &mut t.layers)
    }
}
```

---

## Task 4: Multi-Layer Tracks

Already addressed in Task 3 with `Track::Vec<Layer>` structure.

**Key benefits:**
- Multiple non-overlapping clips per track
- User can organize related clips
- Track-level operations (lock, mute, color)
- DAW-like workflow

**Auto-layout preserved:**
- `migrate_to_tracks()` uses greedy algorithm
- New layers auto-assigned to fitting track
- Optional: `track_id` hint in layer attrs for user preference

---

## Task 5: Node Editor Integration

### 5.1 Crate Selection

**Recommended:** `egui-snarl 0.9.0`

```toml
# Cargo.toml
egui-snarl = { version = "0.9", features = ["serde"] }
```

**Reasons:**
- egui 0.33 compatible
- Active development (Jan 2025)
- Built-in serde for save/load
- Professional wire rendering

### 5.2 Architecture: Second Tab in Timeline

**File:** `src/widgets/timeline/mod.rs`

```rust
pub enum TimelineTab {
    Timeline,
    NodeGraph,
}

pub struct TimelineWidget {
    pub tab: TimelineTab,
    pub timeline_state: TimelineState,
    pub node_state: NodeGraphState,
    // ...
}
```

**Shared data model:**
- Both tabs operate on `Project.media`
- Same `Comp` with same `tracks`
- Edits in one view reflect immediately in other
- No data duplication

### 5.3 Node Graph Representation

```rust
use egui_snarl::{Snarl, SnarlViewer};

/// Node types in the graph
#[derive(Clone, Serialize, Deserialize)]
pub enum CompNode {
    /// Source composition (file or layer comp)
    Source { comp_uuid: Uuid },
    /// Current composition being viewed
    Output { comp_uuid: Uuid },
}

/// Node graph state for current comp
pub struct NodeGraphState {
    pub snarl: Snarl<CompNode>,
    pub comp_uuid: Uuid,  // Currently displayed comp
}

impl NodeGraphState {
    /// Rebuild graph from comp hierarchy
    pub fn rebuild_from_comp(&mut self, comp: &Comp, project: &Project) {
        self.snarl = Snarl::new();

        // Add output node (current comp)
        let output_id = self.snarl.insert_node(
            egui::pos2(400.0, 200.0),
            CompNode::Output { comp_uuid: comp.get_uuid() }
        );

        // Add source nodes for each layer
        let mut y = 50.0;
        for layer in comp.all_layers() {
            if let Some(source_uuid) = layer.source_uuid() {
                let source_id = self.snarl.insert_node(
                    egui::pos2(100.0, y),
                    CompNode::Source { comp_uuid: source_uuid }
                );

                // Connect source to output
                self.snarl.connect(source_id, 0, output_id, 0);

                y += 60.0;
            }
        }
    }

    /// Sync graph changes back to comp
    pub fn sync_to_comp(&self, comp: &mut Comp, project: &Project) {
        // When connections change in node graph, update comp.tracks
        // When node deleted, remove corresponding layer
        // etc.
    }
}
```

### 5.4 UI Integration

```rust
// In timeline widget show()
fn show(&mut self, ui: &mut egui::Ui, project: &mut Project) {
    ui.horizontal(|ui| {
        ui.selectable_value(&mut self.tab, TimelineTab::Timeline, "Timeline");
        ui.selectable_value(&mut self.tab, TimelineTab::NodeGraph, "Node Graph");
    });

    ui.separator();

    match self.tab {
        TimelineTab::Timeline => {
            self.timeline_state.show(ui, project);
        }
        TimelineTab::NodeGraph => {
            self.node_state.show(ui, project);
        }
    }
}
```

---

## Implementation Order

| Priority | Task | Effort | Dependencies |
|----------|------|--------|--------------|
| 1 | `check_collisions()` | Low | None |
| 2 | Red drop preview | Low | Task 1 |
| 3 | AddLayerEvent check | Low | Task 1 |
| 4 | DFS Iterator | Low | None |
| 5 | Layer struct | Medium | None |
| 6 | Track struct | Medium | Task 5 |
| 7 | Comp migration | Medium | Task 5, 6 |
| 8 | Two compose modes | Medium | Task 4 |
| 9 | Prefs setting | Low | Task 8 |
| 10 | Node Editor tab | High | Task 4-7 |

---

## Checklist

### Task 1: Cyclic Dependency
- [ ] Add `Comp::check_collisions(potential_child, project, hier)` method
- [ ] Add `would_cycle` parameter to `draw_drop_preview()`
- [ ] Update timeline drag handling to check collisions
- [ ] Update AddLayerEvent handler with collision check
- [ ] Add `planned_render_mode` to SystemPrefs
- [ ] Implement `compose_recursive()` with visited set
- [ ] Implement `compose_planned()` with render plan
- [ ] Add Prefs UI toggle for render mode

### Task 2: DFS Iterator
- [ ] Create `CompDfsIter` struct
- [ ] Implement `Iterator` trait
- [ ] Add `with_max_depth()` builder
- [ ] Add `Comp::iter_dfs()` method
- [ ] Refactor `check_collisions()` to use iterator

### Task 3: Layer/Track Architecture
- [ ] Create `src/entities/layer.rs`
- [ ] Define `Layer` struct wrapping Attrs
- [ ] Define `Track` struct with `Vec<Layer>`
- [ ] Add helper methods to Layer
- [ ] Add `can_place()`, `add_layer()` to Track
- [ ] Update Comp: replace `children` with `tracks`
- [ ] Implement `migrate_to_tracks()`
- [ ] Add `all_layers()` / `all_layers_mut()` iterators
- [ ] Update all code referencing `children`

### Task 4: Multi-Layer Tracks
- [ ] Covered by Task 3 implementation

### Task 5: Node Editor
- [ ] Add `egui-snarl` to Cargo.toml
- [ ] Create `TimelineTab` enum
- [ ] Create `NodeGraphState` struct
- [ ] Implement `rebuild_from_comp()`
- [ ] Implement `sync_to_comp()`
- [ ] Add tab UI to timeline widget
- [ ] Implement SnarlViewer for CompNode

---

## Key Files Reference

| File | Purpose | Key Lines |
|------|---------|-----------|
| `src/entities/comp.rs` | Main Comp struct | compose @ 1374, add_child_layer @ 1613 |
| `src/main_events.rs` | Event handlers | AddLayerEvent @ 479 |
| `src/widgets/timeline/timeline_ui.rs` | Timeline rendering | draw_drop_preview @ 1076 |
| `src/entities/attrs.rs` | Attribute storage | AttrValue enum, dirty tracking |
| `src/entities/keys.rs` | Attribute constants | A_UUID, A_IN, A_OUT, etc. |
| `src/entities/prefs.rs` | User preferences | SystemPrefs struct |

---

## Notes

1. **Backward compatibility:** Migration from `Vec<(Uuid, Attrs)>` to `Vec<Track>` via serde defaults
2. **Performance:** DFS iterator is lazy, doesn't allocate full tree
3. **Thread safety:** `check_collisions()` takes read lock on media
4. **UI consistency:** Both Timeline and Node Graph show same data, no sync issues
