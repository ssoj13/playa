# CameraNode Status

## Summary

**CameraNode fully implemented and integrated.** Backend is ready, UI exposure may be missing.

---

## Implementation

### `src/entities/camera_node.rs`

Full 3D camera with:

- **Standard layer transform**: position, rotation, scale, pivot
- **Projection types**: perspective, orthographic
- **POI mode**: look-at target (like After Effects)
- **Rotation mode**: Euler angles (alternative to POI)
- **Lens**: fov (39.6 default like AE), near/far clip
- **DOF placeholders**: dof_enabled, focus_distance, aperture (future)

Key methods:
```rust
view_matrix()                        // world -> camera space
projection_matrix(aspect, height)    // camera -> clip space  
view_projection_matrix(aspect, h)    // combined
```

### Integration Points

1. **`NodeKind` enum** (`node_kind.rs`):
   ```rust
   pub enum NodeKind {
       Image(ImageNode),
       Solid(SolidNode),
       Text(TextNode),
       Camera(CameraNode),  // <-- registered
       ...
   }
   ```

2. **`comp_node.rs::active_camera()`** (line 512):
   ```rust
   pub fn active_camera(...) -> Option<&CameraNode>
   ```
   Finds topmost visible Camera layer on current frame.

3. **`compose_internal()`** (line 983):
   ```rust
   let camera = self.active_camera(frame_idx, ctx.media);
   let view_projection: Option<Mat4> = camera.map(|cam| {
       let dim = self.dim();
       let aspect = dim.0 as f32 / dim.1 as f32;
       cam.view_projection_matrix(aspect, dim.1 as f32)
   });
   ```
   Camera's VP matrix passed to `transform_frame_with_camera()`.

---

## What Works

- CameraNode creation with all attributes
- View/projection matrix generation
- Automatic lookup of topmost camera layer
- Integration into compositor pipeline
- Perspective and orthographic modes
- POI (look-at) and rotation modes

---

## Possibly Missing

1. **UI to add Camera layer** - need to check if "Add Camera" exists in layer menu
2. **Inspector panel for camera attrs** - fov, near/far, projection type
3. **3D layer toggle** - layers need "3D" flag to be affected by camera
4. **Viewport camera preview** - showing camera frustum in viewport

---

## 3D Pipeline Flow

```
Layer position/rotation/scale
        |
        v
    Model matrix (layer transform)
        |
        v
    Camera view matrix (world -> camera)
        |
        v
    Camera projection matrix (camera -> clip)
        |
        v
    Final pixel position
```

Without camera: layers use 2D orthographic view (current default).
With camera: layers in 3D space, perspective/ortho based on camera settings.

---

## Next Steps (if needed)

1. Add "New Camera" to layer creation menu
2. Build camera inspector panel
3. Add "3D Layer" toggle to layer attrs
4. (Optional) Viewport frustum preview
