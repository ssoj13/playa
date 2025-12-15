# Layer Transform Implementation Plan

## Current State

Layer attributes already defined in `attr_schemas.rs`:
```rust
AttrDef::new("position", AttrType::Vec3, DAG_DISP_KEY),  // [x, y, z]
AttrDef::new("rotation", AttrType::Vec3, DAG_DISP_KEY),  // [rx, ry, rz] radians
AttrDef::new("scale", AttrType::Vec3, DAG_DISP_KEY),     // [sx, sy, sz]
AttrDef::new("pivot", AttrType::Vec3, DAG_DISP_KEY),     // [px, py, pz] anchor
```

Layer::new() initializes defaults:
```rust
attrs.set(A_POSITION, AttrValue::Vec3([0.0, 0.0, 0.0]));
attrs.set(A_ROTATION, AttrValue::Vec3([0.0, 0.0, 0.0]));
attrs.set(A_SCALE, AttrValue::Vec3([1.0, 1.0, 1.0]));
attrs.set(A_PIVOT, AttrValue::Vec3([0.0, 0.0, 0.0]));
```

**Problem**: Transforms are stored but NOT applied during compositing!

---

## Transform Math

Standard 2D affine transform order (like After Effects):
```
1. Translate to pivot (move anchor to origin)
2. Scale
3. Rotate (Z-axis for 2D)
4. Translate back from pivot
5. Apply position offset
```

Matrix form:
```
M = T(position) * T(pivot) * R(rotation.z) * S(scale) * T(-pivot)
```

For each pixel `(x, y)` in output:
```
[src_x, src_y] = M^(-1) * [x, y]  // inverse transform to sample source
```

---

## Implementation Options

### Option A: CPU Transform (Simple, Slower)

Add `transform_frame()` in `compositor.rs`:

```rust
pub fn transform_frame(
    src: &Frame,
    dst_width: usize,
    dst_height: usize,
    position: [f32; 3],
    rotation: [f32; 3],
    scale: [f32; 3],
    pivot: [f32; 3],
) -> Frame {
    // Build inverse transform matrix
    let inv_mat = build_inverse_transform(position, rotation, scale, pivot);
    
    // Create output frame
    let mut dst = Frame::new(dst_width, dst_height, src.depth());
    
    // For each output pixel, sample source with bilinear interpolation
    for y in 0..dst_height {
        for x in 0..dst_width {
            let [src_x, src_y] = apply_matrix(&inv_mat, x as f32, y as f32);
            let color = sample_bilinear(src, src_x, src_y);
            dst.set_pixel(x, y, color);
        }
    }
    dst
}
```

**Pros**: Simple, no GPU dependency
**Cons**: Slow for large frames, no hardware acceleration

### Option B: GPU Transform (Fast, Complex)

Modify `gpu_compositor.rs` blend shader to accept transform matrix:

```glsl
uniform mat3 u_transform;  // Per-layer transform

void main() {
    // Transform UV coordinates
    vec2 transformed_uv = (u_transform * vec3(v_uv, 1.0)).xy;
    
    // Sample with transformed coords (clamp or transparent outside)
    vec4 layer_color = texture(u_layer, transformed_uv);
    
    // Apply blend mode...
}
```

**Pros**: Fast, GPU-accelerated
**Cons**: More complex, requires shader modifications

### Option C: Hybrid (Recommended)

1. **GPU path** (`GpuCompositor`): Pass transform matrix to shader
2. **CPU fallback** (`CpuCompositor`): Use `image` crate's affine transform

---

## Recommended Implementation

### Phase 1: Layer Transform Getters

Add to `comp_node.rs` Layer impl:

```rust
impl Layer {
    pub fn position(&self) -> [f32; 3] {
        self.attrs.get(A_POSITION)
            .and_then(|v| match v { AttrValue::Vec3(a) => Some(*a), _ => None })
            .unwrap_or([0.0, 0.0, 0.0])
    }
    
    pub fn rotation(&self) -> [f32; 3] { ... }
    pub fn scale(&self) -> [f32; 3] { ... }
    pub fn pivot(&self) -> [f32; 3] { ... }
    
    /// Check if layer has non-identity transform
    pub fn has_transform(&self) -> bool {
        let pos = self.position();
        let rot = self.rotation();
        let scale = self.scale();
        
        pos != [0.0, 0.0, 0.0] ||
        rot != [0.0, 0.0, 0.0] ||
        scale != [1.0, 1.0, 1.0]
    }
    
    /// Build 3x3 affine transform matrix
    pub fn transform_matrix(&self) -> [[f32; 3]; 3] { ... }
}
```

### Phase 2: CPU Transform

New file `entities/transform.rs`:

```rust
/// 3x3 affine matrix (2D transform + translation)
pub type Mat3 = [[f32; 3]; 3];

pub fn identity() -> Mat3 { ... }
pub fn translate(x: f32, y: f32) -> Mat3 { ... }
pub fn rotate(angle: f32) -> Mat3 { ... }
pub fn scale(sx: f32, sy: f32) -> Mat3 { ... }
pub fn multiply(a: &Mat3, b: &Mat3) -> Mat3 { ... }
pub fn invert(m: &Mat3) -> Option<Mat3> { ... }

/// Build layer transform matrix
pub fn build_layer_transform(
    position: [f32; 3],
    rotation: [f32; 3],
    scale: [f32; 3],
    pivot: [f32; 3],
    frame_center: (f32, f32),  // Center of source frame
) -> Mat3 {
    // AE-style: anchor is relative to layer center
    let pivot_x = frame_center.0 + pivot[0];
    let pivot_y = frame_center.1 + pivot[1];
    
    let t_pos = translate(position[0], position[1]);
    let t_pivot = translate(pivot_x, pivot_y);
    let t_pivot_inv = translate(-pivot_x, -pivot_y);
    let r = rotate(rotation[2]);  // Z rotation only for 2D
    let s = scale(scale[0], scale[1]);
    
    // M = T(pos) * T(pivot) * R * S * T(-pivot)
    multiply(&t_pos, &multiply(&t_pivot, &multiply(&r, &multiply(&s, &t_pivot_inv))))
}

/// Sample frame with bilinear interpolation
pub fn sample_bilinear(frame: &Frame, x: f32, y: f32) -> [f32; 4] { ... }

/// Transform frame using CPU
pub fn transform_frame(
    src: &Frame,
    canvas_size: (usize, usize),
    transform: &Mat3,
) -> Frame { ... }
```

### Phase 3: Integrate into Compositor

Modify `compose_internal()` in `comp_node.rs`:

```rust
// After getting source frame:
if let Some(mut frame) = source_node.compute(source_frame, ctx) {
    // Apply layer transform if needed
    if layer.has_transform() {
        let canvas = self.dim();
        let transform = layer.transform_matrix();
        frame = transform::transform_frame(&frame, canvas, &transform);
    }
    
    source_frames.push((frame, opacity, blend));
}
```

### Phase 4: GPU Transform (Optional)

Extend `GpuCompositor::blend_textures()`:
- Add `transform: Mat3` parameter to layer data
- Pass matrix as uniform to blend shader
- Transform UVs in shader before sampling

---

## UI Integration

### Attribute Editor

Transform attrs already exist, just need proper display:
- Position: drag XY (ignore Z for 2D)
- Rotation: angle slider/drag (degrees in UI, radians internally)
- Scale: percentage (100% = 1.0)
- Pivot: drag XY relative to layer center

### Viewport

Future: Add transform gizmo for direct manipulation:
- Move handle at position
- Rotate ring around pivot
- Scale handles at corners

---

## Task Breakdown

- [ ] **Phase 1**: Add Layer transform getters (30 min)
- [ ] **Phase 2**: Create transform.rs with matrix math (2 hr)
- [ ] **Phase 3**: Integrate CPU transform into compose_internal (1 hr)
- [ ] **Phase 4**: Add transform controls to AE widget (1 hr)
- [ ] **Phase 5**: GPU shader transform (optional, 3+ hr)

---

## Notes

### Coordinate System
- Origin: top-left (OpenGL texture coords)
- Position: pixels from canvas origin
- Pivot: offset from layer center (like AE anchor point)
- Rotation: counter-clockwise, radians

### Edge Cases
- Scale = 0: prevent divide-by-zero
- Large rotation: handle wrap-around
- Off-canvas layers: still composite (may be partially visible)

### Performance
- Skip transform if identity (position=0, rotation=0, scale=1)
- Cache transform matrix per frame (don't recompute per pixel)
- Consider SIMD for pixel loop (rayon parallel iterator)
