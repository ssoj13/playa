# Playa Architecture v2 - Implementation Plan

## Final Decisions

| Item | Decision |
|------|----------|
| Mode | `i8` constants in `keys.rs`: `COMP_NORMAL=0`, `COMP_FILE=1` |
| UUID | `AttrValue::Uuid(Uuid)` - new variant |
| Node | Trait-based: `trait Node { attrs(), data(), compute() }` |
| Children | `Vec<Attrs>` - each child is full Attrs from `new_comp()` |
| Constructor | Single `new_comp()` with ALL attributes (mode determines behavior) |
| EventBus | Multi-thread pub/sub, event structs per module |
| Accessors | None - direct `comp.core.attrs.get_i32("in")` |
| Compatibility | New format only, remove old |

---

## Phase 1: AttrValue Extensions

**File:** `src/entities/attrs.rs`

### 1.1 Add new AttrValue variants

```rust
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AttrValue {
    // Existing
    Bool(bool),
    Str(String),
    Int(i32),
    UInt(u32),
    Float(f32),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    Mat3([[f32; 3]; 3]),
    Mat4([[f32; 4]; 4]),
    Json(String),
    
    // NEW
    Uuid(Uuid),
    List(Vec<AttrValue>),
}
```

### 1.2 Add Attrs helper methods

```rust
impl Attrs {
    // Uuid helpers
    pub fn get_uuid(&self, key: &str) -> Option<Uuid> {
        match self.map.get(key) {
            Some(AttrValue::Uuid(v)) => Some(*v),
            _ => None,
        }
    }
    
    pub fn set_uuid(&mut self, key: impl Into<String>, value: Uuid) {
        self.set(key, AttrValue::Uuid(value));
    }
    
    // List helpers
    pub fn get_list(&self, key: &str) -> Option<&Vec<AttrValue>> {
        match self.map.get(key) {
            Some(AttrValue::List(v)) => Some(v),
            _ => None,
        }
    }
    
    pub fn get_list_mut(&mut self, key: &str) -> Option<&mut Vec<AttrValue>> {
        match self.map.get_mut(key) {
            Some(AttrValue::List(v)) => Some(v),
            _ => None,
        }
    }
    
    pub fn set_list(&mut self, key: impl Into<String>, value: Vec<AttrValue>) {
        self.set(key, AttrValue::List(value));
    }
    
    // i8 helpers (for mode constants)
    pub fn get_i8(&self, key: &str) -> Option<i8> {
        self.get_i32(key).map(|v| v as i8)
    }
    
    pub fn set_i8(&mut self, key: impl Into<String>, value: i8) {
        self.set(key, AttrValue::Int(value as i32));
    }
}
```

### 1.3 Update Hash impl for new variants

```rust
impl Hash for AttrValue {
    fn hash<H: Hasher>(&self, state: &mut H) {
        use AttrValue::*;
        std::mem::discriminant(self).hash(state);
        match self {
            // ... existing ...
            Uuid(v) => v.hash(state),
            List(v) => {
                v.len().hash(state);
                for item in v {
                    item.hash(state);
                }
            }
        }
    }
}
```

---

## Phase 2: Keys & Constants

**File:** `src/entities/keys.rs`

```rust
//! Attribute keys and constants

// === Comp Mode Constants ===
pub const COMP_NORMAL: i8 = 0;  // Layer composition mode
pub const COMP_FILE: i8 = 1;    // File/sequence loading mode

// === Attribute Keys ===
pub const A_UUID: &str = "uuid";
pub const A_NAME: &str = "name";
pub const A_MODE: &str = "mode";
pub const A_FRAME: &str = "frame";
pub const A_FPS: &str = "fps";

// Timeline bounds
pub const A_IN: &str = "in";
pub const A_OUT: &str = "out";
pub const A_TRIM_IN: &str = "trim_in";
pub const A_TRIM_OUT: &str = "trim_out";

// Compose flags
pub const A_SOLO: &str = "solo";
pub const A_MUTE: &str = "mute";
pub const A_VISIBLE: &str = "visible";
pub const A_OPACITY: &str = "opacity";
pub const A_BLEND_MODE: &str = "blend_mode";

// Transform
pub const A_POSITION: &str = "position";
pub const A_ROTATION: &str = "rotation";
pub const A_SCALE: &str = "scale";
pub const A_PIVOT: &str = "pivot";

// Playback
pub const A_SPEED: &str = "speed";

// Relationships
pub const A_SOURCE_UUID: &str = "source_uuid";
pub const A_CHILDREN: &str = "children";
pub const A_PARENT: &str = "parent";

// File mode
pub const A_FILE_MASK: &str = "file_mask";
pub const A_FILE_START: &str = "file_start";
pub const A_FILE_END: &str = "file_end";

// Dimensions
pub const A_WIDTH: &str = "width";
pub const A_HEIGHT: &str = "height";
```

---

## Phase 3: Node Trait & NodeCore

**File:** `src/entities/node.rs` (NEW)

```rust
//! Base node trait and core structure for all composable entities.

use super::{Attrs, AttrValue};
use super::keys::*;
use uuid::Uuid;

/// Shared data for all node types
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NodeCore {
    /// Persistent attributes (serialized)
    pub attrs: Attrs,
    
    /// Transient runtime data (not serialized)
    #[serde(skip, default)]
    pub data: Attrs,
}

impl Default for NodeCore {
    fn default() -> Self {
        Self {
            attrs: Attrs::new(),
            data: Attrs::new(),
        }
    }
}

impl NodeCore {
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Create with initial attributes
    pub fn with_attrs(attrs: Attrs) -> Self {
        Self {
            attrs,
            data: Attrs::new(),
        }
    }
}

/// Context passed to compute()
pub struct ComputeContext<'a> {
    pub project: &'a crate::entities::Project,
}

/// Base trait for all node types (Comp, Project, etc.)
pub trait Node {
    /// Get persistent attributes
    fn attrs(&self) -> &Attrs;
    
    /// Get mutable persistent attributes
    fn attrs_mut(&mut self) -> &mut Attrs;
    
    /// Get transient runtime data
    fn data(&self) -> &Attrs;
    
    /// Get mutable transient data
    fn data_mut(&mut self) -> &mut Attrs;
    
    /// Get node UUID
    fn uuid(&self) -> Uuid {
        self.attrs().get_uuid(A_UUID).unwrap_or_else(Uuid::nil)
    }
    
    /// Compute/update node state
    fn compute(&mut self, ctx: &ComputeContext) -> anyhow::Result<()> {
        Ok(()) // Default: no-op
    }
    
    /// Check if node needs recomputation
    fn is_dirty(&self) -> bool {
        self.attrs().is_dirty()
    }
    
    /// Clear dirty flag
    fn clear_dirty(&self) {
        self.attrs().clear_dirty();
    }
    
    /// Mark as dirty
    fn mark_dirty(&self) {
        self.attrs().mark_dirty();
    }
}
```

---

## Phase 4: Comp Refactor

**File:** `src/entities/comp.rs`

### 4.1 New Comp structure

```rust
use super::node::{Node, NodeCore, ComputeContext};
use super::keys::*;
use super::{Attrs, AttrValue};
use uuid::Uuid;

/// Composition - unified container for layers and file sequences
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Comp {
    /// Core node data (attrs + transient data)
    pub core: NodeCore,
    
    /// Children layers - each is full Attrs with uuid, source_uuid, timing, etc.
    /// Stored separately for efficient mutation (not in attrs)
    #[serde(default)]
    pub children: Vec<Attrs>,
    
    // === Runtime-only fields ===
    #[serde(skip)]
    event_sender: CompEventSender,
    
    #[serde(skip)]
    cache_manager: Option<Arc<CacheManager>>,
    
    #[serde(skip)]
    global_cache: Option<Arc<GlobalFrameCache>>,
}

impl Node for Comp {
    fn attrs(&self) -> &Attrs { &self.core.attrs }
    fn attrs_mut(&mut self) -> &mut Attrs { &mut self.core.attrs }
    fn data(&self) -> &Attrs { &self.core.data }
    fn data_mut(&mut self) -> &mut Attrs { &mut self.core.data }
    
    fn compute(&mut self, ctx: &ComputeContext) -> anyhow::Result<()> {
        // Composition logic here
        Ok(())
    }
}
```

### 4.2 Unified constructor

```rust
impl Comp {
    /// Create comp with ALL attributes (unified schema).
    /// Mode determines get_frame() behavior: COMP_NORMAL -> compose(), COMP_FILE -> loader()
    /// All attributes always present, unused ones have default/nil values.
    pub fn new_comp(name: &str, start: i32, end: i32, fps: f32) -> Self {
        let mut attrs = Attrs::new();
        
        // === Identity ===
        attrs.set_uuid(A_UUID, Uuid::new_v4());
        attrs.set(A_NAME, AttrValue::Str(name.to_string()));
        attrs.set_i8(A_MODE, COMP_NORMAL);  // Default: layer composition
        
        // === Timeline ===
        attrs.set(A_IN, AttrValue::Int(start));
        attrs.set(A_OUT, AttrValue::Int(end));
        attrs.set(A_TRIM_IN, AttrValue::Int(start));
        attrs.set(A_TRIM_OUT, AttrValue::Int(end));
        attrs.set(A_FPS, AttrValue::Float(fps));
        attrs.set(A_FRAME, AttrValue::Int(start));
        
        // === Compose flags ===
        attrs.set(A_SOLO, AttrValue::Bool(false));
        attrs.set(A_MUTE, AttrValue::Bool(false));
        attrs.set(A_VISIBLE, AttrValue::Bool(true));
        attrs.set(A_OPACITY, AttrValue::Float(1.0));
        attrs.set(A_BLEND_MODE, AttrValue::Str("normal".to_string()));
        
        // === Transform ===
        attrs.set(A_POSITION, AttrValue::Vec3([0.0, 0.0, 0.0]));
        attrs.set(A_ROTATION, AttrValue::Vec3([0.0, 0.0, 0.0]));
        attrs.set(A_SCALE, AttrValue::Vec3([1.0, 1.0, 1.0]));
        attrs.set(A_PIVOT, AttrValue::Vec3([0.0, 0.0, 0.0]));
        
        // === Playback ===
        attrs.set(A_SPEED, AttrValue::Float(1.0));
        
        // === Relationships (always present, nil when unused) ===
        attrs.set_uuid(A_SOURCE_UUID, Uuid::nil());  // nil = no source
        attrs.set_uuid(A_PARENT, Uuid::nil());       // nil = root level
        
        // === File mode attrs (always present, empty when unused) ===
        attrs.set(A_FILE_MASK, AttrValue::Str(String::new()));  // empty = no file
        attrs.set(A_FILE_START, AttrValue::Int(0));
        attrs.set(A_FILE_END, AttrValue::Int(0));
        
        // === Dimensions (0 = auto-detect from content) ===
        attrs.set(A_WIDTH, AttrValue::Int(0));
        attrs.set(A_HEIGHT, AttrValue::Int(0));
        
        Self {
            core: NodeCore::with_attrs(attrs),
            children: Vec::new(),
            event_sender: CompEventSender::dummy(),
            cache_manager: None,
            global_cache: None,
        }
    }
}

// === get_frame() dispatches based on mode ===
impl Comp {
    pub fn get_frame(&self, frame: i32, ctx: &ComputeContext) -> Option<Frame> {
        let mode = self.core.attrs.get_i8(A_MODE).unwrap_or(COMP_NORMAL);
        
        match mode {
            COMP_NORMAL => self.compose(frame, ctx),
            COMP_FILE => self.load(frame, ctx),
            _ => None,
        }
    }
    
    /// Compose children layers (COMP_NORMAL mode)
    fn compose(&self, frame: i32, ctx: &ComputeContext) -> Option<Frame> {
        // ... composition logic
    }
    
    /// Load frame from file/sequence (COMP_FILE mode)
    fn load(&self, frame: i32, ctx: &ComputeContext) -> Option<Frame> {
        // ... loader logic using A_FILE_MASK, A_FILE_START, etc.
    }
}
```

### 4.3 Time conversion methods

```rust
impl Comp {
    /// Convert comp frame to child's local frame.
    /// Accounts for child's "in" point and "speed".
    ///
    /// # Example
    /// ```
    /// // child.in=20, child.speed=2.0, comp_frame=50
    /// // local = (50 - 20) * 2.0 = 60
    /// ```
    #[inline]
    pub fn comp2local(&self, child_idx: usize, comp_frame: i32) -> Option<i32> {
        let child = self.children.get(child_idx)?;
        let child_in = child.get_i32(A_IN).unwrap_or(0);
        let speed = child.get_float(A_SPEED).unwrap_or(1.0);
        
        let offset = comp_frame - child_in;
        Some((offset as f32 * speed).round() as i32)
    }
    
    /// Convert child's local frame to comp frame.
    /// Inverse of comp2local.
    ///
    /// # Example
    /// ```
    /// // child.in=20, child.speed=2.0, local_frame=60
    /// // comp = 60 / 2.0 + 20 = 50
    /// ```
    #[inline]
    pub fn local2comp(&self, child_idx: usize, local_frame: i32) -> Option<i32> {
        let child = self.children.get(child_idx)?;
        let child_in = child.get_i32(A_IN).unwrap_or(0);
        let speed = child.get_float(A_SPEED).unwrap_or(1.0);
        
        if speed.abs() < 0.0001 {
            return Some(child_in);
        }
        
        Some((local_frame as f32 / speed).round() as i32 + child_in)
    }
    
    /// Get child's visible range in comp coordinates.
    #[inline]
    pub fn child_range(&self, child_idx: usize) -> Option<(i32, i32)> {
        let child = self.children.get(child_idx)?;
        let in_pt = child.get_i32(A_IN).unwrap_or(0);
        let out_pt = child.get_i32(A_OUT).unwrap_or(100);
        Some((in_pt, out_pt))
    }
}
```

### 4.4 Time conversion tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    /// Helper: create child attrs with timing
    fn make_child(start: i32, end: i32) -> Attrs {
        let mut child = Comp::new_comp("child", start, end, 24.0).core.attrs;
        child.set(A_IN, AttrValue::Int(start));
        child.set(A_OUT, AttrValue::Int(end));
        child
    }
    
    #[test]
    fn test_comp2local_basic() {
        let mut comp = Comp::new_comp("Test", 0, 100, 24.0);
        comp.children.push(make_child(20, 80));
        
        // comp_frame=50, child.in=20, speed=1.0
        // local = (50 - 20) * 1.0 = 30
        assert_eq!(comp.comp2local(0, 50), Some(30));
        assert_eq!(comp.comp2local(0, 20), Some(0));  // at child start
        assert_eq!(comp.comp2local(0, 80), Some(60)); // at child end
    }
    
    #[test]
    fn test_comp2local_with_speed() {
        let mut comp = Comp::new_comp("Test", 0, 100, 24.0);
        let mut child = make_child(20, 80);
        child.set(A_SPEED, AttrValue::Float(2.0));
        comp.children.push(child);
        
        // comp_frame=50, child.in=20, speed=2.0
        // local = (50 - 20) * 2.0 = 60
        assert_eq!(comp.comp2local(0, 50), Some(60));
    }
    
    #[test]
    fn test_local2comp_basic() {
        let mut comp = Comp::new_comp("Test", 0, 100, 24.0);
        comp.children.push(make_child(20, 80));
        
        // local_frame=30, child.in=20, speed=1.0
        // comp = 30 / 1.0 + 20 = 50
        assert_eq!(comp.local2comp(0, 30), Some(50));
    }
    
    #[test]
    fn test_local2comp_with_speed() {
        let mut comp = Comp::new_comp("Test", 0, 100, 24.0);
        let mut child = make_child(20, 80);
        child.set(A_SPEED, AttrValue::Float(2.0));
        comp.children.push(child);
        
        // local_frame=60, child.in=20, speed=2.0
        // comp = 60 / 2.0 + 20 = 50
        assert_eq!(comp.local2comp(0, 60), Some(50));
    }
    
    #[test]
    fn test_roundtrip() {
        let mut comp = Comp::new_comp("Test", 0, 100, 24.0);
        let mut child = make_child(10, 90);
        child.set(A_SPEED, AttrValue::Float(1.5));
        comp.children.push(child);
        
        // comp -> local -> comp should roundtrip
        for comp_frame in [10, 25, 50, 75, 90] {
            let local = comp.comp2local(0, comp_frame).unwrap();
            let back = comp.local2comp(0, local).unwrap();
            assert_eq!(back, comp_frame, "Roundtrip failed for frame {}", comp_frame);
        }
    }
    
    #[test]
    fn test_invalid_child_idx() {
        let comp = Comp::new_comp("Test", 0, 100, 24.0);
        assert_eq!(comp.comp2local(0, 50), None);
        assert_eq!(comp.local2comp(0, 50), None);
    }
    
    #[test]
    fn test_negative_frames() {
        let mut comp = Comp::new_comp("Test", -50, 50, 24.0);
        let mut child = make_child(-20, 30);
        comp.children.push(child);
        
        // child.in=-20, comp_frame=0
        // local = (0 - (-20)) * 1.0 = 20
        assert_eq!(comp.comp2local(0, 0), Some(20));
        assert_eq!(comp.comp2local(0, -20), Some(0));
    }
    
    #[test]
    fn test_mode_dispatch() {
        let mut comp = Comp::new_comp("Test", 0, 100, 24.0);
        assert_eq!(comp.core.attrs.get_i8(A_MODE), Some(COMP_NORMAL as i8));
        
        // Switch to file mode
        comp.core.attrs.set_i8(A_MODE, COMP_FILE);
        comp.core.attrs.set(A_FILE_MASK, AttrValue::Str("/path/seq.*.exr".into()));
        assert_eq!(comp.core.attrs.get_i8(A_MODE), Some(COMP_FILE as i8));
    }
}
```

---

## Phase 5: EventBus

**File:** `src/event_bus.rs` (NEW)

```rust
//! Multi-threaded pub/sub event bus using crossbeam channels.

use crossbeam_channel::{Sender, Receiver, unbounded};
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Marker trait for events
pub trait Event: Send + Sync + Clone + 'static {}

/// Type-erased callback
type Callback = Arc<dyn Fn(&dyn Any) + Send + Sync>;

/// Multi-threaded event bus with pub/sub pattern.
///
/// # Example
/// ```
/// let bus = EventBus::new();
///
/// // Subscribe to events
/// bus.subscribe(|e: &PlayEvent| {
///     println!("Play started");
/// });
///
/// // Emit from any thread
/// bus.emit(PlayEvent { frame: 0 });
/// ```
#[derive(Clone)]
pub struct EventBus {
    subscribers: Arc<RwLock<HashMap<TypeId, Vec<Callback>>>>,
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            subscribers: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// Subscribe to events of type E
    pub fn subscribe<E: Event, F>(&self, callback: F)
    where
        F: Fn(&E) + Send + Sync + 'static,
    {
        let type_id = TypeId::of::<E>();
        let wrapped: Callback = Arc::new(move |any: &dyn Any| {
            if let Some(event) = any.downcast_ref::<E>() {
                callback(event);
            }
        });
        
        self.subscribers
            .write()
            .unwrap()
            .entry(type_id)
            .or_default()
            .push(wrapped);
    }
    
    /// Emit event to all subscribers
    pub fn emit<E: Event>(&self, event: E) {
        let type_id = TypeId::of::<E>();
        if let Some(callbacks) = self.subscribers.read().unwrap().get(&type_id) {
            for cb in callbacks {
                cb(&event);
            }
        }
    }
    
    /// Remove all subscribers for event type
    pub fn clear<E: Event>(&self) {
        let type_id = TypeId::of::<E>();
        self.subscribers.write().unwrap().remove(&type_id);
    }
    
    /// Remove all subscribers
    pub fn clear_all(&self) {
        self.subscribers.write().unwrap().clear();
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}
```

### Event definitions per module

**File:** `src/player_events.rs`
```rust
use crate::event_bus::Event;

#[derive(Clone, Debug)]
pub struct PlayEvent;
impl Event for PlayEvent {}

#[derive(Clone, Debug)]
pub struct PauseEvent;
impl Event for PauseEvent {}

#[derive(Clone, Debug)]
pub struct StopEvent;
impl Event for StopEvent {}

#[derive(Clone, Debug)]
pub struct FrameChangedEvent {
    pub frame: i32,
}
impl Event for FrameChangedEvent {}

#[derive(Clone, Debug)]
pub struct JogForwardEvent;
impl Event for JogForwardEvent {}

#[derive(Clone, Debug)]
pub struct JogBackwardEvent;
impl Event for JogBackwardEvent {}
```

**File:** `src/dialogs/encode/encode_events.rs`
```rust
use crate::event_bus::Event;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct EncodeStartEvent {
    pub output_path: PathBuf,
    pub codec: String,
}
impl Event for EncodeStartEvent {}

#[derive(Clone, Debug)]
pub struct EncodeCancelEvent;
impl Event for EncodeCancelEvent {}

#[derive(Clone, Debug)]
pub struct EncodeProgressEvent {
    pub frame: i32,
    pub total: i32,
}
impl Event for EncodeProgressEvent {}

#[derive(Clone, Debug)]
pub struct EncodeCompleteEvent {
    pub output_path: PathBuf,
}
impl Event for EncodeCompleteEvent {}
```

---

## Phase 6: Migration

### 6.1 Remove old code

- [x] Remove `AppEvent` enum from `events.rs` ✅ (file deleted)
- [x] Remove old accessors from `Comp` (is_layer_mode, get_mode, set_mode) ✅
- [ ] Remove `CompMode` enum (kept for serde backwards compat)
- [x] Update all `match event` handlers to EventBus subscribers ✅
- [x] Fix self.mode → is_file_mode() everywhere ✅

### 6.2 Update imports

```rust
// Old
use crate::events::AppEvent;

// New
use crate::event_bus::EventBus;
use crate::player_events::{PlayEvent, StopEvent, ...};
```
✅ Done

### 6.3 Cleanup (TODO)

57 warnings remaining:
- Unused events (EncodeStartEvent, MoveLayerEvent, etc.) - created but not sent yet
- Unused traits (ProjectUI, TimelineUI, etc.) - future UI infrastructure
- Unused methods (remove_media_and_cleanup, select_item, etc.)
- Unused fields (data in Comp, texture_cache, subscribers)

**Decision needed:** Remove unused code or add `#[allow(unused)]` for future-use

---

## Implementation Order

| # | Phase | Files | Effort | Status |
|---|-------|-------|--------|--------|
| 1 | AttrValue extensions | `attrs.rs` | Small | ✅ Done |
| 2 | Keys & constants | `keys.rs` | Small | ✅ Done |
| 3 | Node trait | `node.rs` (new) | Medium | ✅ Done (simplified, no NodeCore) |
| 4 | Comp refactor | `comp.rs` | Large | ✅ Done |
| 5 | EventBus | `event_bus.rs` (new) | Medium | ✅ Done |
| 6 | Event definitions | `*_events.rs` | Medium | ✅ Done |
| 7 | Migration | `main.rs`, all widgets | Large | ✅ Done |
| 8 | Tests | Throughout | Medium | ⏳ TODO |

**Total estimate:** ~2000 lines changed/added

---

## File Structure After

```
src/
├── entities/
│   ├── mod.rs
│   ├── attrs.rs        # +Uuid, +List variants
│   ├── keys.rs         # +COMP_NORMAL, COMP_FILE, all A_* constants
│   ├── node.rs         # NEW: Node trait, NodeCore, ComputeContext
│   ├── comp.rs         # Refactored: uses NodeCore
│   ├── project.rs      # Refactored: implements Node
│   ├── frame.rs
│   └── ...
├── event_bus.rs        # NEW: pub/sub EventBus
├── player.rs           # Refactored: implements Node
├── player_events.rs    # NEW: PlayEvent, StopEvent, etc.
├── events.rs           # REMOVED or minimal
├── dialogs/
│   └── encode/
│       ├── mod.rs
│       ├── encode.rs
│       ├── encode_ui.rs
│       └── encode_events.rs  # NEW
└── widgets/
    └── timeline/
        ├── timeline_events.rs  # NEW
        └── ...
```

---

## Questions Resolved

All questions answered. Ready to implement.
