# GPU compositor unification — research + plan

Date: 2026-05-10. Goal stated by user: **GPU-first, unified CPU/GPU
data shape, faster, no logic loss.**

## Status (2026-05-11)

| Phase | Done | Commit |
|---|---|---|
| Infrastructure | ✅ canvas-to-src matrix, image_to_frame_affine helper, GPU double-transform fix | `a4863d0` |
| A — LayerPayload type | ✅ unified data shape across CompositorType / GpuBlendBridge / both backends | `3a1d82b` |
| B-2D — GPU skip pre-render (2D-flat) | ✅ raw frame + canvas-to-src matrix on GPU path for layers without camera or X/Y rot | `97a1e3c` |
| B-camera — GPU shader camera VP | ✅ per-pixel ray-plane unproject in layer_blend.wgsl, comp_node populates CameraPathInfo | `123c6c4` |
| C — CPU compositor matrix-aware | ⏳ next session | — |
| D — GPU depth + OIT | ⏳ pending | — |
| E — Effects framework + ports | ⏳ pending | — |

Shipped tooling (separate from compositor work): `playa-coord`
crate (`df1bf38`), screen_ndc Mat4 + flip_y (`ba1f09d`).

**Result of Phases A + B**: GPU compositor backend now skips CPU
pre-render for **all non-tilted layers** (covers ≥95% of typical
layer transforms — only X/Y rotated layers still pre-render). The
~33 GB/s memory bandwidth burn from the canvas-sized resample on
heavy comps is gone on the GPU path.

**Next step (Phase C)**: rewrite `CpuCompositor::blend_with_dim` as
a matrix-aware single-pass resample-blend. For each output canvas
pixel: iterate layers bottom-to-top, apply each layer's `inv_matrix`
(or `camera_path` ray-plane) → src pixel, bilinear sample, blend with
mode + opacity. Removes CPU pre-render from the CPU backend too,
unifying both backends on one data shape. Then drop `gpu_inline`
gate in comp_node — both backends consume raw frames + matrices for
non-tilted layers. Estimated 2-3 days; recommend starting in fresh
session for focus + rayon-parallelization decisions.

## 1. Current state of the world

### 1.1 Per-layer pipeline (today, in `compose_internal`)

For each layer in render-order:

```
source_node.compute(src_frame_idx)           // raw Frame (Loaded/Loading)
    ↓
effects::apply_all(frame, &layer.effects)    // CPU pixel ops (blur/brightness/hsv)
    ↓
transform pre-render decision:
    if GPU active && 2D-flat: skip pre-render, build canvas-to-src 3×3 matrix
    else:                     transform_frame_with_camera (CPU resample to canvas)
    ↓
track_matte::apply_track_matte(frame, mask, channel)   // CPU multiply alpha
    ↓
push (frame, opacity, blend_mode, matrix) to source_frames
```

Then `compositor.blend_with_dim(source_frames, canvas_dim)` does the
final stack.

### 1.2 What each compositor backend does

| Stage | CpuCompositor | WgpuCompositor |
|---|---|---|
| Input frame size | canvas (pre-rendered) | canvas (pre-rendered today; raw after Phase 1) |
| Matrix usage | **ignored** (frame already transformed) | uniform `m * vec3(canvas_pixel, 1)` → src texture sample |
| Camera VP | **CPU pre-renders via `transform_frame_with_camera`** (ray-plane unproject) | not aware |
| Track matte | applied **before** compositor (CPU pixel op) | applied before compositor |
| Effects | applied **before** transform (CPU) | same |
| Blend modes | match formulas in CPU+WGSL | same 8 modes |
| Z order / depth | painter's algorithm (sorted) | painter's algorithm |
| Transparency overlap | back-to-front blend | back-to-front blend (1 GPU pass per layer) |
| Format | F32/F16/U8 (3 codepaths) | upload as Rgba32F/16F/8Unorm |

### 1.3 Asymmetry inventory

What forces "different code path" between backends:

1. **Frame size at compositor input** — CPU expects pre-rendered (=
   canvas-sized). GPU shader can sample any-sized src via matrix.
2. **Matrix semantics** — CPU compositor literally `_transform` ignored
   (line 293 of compositor.rs). Documented as "GPU only".
3. **Camera projection** — only CPU pre-render path handles 3D / camera.
   GPU shader has no `camera_vp` uniform.
4. **Tilted layer (X/Y rot)** — CPU pre-render ray-plane intersection.
   Not representable in 3×3 matrix; needs 4×4 + per-pixel ray work.

### 1.4 Memory bandwidth audit (the silent killer)

For N layers @ canvas WxH F32:

| Step | Bytes per frame |
|---|---|
| CPU pre-render (read src, write canvas-sized) | `N × W × H × 16 × 2` |
| GPU upload (canvas-sized texture per layer) | `N × W × H × 16` |
| GPU blend pass output (one per layer) | `(N-1) × W × H × 16` |
| GPU readback to CPU | `W × H × 16` |
| Final upload to viewport texture | `W × H × 16` |

**8-layer 4K F32 @ 30fps**: `8 × 4096 × 2160 × 16 = ~1.1 GB/frame` for
pre-render alone, **33 GB/s sustained**. That's most of a desktop
DDR5 bandwidth budget burned on a redundant resample.

Skipping the pre-render cuts this by `N ×`: **biggest single perf
win available.**

## 2. Unification proposal

### 2.1 Unified data shape

Replace the awkward tuple `(Frame, f32, BlendMode, [f32; 9])` with
a typed payload:

```rust
pub struct LayerPayload {
    pub frame: Frame,                   // RAW source (not pre-rendered)
    pub opacity: f32,
    pub blend_mode: BlendMode,
    pub inv_matrix: [f32; 9],          // canvas-to-src 2D affine
    pub camera_vp_inv: Option<[f32; 16]>, // 3D camera inverse VP, None = 2D ortho
    pub z_position: f32,                // for future depth sort / OIT
    pub mask: Option<MaskRef>,          // track matte (texture + channel)
    pub layer_is_tilted: bool,         // X/Y rotation present (ray-plane needed)
}
```

Both backends consume `Vec<LayerPayload>` + canvas dim. **Same data
shape, same semantics — diverge only in implementation mechanics**
(CPU resamples via `sample_bilinear`, GPU rasterizes via shader).

### 2.2 CPU compositor rewrite (matrix-aware resample-blend)

Currently CPU does:
```
pre-render layer N (read src, write canvas)  // N * canvas writes
↓
blend layer N onto accumulator (read canvas, write canvas)  // N * canvas writes
```
= **2N memory passes**.

New CPU compositor: resample-while-blending in single pass.
```
for each canvas pixel:
    for each layer (bottom-to-top):
        canvas_pixel → inv_matrix → maybe camera_vp_inv → src_pixel
        sample src bilinear
        apply mask (multiply alpha)
        blend with accumulator using mode + opacity
write accumulator pixel
```
= **N memory passes** (one per layer, sequential) + canvas-size accumulator
write. **2× faster** in the typical case, more for many layers.

Cost: per-pixel matrix ops + sample. For F32 4K this is `4096 × 2160 ×
N × ~10 FLOPs = ~88M FLOPs per layer per frame`. Modern CPU at 50
GFLOPs (parallel rayon) handles this in ~2ms. Pre-render today is
~10-15ms per layer for the same case (memory-bound, not compute-bound).

### 2.3 GPU compositor full pipeline

Update `layer_blend.wgsl`:

```wgsl
struct Uniforms {
    opacity: f32,
    blend_mode: i32,
    canvas_size: vec2<f32>,
    top_size: vec2<f32>,
    mask_channel: i32,           // -1 = no mask, 0 = R, 3 = A, etc.
    pad0: vec3<f32>,
    canvas_to_src_2d: mat3x3<f32>,    // existing
    camera_vp_inv: mat4x4<f32>,       // NEW: 3D camera unproject (identity = 2D)
}

@group(0) @binding(N+1) var t_mask: texture_2d<f32>;  // optional mask texture

fn fs_blend(inp: VsOut) -> @location(0) vec4<f32> {
    let canvas_pixel = inp.uv * u.canvas_size;
    
    // 1. Apply 2D affine
    var src_pixel = (u.canvas_to_src_2d * vec3(canvas_pixel, 1.0)).xy;
    
    // 2. If camera active: unproject through inverse VP
    //    (For tilted layers, would need ray-plane; tilted is Phase D.)
    
    // 3. Sample top texture
    let top_uv = src_pixel / u.top_size;
    if (top_uv.x < 0.0 || top_uv.x > 1.0 || top_uv.y < 0.0 || top_uv.y > 1.0) {
        return textureSample(t_bottom, s, inp.uv);  // out of bounds → bottom only
    }
    var top = textureSample(t_top, s, top_uv);
    
    // 4. Apply track matte (if present)
    if (u.mask_channel >= 0) {
        let mask_v = textureSample(t_mask, s, top_uv)[u.mask_channel];
        top.a *= mask_v;
    }
    
    // 5. Blend with bottom + opacity + mode
    let bottom = textureSample(t_bottom, s, inp.uv);
    let blended = blend_rgb(bottom.rgb, top.rgb, u.blend_mode);
    let top_alpha = top.a * u.opacity;
    return vec4(bottom.rgb * (1.0 - top_alpha) + blended * top_alpha,
                bottom.a * (1.0 - top_alpha) + top_alpha);
}
```

### 2.4 Phase D — depth + OIT (GPU-only)

For GPU **only** — CPU keeps painter's algo (reasonable cost-benefit
trade — depth buffers / OIT logic on CPU is a 5-10× perf hit and
asymmetric advanced GPU features are OK).

**Depth buffer** (opaque layers):
- Add depth attachment to render pass (Depth32Float)
- Per-vertex `clip_position.z = layer_z_normalized`
- Enable depth test in pipeline state

**Weighted-blended OIT** (transparent layers):
- Render to two color attachments: `accum (Rgba16F)`, `revealage (R8Unorm)`
- Composite pass: `final = accum / max(revealage, 1e-5) * (1 - revealage_a) + bg * revealage_a`
- Approximate but order-independent — single pass for N layers

Together: **1-2 GPU passes total** instead of N. For 16-layer comp
that's 8-16× speedup at the compositor stage.

### 2.5 Effects — unified GPU framework + port the 3 we have

Effects today: `brightness`, `hsv`, `blur`. We port all three to GPU
**under a unified simple framework** that any future effect can
plug into without scaffolding boilerplate.

"Simple" = the minimum that makes adding a new effect a one-file
change. Not over-engineered: no compute shaders, no GPU resource
graph, no automatic dependency analysis. Just enough shared shape
that the 3 ports look like data, not three separate render
pipelines.

#### Framework — `GpuEffect` trait + chain executor

```rust
/// One effect = one or more render passes against a layer texture.
pub trait GpuEffect: Send + Sync {
    fn name(&self) -> &'static str;

    /// Ordered render passes. 1 for simple, N for multi-pass (blur).
    /// Each pass reads either the original layer src or the previous
    /// pass output, writes into a ping-pong texture from the chain pool.
    fn passes(&self) -> Vec<EffectPass>;
}

pub struct EffectPass {
    pub wgsl: &'static str,        // shader source
    pub entry: &'static str,       // fragment entry
    pub uniforms: Vec<u8>,         // serialized POD struct matching shader
    pub input: EffectInput,        // where to read from
}

pub enum EffectInput {
    LayerSrc,                       // original layer texture (start of chain)
    PreviousPass,                   // output of pass i-1 (ping-pong)
}
```

#### Chain executor (lives in `playa-engine/src/render_gpu/effects/mod.rs`)

```rust
pub struct EffectChain { /* device, queue, pool, pipeline cache */ }

impl EffectChain {
    /// Run a chain of effects against a source texture, return final.
    pub fn execute(
        &mut self,
        src: &wgpu::Texture,
        effects: &[Box<dyn GpuEffect>],
    ) -> wgpu::Texture {
        let mut current = src.clone();
        for effect in effects {
            for pass in effect.passes() {
                let next = self.pool.acquire(current.size(), current.format());
                self.dispatch(&pass, &current, &next);
                current = next;
            }
        }
        current
    }
}
```

What the framework gives you on day one:
- One Rust module + one WGSL file per effect
- Pipeline cache (don't rebuild render pipeline per frame)
- Ping-pong texture pool (reuse by `(size, format)` key)
- Uniform buffer reused per pass
- Multi-pass support free (blur returns 2 passes; framework
  ping-pongs without effect knowing)

#### The 3 ports

| Effect | WGSL | Rust |
|---|---|---|
| `Brightness` | 1 file, `out.rgb = src.rgb * scale` | `impl GpuEffect`, single pass |
| `Hsv` | 1 file, RGB→HSV→adjust→RGB color matrix | `impl GpuEffect`, single pass |
| `Blur` | 1 file, Gaussian kernel | `impl GpuEffect`, 2 passes (H, V) returning `EffectInput::LayerSrc` then `EffectInput::PreviousPass` |

Wiring: when `CompositorType::Wgpu` active, comp_node builds `Vec<Box<dyn GpuEffect>>` from `layer.effects` and the chain runs before
the blend pass — CPU effects skipped entirely. When
`CompositorType::Cpu` active, current CPU effect path stays.

## 3. Migration plan

### Phase A — Layer payload type (~1 day)
- Define `LayerPayload` struct, public in `entities::compositor`
- Change `compose_internal` to push `LayerPayload`
- Update `CompositorType::blend_with_dim` signature
- Update `gpu_blend_bridge::GpuBlendRequest` to carry `LayerPayload`
- CPU compositor: still ignores matrix (fallback to old behavior),
  but consumes the new shape
- GPU compositor: still uses matrix only (no camera VP yet)
- Verify: all existing tests pass, behavior unchanged

### Phase B — GPU shader: camera VP + track matte (~2 days)
- Add `camera_vp_inv: mat4x4` uniform
- Add optional mask texture binding
- Shader applies inverse VP for camera (when not identity)
- Shader applies mask channel multiplier
- comp_node: skip CPU pre-render for ALL 2D-affine + camera-projected
  flat layers (was: only 2D-flat). Tilted (X/Y rot) still pre-renders.
- comp_node: pass mask texture to compositor instead of pre-multiplying
- Verify: identity case, Z-rotation, scale, translate, camera-perspective,
  camera-ortho — all match CPU pre-render output (within bilinear
  sample tolerance)

### Phase C — CPU compositor unified (~3 days)
- Rewrite `CpuCompositor::blend_with_dim` as resample-while-blend
- Apply matrix per output pixel
- Apply camera unproject per output pixel (when not identity)
- Apply mask sample per output pixel
- Apply blend
- Single output pass, no pre-render needed
- comp_node: stop CPU pre-rendering ENTIRELY (both backends consume raw)
- KEEP `transform_frame_with_camera` for tilted-layer ray-plane edge case
  (small minority of layers)
- Verify: golden frame regression (pixel-diff with previous CPU output)

### Phase D — GPU depth + OIT (~3-4 days)
- Add depth attachment to compositor pipeline
- Per-vertex Z from layer pos[2]
- Enable depth test for opaque layers (alpha = 1.0)
- For transparent layers: route to OIT (weighted blended)
  - Two color attachments
  - Composite pass at end
- comp_node: stop sorting layers by Z (depth test handles it)
- Verify: 3D layers crossing Z, alpha-blended overlapping layers

### Phase E — GPU effects framework + port all 3 (~4-5 days total)
- `GpuEffect` trait + `EffectChain` executor + texture pool +
  pipeline cache (~1 day)
- `Brightness` impl + WGSL — 1-pass (~half day)
- `Hsv` impl + WGSL — 1-pass color matrix (~half day)
- `Blur` impl + WGSL — 2-pass separable (~1 day)
- Wire into compose_internal: when GPU active, build
  `Vec<Box<dyn GpuEffect>>` from `layer.effects` and run chain
  before blend pass (~half day)
- Verify each effect: golden-frame compare against CPU output
  (1-LSB bilinear / float-precision tolerance)
- CPU effect impls kept as fallback for `CompositorType::Cpu`

## 4. Speed wins (estimated)

| Phase | Change | Speed delta |
|---|---|---|
| A | Type rename, no behavior change | 0% |
| B | Skip CPU pre-render for camera/2D layers (was: 2D only) | **30-50%** engine time on heavy comps |
| C | CPU compositor single-pass | **2-3×** CPU compositor speed |
| D | GPU 1-2 passes vs N | **3-10×** GPU compositor for N≥8 layers |
| E | Port 3 existing effects to GPU | brightness/hsv ~negligible; blur 5-10× |

Combined Phase A-E: **realistically 2-5× total engine throughput**
on typical multi-layer 4K comps. Most of the win comes from B+D;
E is a code-architecture improvement (effects move off CPU
hot path) more than a raw perf win for 2 of the 3 effects.

## 5. Decision points (need user input)

1. **Keep CpuCompositor at all?** Linux without GPU, headless render, CI
   tests need it. If yes → must rewrite (Phase C). If "GPU only,
   require wgpu" → can delete CPU compositor entirely. **Recommendation:
   keep, but rewrite to unified data shape.**

2. **CPU-GPU bit-identical output?** Bilinear sampling: CPU's
   `sample_bilinear` and GPU's `textureSample` may produce 1-LSB
   differences (rounding). For tests this means tolerance-based
   compare, not byte-identical. **Recommendation: tolerance-compare
   (1-LSB), not byte-identical.**

3. **OIT method**: weighted-blended (cheap, approximate but good
   enough) vs depth-peeling (exact, multi-pass) vs per-pixel linked
   lists (compute shader, complex). **Recommendation: weighted-blended
   for Phase D, revisit if quality complaints.**

4. **Color management** (OCIO/OIIO from original TODO): is OCIO LUT
   application part of "first class compositor" or separate phase?
   GPU OCIO is a major add (OCIO Cg shader needs translation to wgsl).
   **Recommendation: separate phase, NOT included in this work.**

5. **Migration order**: ship A→B→C→D in 4 commits, OR fold into 1-2
   bigger commits? **Recommendation: 4 commits, atomic — each phase
   is independently revert-able and verify-able.**

## 6. Risks

- **C is high-touch**: rewriting CPU compositor changes pixel output
  for every render path. Need golden-frame tests + tolerance compare
  before merge. Risk: subtle regression in blend math under specific
  alpha edge cases.
- **D depth + OIT is complex**: weighted-blended needs careful tuning
  of the weight function for HDR. F32 buffers may overflow weight
  accumulator. Mitigation: clamp weights, use F16 accumulator with
  range guard.
- **3D tilted layers**: ray-plane intersection in shader is per-pixel,
  expensive. CPU does it cheaper for the small number of tilted
  layers in typical comps. **Recommendation: keep CPU pre-render for
  tilted layers indefinitely (small edge case, not the hot path).**

## 7. What gets removed in the end state

After Phase A-D landed:

- `compose_internal` no longer calls `transform_frame_with_camera`
  except for tilted-layer pre-render edge case
- `transform_frame_with_camera` simplifies — only used for the tilted
  fallback (could rename `pre_render_tilted_layer`)
- CPU compositor and GPU compositor consume the same `LayerPayload`
- `gpu_blend_bridge` carries `LayerPayload` instead of tuple
- comp_node stops sorting layers by Z (GPU depth test handles it)
- IDENTITY_TRANSFORM constant has fewer special cases (only literal
  identity, used for "no transform needed" flag)

## 8. What this proposal does NOT change

- Source loading (decoders, cache) — unchanged
- Effects pipeline — kept on CPU (deferred to Phase E if/when ported)
- Tilted layer pre-render — kept on CPU (perf trade-off)
- Frame format / pixel buffer types — unchanged
- Cache key derivation — unchanged
- Worker thread scheduling — unchanged
- gpu_blend_bridge mpsc channel mechanics — unchanged (only payload
  shape inside)

---

## TL;DR for decision

- **Phase A** (1 day): types only, no behavior change. Safe.
- **Phase B** (2 days): biggest single perf win (skip CPU pre-render
  for camera-projected layers). Medium risk (camera math in shader).
- **Phase C** (3 days): unifies CPU compositor to matrix-aware. Higher
  risk — touches every render path. Mitigated by golden-frame tests.
- **Phase D** (3-4 days): GPU depth + OIT. GPU-only feature. Independent
  of CPU path.
- **Phase E** (4-5 days): unified `GpuEffect` framework + port
  brightness, HSV, blur. Adding a new effect after this = 1 WGSL +
  ~50 LOC Rust.

Total: ~13-15 days for full GPU-first unified compositor through
Phase E (compositor + transform + camera + depth + OIT + effects).
