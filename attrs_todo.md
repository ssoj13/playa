# Attribute System Refactoring

## STATUS: IMPLEMENTED

All phases completed. Schema-based dirty detection is now active.

---

## Problem

`modify_comp()` checks `is_dirty()` after any mutation and emits `AttrsChangedEvent`.
This causes cache invalidation even for non-rendering attributes like `frame` (playhead position).

**Current workaround:** `set_silent()` vs `set()` - manual choice by caller, error-prone.

---

## Entities with `attrs: Attrs`

| Entity | Purpose | Serialized | DAG-relevant |
|--------|---------|------------|--------------|
| **FileNode** | Loads image sequence/video from disk | Yes | Yes |
| **CompNode** | Composites layers into single frame | Yes | Yes |
| **Layer** | Reference to FileNode/CompNode inside CompNode | Yes | Yes |
| **Project** | Top-level container (media pool, order) | Yes | No (meta only) |
| **Player** | Playback state (playing, fps, loop) | Yes | No (UI state) |
| **Frame** | Pixel data for a frame | No | No (legacy attrs, unused) |

### FileNode Attributes
```
name        : Str    - display name
file_mask   : Str    - file pattern (e.g., "frame.%04d.exr")
file_dir    : Str    - directory path
file_start  : Int    - first file frame number
file_end    : Int    - last file frame number
fps         : Float  - frame rate
width       : UInt   - resolution
height      : UInt   - resolution
in          : Int    - timeline start
out         : Int    - timeline end
trim_in     : Int    - trim from start (source frames)
trim_out    : Int    - trim from end (source frames)
padding     : UInt   - frame number padding
```

### CompNode Attributes
```
name        : Str    - display name
uuid        : Uuid   - identity (readonly)
fps         : Float  - frame rate
width       : UInt   - resolution
height      : UInt   - resolution
in          : Int    - timeline start
out         : Int    - timeline end
trim_in     : Int    - work area trim start
trim_out    : Int    - work area trim end
frame       : Int    - PLAYHEAD POSITION (non-DAG!)
```

### Layer Attributes
```
name        : Str    - display name
uuid        : Uuid   - identity (readonly)
source_uuid : Uuid   - reference to FileNode/CompNode (internal)
in          : Int    - layer start on parent timeline
out         : Int    - layer end on parent timeline (computed from source + trims)
trim_in     : Int    - trim from source start (source frames)
trim_out    : Int    - trim from source end (source frames)
opacity     : Float  - 0.0-1.0
blend_mode  : Str    - "normal", "add", "multiply", "screen", etc.
visible     : Bool   - layer visibility
mute        : Bool   - audio mute
solo        : Bool   - solo mode
speed       : Float  - playback speed multiplier
position    : Vec3   - transform
rotation    : Vec3   - transform (radians)
scale       : Vec3   - transform
pivot       : Vec3   - transform anchor
```

### Project Attributes
```
comps_order : Json   - Vec<Uuid> - UI order of media items
selection   : Json   - Vec<Uuid> - current selection
active      : Json   - Option<Uuid> - currently active item
```

### Player Attributes
```
is_playing     : Bool  - playback state
fps_base       : Float - project fps
fps_play       : Float - playback fps
loop_enabled   : Bool  - loop mode
play_direction : Float - 1.0 or -1.0
```

---

## Proposed Architecture

### AttrFlags - Attribute properties

```rust
bitflags! {
    pub struct AttrFlags: u8 {
        const DAG      = 0b00001;  // Affects render - changes invalidate cache
        const DISPLAY  = 0b00010;  // Show in Attribute Editor
        const KEYABLE  = 0b00100;  // Can be animated (future)
        const READONLY = 0b01000;  // Cannot be modified by user
        const INTERNAL = 0b10000;  // Internal use only
    }
}
```

### AttrDef - Single attribute definition

```rust
pub struct AttrDef {
    pub key: &'static str,
    pub value_type: AttrType,
    pub default: fn() -> AttrValue,  // Factory for default value
    pub flags: AttrFlags,
}

pub enum AttrType {
    Bool, Str, Int, UInt, Float, Vec3, Vec4, Uuid, Json,
}
```

### AttrSchema - Collection of definitions

```rust
pub struct AttrSchema {
    defs: &'static [AttrDef],
    index: HashMap<&'static str, usize>,  // Built at init for O(1) lookup
}

impl AttrSchema {
    pub fn is_dag(&self, key: &str) -> bool;
    pub fn is_display(&self, key: &str) -> bool;
    pub fn default(&self, key: &str) -> Option<AttrValue>;
}
```

### Static schemas

```rust
lazy_static! {
    pub static ref FILE_SCHEMA: AttrSchema = AttrSchema::build(&[
        AttrDef::new("name", Str, || "".into(), DISPLAY),
        AttrDef::new("file_mask", Str, || "".into(), DAG | DISPLAY),
        AttrDef::new("width", UInt, || 0.into(), DAG | DISPLAY | READONLY),
        AttrDef::new("height", UInt, || 0.into(), DAG | DISPLAY | READONLY),
        // ...
    ]);
    
    pub static ref COMP_SCHEMA: AttrSchema = AttrSchema::build(&[
        AttrDef::new("name", Str, || "".into(), DISPLAY),
        AttrDef::new("frame", Int, || 0.into(), 0),  // NO FLAGS = non-DAG, non-display
        AttrDef::new("fps", Float, || 24.0.into(), DAG | DISPLAY),
        AttrDef::new("in", Int, || 0.into(), DAG | DISPLAY),
        AttrDef::new("out", Int, || 100.into(), DAG | DISPLAY),
        // ...
    ]);
    
    pub static ref LAYER_SCHEMA: AttrSchema = AttrSchema::build(&[
        AttrDef::new("name", Str, || "".into(), DISPLAY),
        AttrDef::new("source_uuid", Uuid, || Uuid::nil().into(), INTERNAL),
        AttrDef::new("opacity", Float, || 1.0.into(), DAG | DISPLAY | KEYABLE),
        AttrDef::new("blend_mode", Str, || "normal".into(), DAG | DISPLAY),
        // ...
    ]);
    
    pub static ref PROJECT_SCHEMA: AttrSchema = AttrSchema::build(&[
        AttrDef::new("comps_order", Json, || "[]".into(), INTERNAL),
        AttrDef::new("selection", Json, || "[]".into(), INTERNAL),
        AttrDef::new("active", Json, || "null".into(), INTERNAL),
    ]);
    
    pub static ref PLAYER_SCHEMA: AttrSchema = AttrSchema::build(&[
        AttrDef::new("is_playing", Bool, || false.into(), 0),
        AttrDef::new("fps_play", Float, || 24.0.into(), 0),
        // ...
    ]);
}
```

### Attrs - Runtime storage

```rust
pub struct Attrs {
    schema: Option<&'static AttrSchema>,  // None for legacy/Frame
    values: HashMap<String, AttrValue>,
    dirty: AtomicBool,
}

impl Attrs {
    pub fn new() -> Self;  // No schema (legacy)
    pub fn with_schema(schema: &'static AttrSchema) -> Self;
    
    /// Set attribute - auto-determines if DAG based on schema
    pub fn set(&mut self, key: &str, value: AttrValue) {
        self.values.insert(key.to_string(), value);
        // Only mark dirty if schema says this is a DAG attr
        if self.schema.map(|s| s.is_dag(key)).unwrap_or(true) {
            self.dirty.store(true, Ordering::SeqCst);
        }
    }
    
    // set_silent() becomes unnecessary - remove it
}
```

---

## Serialization Strategy

**Schema is NOT serialized** - it's static code.

**Values ARE serialized** - HashMap<String, AttrValue>

```rust
#[derive(Serialize, Deserialize)]
pub struct Attrs {
    #[serde(skip)]
    schema: Option<&'static AttrSchema>,
    
    #[serde(flatten)]
    values: HashMap<String, AttrValue>,
    
    #[serde(skip)]
    dirty: AtomicBool,
}
```

**After deserialization:** Each entity sets its schema:

```rust
impl FileNode {
    pub fn attach_schema(&mut self) {
        self.attrs.set_schema(&FILE_SCHEMA);
    }
}

// Called after Project::from_json():
impl Project {
    pub fn attach_schemas(&mut self) {
        self.attrs.set_schema(&PROJECT_SCHEMA);
        for node in self.media.values_mut() {
            match node {
                NodeKind::File(f) => f.attrs.set_schema(&FILE_SCHEMA),
                NodeKind::Comp(c) => {
                    c.attrs.set_schema(&COMP_SCHEMA);
                    for layer in &mut c.layers {
                        layer.attrs.set_schema(&LAYER_SCHEMA);
                    }
                }
            }
        }
    }
}
```

---

## DAG vs Non-DAG Classification

### DAG (cache invalidation on change)
- `in`, `out`, `trim_in`, `trim_out` - timing
- `opacity`, `blend_mode`, `visible`, `mute`, `solo` - compositing
- `position`, `rotation`, `scale`, `pivot` - transform
- `speed` - affects source frame mapping
- `width`, `height`, `fps` - resolution/timing
- `file_mask`, `file_start`, `file_end` - source data
- `source_uuid` - layer source (internal but DAG)

### Non-DAG (no cache invalidation)
- `frame` - playhead position
- `name` - display only
- `uuid` - identity (never changes after creation)
- `comps_order`, `selection`, `active` - project UI state
- `is_playing`, `fps_play`, `loop_enabled`, `play_direction` - player state

---

## Migration Plan

### Phase 1: Infrastructure (attrs.rs) - DONE
- [x] Add `AttrFlags` (u8 bitfield with FLAG_DAG, FLAG_DISPLAY, etc.)
- [x] Add `AttrType` enum
- [x] Add `AttrDef` struct
- [x] Add `AttrSchema` struct with `is_dag()`, `is_display()`
- [x] Update `Attrs` to hold optional `schema: Option<&'static AttrSchema>`
- [x] Update `Attrs::set()` to check schema before marking dirty

### Phase 2: Define schemas (attr_schemas.rs) - DONE
- [x] Define `FILE_SCHEMA`
- [x] Define `COMP_SCHEMA` (with `frame` as non-DAG!)
- [x] Define `LAYER_SCHEMA`
- [x] Define `PROJECT_SCHEMA`
- [x] Define `PLAYER_SCHEMA`

### Phase 3: Attach schemas - DONE
- [x] FileNode::new() uses `Attrs::with_schema(&FILE_SCHEMA)`
- [x] CompNode::new() uses `Attrs::with_schema(&COMP_SCHEMA)`
- [x] Layer::new() uses `Attrs::with_schema(&LAYER_SCHEMA)`
- [x] Project::new() uses `Attrs::with_schema(&PROJECT_SCHEMA)`
- [x] Player::new() uses `Attrs::with_schema(&PLAYER_SCHEMA)`
- [x] Add `Project::attach_schemas()` for post-deserialization
- [x] Call `attach_schemas()` in main.rs (load_project, playlist load, egui restore)
- [x] Call `attach_schemas()` in shell.rs, bin/project.rs

### Phase 4: Cleanup - DONE
- [x] Replace `set_silent()` call in CompNode::set_frame() with `set()`
- [x] `set_silent()` method kept but marked DEPRECATED
- [x] Updated documentation in attrs.rs and comp_node.rs

### Phase 5: Attribute Editor integration
- [ ] Use `DISPLAY` flag to filter attrs in UI
- [ ] Use `READONLY` flag to disable editing
- [ ] Use schema defaults for "reset to default" feature

### Phase 6: Future - Animation
- [ ] Use `KEYABLE` flag to determine animatable attrs
- [ ] Add keyframe storage

---

## Questions Resolved

1. **Schema attachment after deserialization:** 
   → Call `project.attach_schemas()` after `Project::from_json()`

2. **Unknown attributes (forward compat):**
   → Preserve in values HashMap, schema just won't have definition for them

3. **Player attributes:**
   → Separate `PLAYER_SCHEMA`, not DAG

4. **Layer vs Comp:**
   → Separate schemas, Layer has `source_uuid` but no `frame`

---

## Files to Modify

| File | Changes |
|------|---------|
| `src/entities/attrs.rs` | Add AttrFlags, AttrType, AttrDef, AttrSchema; update Attrs |
| `src/entities/attr_schemas.rs` | NEW - define all schemas |
| `src/entities/mod.rs` | Export new module |
| `src/entities/file_node.rs` | Use FILE_SCHEMA |
| `src/entities/comp_node.rs` | Use COMP_SCHEMA, LAYER_SCHEMA |
| `src/entities/project.rs` | Use PROJECT_SCHEMA, add attach_schemas() |
| `src/core/player.rs` | Use PLAYER_SCHEMA |
| `src/main_events.rs` | Remove manual mark_dirty() where schema handles it |
| `src/widgets/attr_editor/` | Use schema for display filtering |
