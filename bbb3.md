# Layer "renderable" attribute - DONE

## Changes

### 1. attr_schemas.rs
Added `renderable` to LAYER_SPECIFIC:
```rust
AttrDef::new("renderable", AttrType::Bool, DAG_DISP),  // false for camera/light/null/audio
```

### 2. node_kind.rs
Added `is_renderable()` method:
```rust
pub fn is_renderable(&self) -> bool {
    match self {
        NodeKind::Camera(_) => false,
        // Future: Light, Transform (null), Audio -> false
        _ => true,
    }
}
```

### 3. comp_node.rs
- Layer::new() sets `renderable=true` by default
- add_child_layer() now accepts `renderable: bool` parameter
- compose_internal() checks `renderable` instead of `as_camera()`

### 4. main_events.rs
AddLayerEvent handler now:
- Gets `s.is_renderable()` from source node
- Passes it to add_child_layer()

## Behavior

| Source Type | renderable | Renders in Comp |
|-------------|------------|-----------------|
| File        | true       | Yes             |
| Comp        | true       | Yes             |
| Text        | true       | Yes             |
| Camera      | false      | No (control)    |
| Light*      | false      | No (control)    |
| Transform*  | false      | No (control)    |
| Audio*      | false      | No (control)    |

*Future types

## UI
- Shows as checkbox in Attribute Editor
- User can override (e.g., make camera renderable for debug)
