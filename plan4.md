# Plan 4: 3D Perspective Projection Implementation

Date: 2025-12-18

This plan details the implementation of true 3D perspective projection (AE-style) based on review of megaplan.md and current codebase state.

---

## 1. Current State Analysis

### 1.1 What's Ready

**CameraNode (`src/entities/camera_node.rs`):**
- `view_matrix()` - world -> camera space (supports POI or Euler rotation)
- `projection_matrix(aspect)` - perspective projection with FOV
- `view_projection_matrix(aspect)` - combined VP matrix
- AE-compatible defaults: FOV 39.6, position [0, 0, -1000], POI [0, 0, 0]
- DOF parameters defined (future use)

**Layer Attributes:**
- `position`, `rotation`, `scale`, `pivot` stored as Vec3 (XYZ data exists)
- Schema supports full 3D values

**EventBus (`src/core/event_bus.rs`):**
- Custom pub/sub on `std::sync` (RwLock/Mutex/Arc)
- NOT crossbeam-based
- crossbeam::deque used only in workers.rs for work-stealing

### 1.2 What's NOT Ready

**compose_internal (`src/entities/comp_node.rs:831`):**
```rust
// Current: uses only rot[2] (Z rotation) - 2D only
let rot_rad = rot[2].to_radians();

// Current: 2D affine transform
frame = transform::transform_frame(&frame, canvas, pos, rot_rad, scl, pvt);

// Current: mat3 inverse matrix (2D)
transform::build_inverse_matrix_3x3(pos, rot_rad, scl, pvt, src_center)
```

**Problems:**
1. Camera not referenced at all in compose_internal
2. Only Z rotation used (rot[0], rot[1] ignored)
3. Transform is 2D affine (mat3), not projective (mat4)
4. CPU transform can't do perspective-correct sampling
5. No Z-ordering/depth handling

---

## 2. Architecture for 3D Perspective

### 2.1 Camera Selection

**Option A: Explicit active_camera field**
```rust
// In CompNode
pub active_camera: Option<Uuid>,
```
- Pro: Clear, explicit
- Con: Extra UI to manage

**Option B: First CameraNode in layers (AE-style)**
- Cameras are layers, topmost camera wins
- Pro: Familiar to AE users
- Con: Need to filter camera layers from render

**Recommendation:** Option B (AE-style) - cameras as layers, topmost active.

### 2.2 Transform Pipeline

```
For each layer:
  1. Build Model matrix (mat4):
     - Translate by position
     - Rotate by rotation (XYZ Euler)
     - Scale by scale
     - Pivot offset
  
  2. Get View matrix from active camera
  
  3. Get Projection matrix from camera (with comp aspect)
  
  4. MVP = Projection * View * Model
  
  5. Transform layer corners through MVP -> screen coords
  
  6. Render with perspective-correct texture sampling
```

### 2.3 GPU vs CPU

**GPU Path (Required for perspective):**
- Perspective-correct texture sampling needs shader
- Pass MVP matrix to fragment shader
- Sample source texture with projective coordinates

**CPU Path (Fallback, limited):**
- Can approximate with corner-pin transform
- No true perspective-correct interpolation
- Acceptable for preview, not production

**Recommendation:** GPU compositor for 3D comps, CPU for 2D-only comps.

### 2.4 Z-Ordering

**Option A: Painter's algorithm**
- Sort layers by Z (centroid or closest point)
- Render back-to-front
- Pro: Simple, works with transparency
- Con: Intersecting layers fail

**Option B: Depth buffer**
- Per-pixel depth test
- Pro: Correct for all cases
- Con: Transparency harder (need OIT or sorting anyway)

**Recommendation:** Painter's algorithm for v1, depth buffer later if needed.

---

## 3. Implementation Plan

### Phase 1: Camera Integration (Foundation)

**1.1 Add camera reference to CompNode**
```rust
// src/entities/comp_node.rs
impl CompNode {
    /// Find active camera (topmost CameraNode in layers)
    pub fn active_camera(&self, media: &HashMap<Uuid, Arc<NodeKind>>) -> Option<&CameraNode> {
        for layer in self.layers.iter() {
            if let Some(NodeKind::Camera(cam)) = media.get(&layer.source_uuid()).map(|n| n.as_ref()) {
                return Some(cam);
            }
        }
        None
    }
}
```

**1.2 Add aspect ratio helper**
```rust
impl CompNode {
    pub fn aspect(&self) -> f32 {
        let (w, h) = self.dim();
        w as f32 / h as f32
    }
}
```

### Phase 2: 3D Transform Math

**2.1 Add mat4 transform builder (`src/entities/transform.rs`)**
```rust
use glam::{Mat4, Vec3};

/// Build model matrix for 3D layer transform
pub fn build_model_matrix(
    position: [f32; 3],
    rotation: [f32; 3],  // degrees, XYZ Euler
    scale: [f32; 3],
    pivot: [f32; 3],
) -> Mat4 {
    let pos = Vec3::from(position);
    let scl = Vec3::from(scale);
    let pvt = Vec3::from(pivot);
    
    // Convert degrees to radians
    let rot_x = rotation[0].to_radians();
    let rot_y = rotation[1].to_radians();
    let rot_z = rotation[2].to_radians();
    
    // Build matrix: T * R * S * pivot_offset
    // Order matters! This matches AE convention.
    let pivot_offset = Mat4::from_translation(-pvt);
    let scale_mat = Mat4::from_scale(scl);
    let rotation_mat = Mat4::from_euler(glam::EulerRot::ZYX, rot_z, rot_y, rot_x);
    let translation = Mat4::from_translation(pos);
    
    translation * rotation_mat * scale_mat * pivot_offset
}

/// Build MVP matrix for layer
pub fn build_mvp(
    model: Mat4,
    view: Mat4,
    projection: Mat4,
) -> Mat4 {
    projection * view * model
}
```

**2.2 Add inverse for texture sampling**
```rust
/// Build inverse MVP for texture coordinate lookup
pub fn build_inverse_mvp(mvp: Mat4) -> Option<Mat4> {
    mvp.inverse()  // Returns Mat4, check for degenerate
}
```

### Phase 3: GPU Compositor Enhancement

**3.1 Update GPU shader (`src/entities/gpu_compositor.rs`)**

Current shader has `u_top_transform` as mat3. Need mat4:
```glsl
uniform mat4 u_mvp;        // Model-View-Projection
uniform mat4 u_mvp_inv;    // Inverse for texture lookup

// In fragment shader:
void main() {
    // Transform screen coord back to texture coord
    vec4 screen_pos = vec4(v_uv * 2.0 - 1.0, 0.0, 1.0);
    vec4 tex_coord = u_mvp_inv * screen_pos;
    tex_coord /= tex_coord.w;  // Perspective divide
    
    // Sample with perspective-correct coords
    vec2 uv = tex_coord.xy * 0.5 + 0.5;
    
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {
        discard;
    }
    
    vec4 color = texture(u_top_tex, uv);
    // ... blending
}
```

**3.2 Update blend API**
```rust
// Change from [f32; 9] (mat3) to [f32; 16] (mat4)
pub fn blend(&self, frames: Vec<(Frame, f32, BlendMode, [f32; 16])>) -> Option<Frame>
```

### Phase 4: compose_internal Update

**4.1 Detect 3D mode**
```rust
fn is_3d_comp(&self, media: &HashMap<Uuid, Arc<NodeKind>>) -> bool {
    // Has camera OR any layer has non-zero X/Y rotation
    self.active_camera(media).is_some() || 
    self.layers.iter().any(|l| {
        let rot = l.attrs.get_vec3(A_ROTATION).unwrap_or([0.0, 0.0, 0.0]);
        rot[0].abs() > 0.001 || rot[1].abs() > 0.001
    })
}
```

**4.2 3D compose path**
```rust
fn compose_internal(&self, frame_idx: i32, ctx: &ComputeContext) -> Option<Frame> {
    // ... existing setup ...
    
    let is_3d = self.is_3d_comp(&ctx.media);
    
    if is_3d {
        self.compose_3d(frame_idx, ctx)
    } else {
        self.compose_2d(frame_idx, ctx)  // existing code
    }
}

fn compose_3d(&self, frame_idx: i32, ctx: &ComputeContext) -> Option<Frame> {
    let camera = self.active_camera(&ctx.media);
    let aspect = self.aspect();
    
    let (view, projection) = if let Some(cam) = camera {
        (cam.view_matrix(), cam.projection_matrix(aspect))
    } else {
        // Default camera at Z=-1000 looking at origin
        let default_view = Mat4::look_at_rh(
            Vec3::new(0.0, 0.0, -1000.0),
            Vec3::ZERO,
            Vec3::Y,
        );
        let default_proj = Mat4::perspective_rh_gl(
            39.6_f32.to_radians(),
            aspect,
            1.0,
            10000.0,
        );
        (default_view, default_proj)
    };
    
    // Collect layers with 3D transforms
    let mut render_layers: Vec<(Frame, f32, BlendMode, Mat4)> = Vec::new();
    
    for layer in self.layers.iter().rev() {
        // ... visibility/timing checks ...
        
        // Skip camera layers from rendering
        if let Some(NodeKind::Camera(_)) = ctx.media.get(&layer.source_uuid()).map(|n| n.as_ref()) {
            continue;
        }
        
        let pos = layer.attrs.get_vec3(A_POSITION).unwrap_or([0.0, 0.0, 0.0]);
        let rot = layer.attrs.get_vec3(A_ROTATION).unwrap_or([0.0, 0.0, 0.0]);
        let scl = layer.attrs.get_vec3(A_SCALE).unwrap_or([1.0, 1.0, 1.0]);
        let pvt = layer.attrs.get_vec3(A_PIVOT).unwrap_or([0.0, 0.0, 0.0]);
        
        let model = transform::build_model_matrix(pos, rot, scl, pvt);
        let mvp = projection * view * model;
        
        // ... get frame, opacity, blend_mode ...
        
        render_layers.push((frame, opacity, blend_mode, mvp));
    }
    
    // Sort by Z (painter's algorithm)
    render_layers.sort_by(|a, b| {
        // Compare Z of layer origin after transform
        let z_a = (a.3 * Vec4::new(0.0, 0.0, 0.0, 1.0)).z;
        let z_b = (b.3 * Vec4::new(0.0, 0.0, 0.0, 1.0)).z;
        z_b.partial_cmp(&z_a).unwrap_or(std::cmp::Ordering::Equal)
    });
    
    // Render with GPU compositor (required for perspective)
    // ... GPU blend call with mat4 transforms ...
}
```

### Phase 5: UI Updates

**5.1 Attribute Editor**
- Already shows XYZ for position/rotation/scale
- Verify rotation X/Y are editable (not greyed out)

**5.2 Viewport Gizmo**
- Current: 2D only (Translate X/Y, Rotate Z, Scale X/Y)
- Future: 3D gizmo with axis handles
- For v1: Keep 2D gizmo, use AE for 3D editing via number inputs

**5.3 Camera creation**
- Add "New > Camera" to comp context menu
- Camera appears as layer in timeline

---

## 4. Execution Order

| Step | Task | Depends On | Effort |
|------|------|------------|--------|
| 1 | `build_model_matrix()` in transform.rs | - | Small |
| 2 | `active_camera()` helper in CompNode | - | Small |
| 3 | `is_3d_comp()` detection | 2 | Small |
| 4 | Update GPU shader for mat4 | - | Medium |
| 5 | Update blend API for mat4 | 4 | Medium |
| 6 | `compose_3d()` implementation | 1,2,3,5 | Large |
| 7 | Z-sorting (painter's algorithm) | 6 | Small |
| 8 | Camera layer creation UI | 2 | Small |
| 9 | Verify AE XYZ rotation inputs work | 6 | Small |

---

## 5. Testing Strategy

**Unit tests:**
- `build_model_matrix()` matches expected output
- Camera matrices are valid (not NaN/Inf)
- MVP inversion works for non-degenerate cases

**Visual tests:**
- Layer with X rotation tilts correctly
- Layer with Y rotation tilts correctly  
- Camera move changes perspective
- Multiple layers sort correctly by Z
- Transparency works with sorted layers

**Regression:**
- Existing 2D comps render identically
- Performance doesn't degrade for 2D comps

---

## 6. Open Decisions

1. **Euler order:** ZYX (AE default) or configurable? MAke a comparison and ask question
2. **Default camera:** Lookup for camera when 3D used, fallback to default app camera settings.
3. **2D/3D toggle:** Per-comp setting, or auto-detect?
4. **Gizmo:** 3D gizmo in v1, we already have one

---

## 7. Relation to megaplan.md

This plan expands **Workstream E (3D Transforms & Camera)** with concrete implementation details.

**megaplan.md open question answered:**
> "do we need real camera/perspective rendering now, or is it 'data plumbing first'?"

**Answer:** Real perspective rendering now. Data is already plumbed (Vec3 attrs exist). Implementation needed.

---

## Appendix: crossbeam vs std::sync Clarification

**Current usage:**
- `crossbeam::deque` - workers.rs work-stealing (USED)
- `crossbeam-channel` - NOT used anywhere
- `EventBus` - custom pub/sub on std::sync::RwLock/Mutex

**For REST API:**
- Need channel for thread communication (not pub/sub)
- Either `crossbeam-channel` or `std::sync::mpsc` works
- Recommendation: use `std::sync::mpsc` unless specific crossbeam features needed

**"Local only" for REST:**
- Means `127.0.0.1` binding
- No LAN access unless explicitly enabled
