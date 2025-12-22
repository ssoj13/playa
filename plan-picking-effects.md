# Plan: Layer Picking & Effects System

## Feature 1: Layer Picking via Viewport (ID Buffer)

### Concept
When enabled, compositor generates second buffer containing layer IDs alongside color.
Clicking in viewport reads pixel from ID buffer to identify which layer is under cursor.

### Current Architecture Analysis

**Compositor Pipeline:**
```
compose_internal() -> collect source_frames[] -> THREAD_COMPOSITOR.blend() -> Frame
```

**CPU Compositor (compositor.rs):**
- `blend()` - blends frames using per-pixel operations
- Iterates layers, applies opacity + blend mode
- No current support for auxiliary buffers

### Implementation: CPU ID Buffer

**Approach:** Render layer IDs in CPU compositor alongside color.

**Steps:**
1. Add `id_buffer: Option<Vec<u32>>` field to Frame (or separate structure)
2. In `CpuCompositor::blend()`, track which layer "wins" each pixel
3. Store layer index/UUID for each pixel based on alpha contribution
4. On viewport click, read ID at mouse position
5. Map ID back to layer, emit selection event

**Frame Extension:**
```rust
pub struct Frame {
    pub data: Arc<Mutex<FrameData>>,
    pub status: FrameStatus,
    // NEW: optional layer ID buffer (same dimensions as color)
    pub layer_ids: Option<Arc<Vec<u32>>>,  // 0 = background, 1+ = layer indices
}
```

**Blend Modification:**
```rust
fn blend_pixel(..., layer_idx: u32) -> (Rgba, u32) {
    // ... existing blend logic ...

    // Track which layer contributed most (alpha > threshold)
    let winning_layer = if top_alpha > 0.5 { layer_idx } else { base_layer_id };

    (result_color, winning_layer)
}
```

**Picking:**
```rust
// In viewport click handler
fn pick_layer_at(frame: &Frame, x: usize, y: usize) -> Option<usize> {
    frame.layer_ids.as_ref()?.get(y * width + x).copied()
        .filter(|&id| id > 0)
        .map(|id| (id - 1) as usize)  // 0 = bg, 1+ = layer index
}
```

### Toggle
Add to ViewportState or AppSettings:
```rust
pub layer_picking_enabled: bool,  // default false - slight perf cost
```

### Questions:
1. **Click or hover?** Select on click, or highlight on hover?
2. **Visual feedback?** Outline selected layer in viewport?
3. **Performance trade-off?** Always generate ID buffer, or only when mode enabled?

---

## Feature 2: Effects System

### Concept
Each Layer has `Vec<Effect>` - ordered list of effects applied before compositing.
Effects are Attrs wrappers with their own schemas.

### Proposed Architecture

```rust
// src/entities/effect.rs

/// Effect descriptor - wraps Attrs with effect-specific schema
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Effect {
    pub uuid: Uuid,
    pub effect_type: EffectType,
    pub attrs: Attrs,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EffectType {
    GaussianBlur,
    BrightnessContrast,
    AdjustHSV,
}
```

### Effect Schemas

```rust
lazy_static! {
    pub static ref GAUSSIAN_BLUR_SCHEMA: AttrSchema = {
        let mut s = AttrSchema::new();
        s.add("radius", AttrType::Float, false);      // 0.0 - 100.0, default 5.0
        s
    };

    pub static ref BRIGHTNESS_CONTRAST_SCHEMA: AttrSchema = {
        let mut s = AttrSchema::new();
        s.add("brightness", AttrType::Float, false);  // -1.0 to 1.0
        s.add("contrast", AttrType::Float, false);    // -1.0 to 1.0
        s
    };

    pub static ref HSV_ADJUST_SCHEMA: AttrSchema = {
        let mut s = AttrSchema::new();
        s.add("hue_shift", AttrType::Float, false);       // -180 to 180 degrees
        s.add("saturation_mult", AttrType::Float, false); // 0.0 to 2.0
        s.add("value_mult", AttrType::Float, false);      // 0.0 to 2.0
        s
    };
}
```

### Layer Integration

```rust
// In Layer struct (comp_node.rs)
pub struct Layer {
    pub attrs: Attrs,
    pub effects: Vec<Effect>,  // NEW: ordered effect stack
}
```

### Compositor Integration

In `compose_internal()`, after getting source frame but before blending:

```rust
// Get source frame
let mut source_frame = source_node.compute(source_frame_idx, ctx)?;

// Apply layer effects in order (CPU)
for effect in &layer.effects {
    if effect.enabled {
        source_frame = effects::apply(&source_frame, effect)?;
    }
}

// Then blend with other layers
source_frames.push((source_frame, opacity, blend_mode, transform));
```

### Effect Implementations (CPU)

```rust
// src/entities/effects/mod.rs
pub mod blur;
pub mod brightness;
pub mod hsv;

pub fn apply(frame: &Frame, effect: &Effect) -> Option<Frame> {
    match effect.effect_type {
        EffectType::GaussianBlur => blur::apply(frame, &effect.attrs),
        EffectType::BrightnessContrast => brightness::apply(frame, &effect.attrs),
        EffectType::AdjustHSV => hsv::apply(frame, &effect.attrs),
    }
}
```

**Gaussian Blur (separable, O(n*r) per pass):**
```rust
pub fn apply(frame: &Frame, attrs: &Attrs) -> Option<Frame> {
    let radius = attrs.get_float("radius").unwrap_or(5.0) as usize;
    if radius == 0 { return Some(frame.clone()); }

    // Build 1D Gaussian kernel
    let kernel = gaussian_kernel(radius);

    // Separable: horizontal pass -> vertical pass
    let temp = convolve_horizontal(frame, &kernel);
    let result = convolve_vertical(&temp, &kernel);

    Some(result)
}
```

**Brightness/Contrast:**
```rust
pub fn apply(frame: &Frame, attrs: &Attrs) -> Option<Frame> {
    let brightness = attrs.get_float("brightness").unwrap_or(0.0);
    let contrast = attrs.get_float("contrast").unwrap_or(0.0);

    // contrast_factor = (1 + contrast) for range [-1, 1]
    let cf = 1.0 + contrast;

    // For each pixel: out = (in - 0.5) * cf + 0.5 + brightness
    // ... per-pixel operation
}
```

**HSV Adjust:**
```rust
pub fn apply(frame: &Frame, attrs: &Attrs) -> Option<Frame> {
    let hue_shift = attrs.get_float("hue_shift").unwrap_or(0.0);
    let sat_mult = attrs.get_float("saturation_mult").unwrap_or(1.0);
    let val_mult = attrs.get_float("value_mult").unwrap_or(1.0);

    // For each pixel:
    // 1. RGB -> HSV
    // 2. H += hue_shift, S *= sat_mult, V *= val_mult
    // 3. HSV -> RGB
}
```

### UI: Effects in Attribute Editor

Add "Effects" section to existing AE panel for selected layer:

```
Layer: kz_1
├── Visibility: [x]
├── Opacity: [====|====] 1.0
├── Blend Mode: [Normal ▼]
├── Transform
│   └── Position, Rotation, Scale...
└── Effects                            <-- NEW SECTION
    ├── [+ Add Effect ▼]
    │
    ├── ▼ Gaussian Blur               [x] [🗑]
    │   └── Radius: [====|====] 5.0
    │
    └── ▼ Brightness/Contrast         [x] [🗑]
        ├── Brightness: [====|====] 0.0
        └── Contrast: [====|====] 0.0
```

Controls:
- **[+ Add Effect]** - dropdown to add new effect
- **[x]** - enable/disable toggle
- **[🗑]** - remove effect
- **▼** - collapse/expand
- Drag handle for reorder (egui_dnd)

---

## Implementation Order

### Phase 1: Effects Foundation
1. Create `src/entities/effects/mod.rs` module structure
2. Define `Effect` struct, `EffectType` enum
3. Add schemas for 3 effects
4. Add `effects: Vec<Effect>` to Layer struct
5. Update Layer serialization

### Phase 2: Effect Processing
6. Implement `effects::apply()` dispatcher
7. Implement Gaussian Blur (most visible)
8. Integrate into `compose_internal()`
9. Test with hardcoded effect

### Phase 3: Effects UI
10. Add "Effects" section to Attribute Editor
11. "Add Effect" dropdown
12. Per-effect controls based on schema
13. Enable/disable, delete buttons
14. Drag reorder

### Phase 4: More Effects
15. Brightness/Contrast implementation
16. HSV Adjust implementation

### Phase 5: Layer Picking (Later)
17. Add layer_ids buffer to Frame
18. Modify CpuCompositor to track winning layer
19. Viewport click -> pick layer
20. Visual feedback

---

## Questions for Confirmation

1. **Effects first, picking later?** (recommended order above)
2. **UI in existing AE panel?** Or separate "Effects" dock tab?
3. **Blur algorithm:** Box blur (fast) or true Gaussian (quality)?
4. **Effect presets?** Save/load effect configurations? (later?)
5. **Keyframing effects?** Animate params over time? (complex, later?)

---

## Estimated Effort

| Task | Hours |
|------|-------|
| Effects module + schemas | 2h |
| Gaussian blur CPU | 3-4h |
| Compositor integration | 1-2h |
| Effects UI in AE | 4-5h |
| B/C + HSV effects | 2-3h |
| Layer picking (CPU) | 4-6h |
| **Total** | **16-22h** |
