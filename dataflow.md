# Playa Compositing Dataflow

## 1. Структура данных

```
Project
├── media: HashMap<Uuid, NodeKind>     # Все ноды (FileNode, CompNode)
├── global_cache: GlobalFrameCache     # Кеш фреймов по (comp_uuid, frame_idx)
└── cache_manager: CacheManager        # Управление памятью, epoch

CompNode
├── attrs: Attrs                       # width, height, fps, in, out, trim_in, trim_out
├── layers: Vec<Layer>                 # Слои (порядок: 0=bottom, N=top)
└── каждый Layer:
    ├── uuid: Uuid
    ├── source_uuid: Uuid              # Ссылка на FileNode или другой CompNode
    └── attrs: Attrs                   # in, trim_in, trim_out, speed, opacity, blend_mode

GlobalFrameCache
├── cache: HashMap<Uuid, HashMap<i32, Frame>>   # comp_uuid → (frame_idx → Frame)
├── lru_order: IndexSet<CacheKey>               # LRU eviction queue
└── cache_manager: Arc<CacheManager>            # Memory tracking
```

## 2. Playhead Movement (Scrubbing/Playback)

```
Player::set_frame(new_frame)
    │
    ▼
player.current_frame() returns new_frame
    │
    ▼
render_viewport_tab() checks:
    frame_changed = (last_rendered_frame != current_frame)
    │
    ▼ (if frame_changed)
player.get_current_frame(&project)
    │
    ▼
active_comp.compute(frame_idx, ctx)
    │
    ▼
Frame returned → texture uploaded → viewport rendered
```

## 3. CompNode::compute() - Core Logic

```rust
fn compute(&self, frame_idx: i32, ctx: &ComputeContext) -> Option<Frame> {
    // 1. Work area bounds check
    let (work_start, work_end) = self.work_area();
    if frame_idx < work_start || frame_idx > work_end {
        return None;
    }
    
    // 2. Dirty flag checks
    let any_layer_dirty = self.layers.iter().any(|l| l.attrs.is_dirty());
    let any_source_dirty = self.layers.iter().any(|l| 
        ctx.media.get(&l.source_uuid)
            .map(|n| n.is_dirty())
            .unwrap_or(false)
    );
    
    // 3. Cache lookup
    let cached_frame = ctx.cache.get(self.uuid(), frame_idx);
    let cache_is_loading = cached_frame.as_ref()
        .map(|f| f.status() != FrameStatus::Loaded)
        .unwrap_or(false);

    // 4. Recompute decision
    let needs_recompute = self.attrs.is_dirty()
        || any_layer_dirty
        || any_source_dirty
        || cached_frame.is_none()
        || cache_is_loading;
    
    // 5. Return cached if no recompute needed
    if !needs_recompute {
        if let Some(frame) = cached_frame {
            return Some(frame);
        }
    }
    
    // 6. Compositing
    let composed = self.compose_internal(frame_idx, ctx)?;
    
    // 7. Cache result
    ctx.cache.insert(self.uuid(), frame_idx, composed.clone());
    
    // 8. Clear dirty flags (comp AND all layers)
    self.attrs.clear_dirty();
    for layer in &self.layers {
        layer.attrs.clear_dirty();
    }
    
    Some(composed)
}
```

## 4. compose_internal() - Frame Assembly

```rust
fn compose_internal(&self, frame_idx: i32, ctx: &ComputeContext) -> Option<Frame> {
    let mut source_frames: Vec<(Frame, f32, BlendMode)> = Vec::new();
    
    // Iterate in REVERSE order (top→bottom for correct blend order)
    // layers[N-1] = top (foreground), layers[0] = bottom (background)
    for layer in self.layers.iter().rev() {
        
        // Skip if frame outside layer's work area
        let (play_start, play_end) = layer.work_area();
        if frame_idx < play_start || frame_idx > play_end {
            continue;  // Layer NOT visible at this frame!
        }
        
        // Skip invisible layers
        if !layer.is_visible() {
            continue;
        }
        
        // Get source node (FileNode or nested CompNode)
        let source_node = ctx.media.get(&layer.source_uuid)?;
        
        // Convert parent frame → source frame (accounting for speed, trim)
        let local_frame = layer.parent_to_local(frame_idx);
        let source_in = source_node.attrs().get_i32(A_IN).unwrap_or(0);
        let source_frame = source_in + local_frame;
        
        // Recursively compute source (if CompNode → recursion)
        if let Some(frame) = source_node.compute(source_frame, ctx) {
            let opacity = layer.opacity();
            let blend = layer.blend_mode();
            source_frames.push((frame, opacity, blend));
        }
    }
    
    // Determine output dimensions from earliest layer
    let dim = earliest_layer_dimensions.unwrap_or(self.dim());
    
    // Add black base
    let base = create_base_frame(dim, target_format);
    source_frames.insert(0, (base, 1.0, BlendMode::Normal));
    
    // Blend all layers
    compositor.blend_with_dim(source_frames, dim)
}
```

## 5. Attribute Change Flow (Layer Move)

```
User drags layer in Timeline
    │
    ▼
Timeline UI dispatches:
    MoveAndReorderLayerEvent { comp_uuid, layer_idx, new_start, new_idx }
    │
    ▼
Event handler in main_events.rs:
    project.modify_comp(comp_uuid, |comp| {
        let uuid = comp.idx_to_uuid(layer_idx);
        let delta = new_start - current_in;
        comp.move_layers(&[uuid], delta);
        // Inside move_layers():
        //   layer.attrs.set(A_IN, new_value)  → marks layer dirty
        //   self.mark_dirty()                  → marks comp dirty
    })
    │
    ▼
modify_comp() checks: comp.is_dirty()?
    │
    ▼ (if dirty)
emit AttrsChangedEvent(comp_uuid)
    │
    ▼
AttrsChangedEvent handler in main.rs:
    1. cache_manager.increment_epoch()    # Cancels pending worker tasks
    2. cache.clear_comp(comp_uuid)        # Removes ALL cached frames for this comp
    3. emit ViewportRefreshEvent
    │
    ▼
ViewportRefreshEvent handler:
    viewport_state.request_refresh()
        → last_rendered_epoch = 0
        → last_rendered_frame = None
    │
    ▼
Next frame: render_viewport_tab()
    epoch_changed = (0 != current_epoch) → true
    │
    ▼
Re-fetch frame via compute() → compose_internal() → new render
```

## 6. Dirty Flag System

### Setting Dirty
```rust
// attrs.set() - marks dirty (for render-affecting changes)
layer.attrs.set(A_IN, AttrValue::Int(new_value));
// Internally: self.dirty.store(true, Ordering::Relaxed)

// attrs.set_silent() - does NOT mark dirty (for UI-only state)
comp.attrs.set_silent(A_FRAME, AttrValue::Int(playhead_pos));
```

### Checking Dirty
```rust
// CompNode::is_dirty() checks BOTH comp attrs AND all layer attrs
fn is_dirty(&self) -> bool {
    self.attrs.is_dirty() || self.layers.iter().any(|l| l.attrs.is_dirty())
}
```

### Clearing Dirty
```rust
// Called after successful compose in compute()
self.attrs.clear_dirty();
for layer in &self.layers {
    layer.attrs.clear_dirty();
}
```

## 7. Cache Invalidation

### Epoch-based Staleness Detection
```
CacheManager tracks global epoch counter.
When attributes change:
    increment_epoch() → epoch++
    
Workers check epoch before returning results:
    if task_epoch != current_epoch → discard (stale)
```

### Cache Structure
```
GlobalFrameCache: HashMap<Uuid, HashMap<i32, Frame>>
                  ─────────────  ──────────────────
                  comp_uuid      frame_idx → Frame

clear_comp(uuid)  → O(1) removes entire inner HashMap
clear_range(uuid, start, end) → removes specific frames
```

## 8. Layer Visibility at Frame

**Critical concept:** A layer is only composited if the current frame falls within its work area.

```
Layer work_area calculation:
    play_start = layer.attrs.in
    play_end = layer.attrs.in + (source_duration / speed) - trim_in - trim_out

If frame_idx is outside [play_start, play_end]:
    → Layer is SKIPPED in compose_internal()
    → Moving this layer does NOT change render at current frame
    → This is CORRECT behavior!
```

### Example
```
Playhead at frame 50

Layer A: work_area = (0, 100)    → frame 50 is INSIDE  → composited
Layer B: work_area = (100, 200)  → frame 50 is OUTSIDE → skipped
Layer C: work_area = (40, 80)    → frame 50 is INSIDE  → composited

Moving Layer B will NOT change the render at frame 50
because Layer B is not visible at that frame.
```

## 9. Nested Composition (CompNode as Layer Source)

```
MainComp
├── Layer 0: source = FileNode (video.mp4)
├── Layer 1: source = SubComp              ← Nested CompNode!
│   └── SubComp
│       ├── Layer 0: source = FileNode (overlay.png)
│       └── Layer 1: source = FileNode (text.exr)
└── Layer 2: source = FileNode (background.jpg)

When MainComp.compute(frame) is called:
    → For Layer 1, calls SubComp.compute(local_frame)
        → SubComp recursively computes its layers
        → Returns composed Frame
    → MainComp blends all source frames together
```

## 10. File Locations

| Component | File |
|-----------|------|
| CompNode, compose_internal | `src/entities/comp_node.rs` |
| Project, modify_comp | `src/entities/project.rs` |
| Attrs, dirty flags | `src/entities/attrs.rs` |
| GlobalFrameCache | `src/core/global_cache.rs` |
| CacheManager | `src/core/cache_man.rs` |
| Event handlers | `src/main_events.rs` |
| Viewport render | `src/main.rs:render_viewport_tab()` |
| Timeline UI | `src/widgets/timeline/timeline_ui.rs` |
