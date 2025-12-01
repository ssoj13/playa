# PLAYA Architecture v2

## Overview

PLAYA - плеер для просмотра и композитинга image sequences и видео.
Rust + egui + OpenGL. Cross-platform (Windows, macOS, Linux).

---

## Core Entities

```
┌─────────────────────────────────────────────────────────────────┐
│                           PlayaApp                              │
│  (main.rs - eframe::App)                                        │
├─────────────────────────────────────────────────────────────────┤
│  Player          - playback engine (JKL, frame-accurate)        │
│  EventBus        - async event distribution                     │
│  Attrs           - serializable hashmap of variants             │
│  Workers         - work-stealing thread pool                    │
│  GlobalFrameCache- LRU cache с memory tracking                  │
│  Shaders         - GPU shader manager (viewport)                │
│  TimelineState   - zoom/pan/selection state                     │
│  ViewportState   - viewport zoom/pan/scrubber                   │
│  AppSettings     - persistent user preferences                  │
└─────────────────────────────────────────────────────────────────┘
```



### Attrs (entities/attrs.rs)

Generic key-value storage для entity metadata. Thread-safe dirty tracking.

```rust
struct Attrs {
    get/set/del(String, Val): getter/setter triggering dirty flag
    _hash() - hidden functions. Hash returns hash of all nested attributes.
    _data: HashMap<String, AttrValue>,
    dirty: AtomicBool,  // "Attrs changed" flag
}

enum AttrValue {
    Bool(bool),
    Str(String),
    Int(i32),
    UInt(u32),
    Float(f32),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    Mat3([[f32; 3]; 3]),
    Mat4([[f32; 4]; 4]),
    Arc<Atomic*> types (need an advice here)
    Json(String),  // Type for JSON-encoded strings, nested JSON serializer for nested HashMaps, Lists and such. Special case in get/set?
}
```

Attrs **Используется в:**:
  - Frame, Comp, Layer, Project для всех их персистентных атрибутов, т.к. Attrs рекурсивно сериализуется в JSON:
  - PlayaApp содержит по сути большой HashMap в котором в ветках <Project:HashMap>, <Timeline:HashMap> содержатся настройки всех частей приложения, которые PlayaApp сохраняет и восстанавливает при старте.



### Frame (entities/frame.rs)
Единица изображения. Immutable pixel buffer + metadata.

```rust
struct Frame {
    buffer: Arc<RwLock<PixelBuffer>>,  // U8 | F16 | F32
    pixel_format: PixelFormat,          // Rgba8 | RgbaF16 | RgbaF32
    width, height: usize,
    status: Arc<AtomicU8>,              // Placeholder|Header|Loading|Loaded|Error
}
```

**Pixel formats:**
- `Rgba8` - 8-bit (sRGB), для видео и LDR
- `RgbaF16` - 16-bit float, EXR half-float
- `RgbaF32` - 32-bit float, EXR full precision

**Operations:**
- `to_rgb24()` / `to_rgb48()` - для encoding
- `tonemap()` - HDR→LDR (ACES, Reinhard, Clamp, None)
- `crop_copy()` - resize с alignment
- `resize()` - bilinear resample




### Node (entities/node.rs)
Node is basically a struct supporting Node trait:

  - Node.attr.* is a persistent Attrs struct and some methods to work with them
  - Node.data is another instance of Attrs, not-serializable, discardable transient data storage, can be safely discarded since it's refreshing every compute.
  - new_node() constructor using init_node_schema() method to build initial attributes
  - new_comp() constructor that creates default attributes for the Comp type
  - new_project() does the same for project, etc, etc.
  - specialized compute() method that processed input/output attributes in self.attr
  - this struct is persistent with serialization to JSON and from JSON via Node.attr
  - Examples:
    - Node.attr.get("something");
    - Node.attr.set("something", variant);
    - Node.data.set("tmp_data", data);


### Comp (entities/comp.rs)
Comp is a specialized Node with a schema of Nested Comp: that's a container that can contain other Comps and Comp-compatible entities (actually UUIDs to them).
It's schema may include:
  - inputs: hashmap of <attr_name: (uuid, name), ..] referring to nodes and attributes in Project.data Hashmap
  - outputs: HashMap of <name:data>
  - internal variables (initialized with init_comp() for instance) like:
    - frame_start, frame_end (original comp range, usually 0..num_frames, but can be different for file sequences that might start even from negative frames: -20..170) or for nested comps that can be moved to the negative domain for example.
    - trim_start, trim_end: trim values for nested Comp (in local comp time)
    - speed
  - Customized compute() that collects inputs (children comps that recursively collect inputs too) and performs various functions over them, producing a Frame in the output attribute (do we have overrides in Rust?)
  - We should be able to run compute(ctx) from multiple threads in different contexts simultaneously for true multithreading. We have Project::Workers thread pool for this.
  - Customized set/get_cache(uuids, frame, &Frame) function that retrieves or puts resulting frame into the global project cache
  - Comp keeps it's children in Attrs, in initialized Attrs.get("children") method and it's just a list of tuples.
  - Tuples, because we keep Tuple(uuid, Attrs) - attributes of this "layer" inside of this Comp: in, speed, trim_in, trim_out

IMPORTSNT: All attributes below we're moving to Comp.attr, Project.attr, etc, for ALL PANELS AND ENTITIES! Attrs is out new state serialization system!

```rust
struct Comp {
    attr: Attrs,
      uuid: Uuid,                // uuid::Uuid - 16 bytes fixed, Copy trait
      name: String,
      comp_type: CompType,       // Normal (container) | File (image or video)
      frame: i32,                // current playhead position
      width, height, fps
      in, out                    // in and out of any Comp is positive or negative i32, constraint: in is ALWAYS lesser than out.
      trim_in, trim_out          // this is basically a "play range" of this comp, time constraint. Nothing is played or cached outside these bounds, cache is getting freed when bounds are changing. in local frames, if in=0 and out=100, trim_in=20 and trim_out=80, it mean for the "outer world" the start of this comp is trim_in and end is trim_out. By default they're matching in and out and changing with in and out unless they're not matching, i.e. when you try to change in and out you see trim_in and trim_out are differ so we don't touch them.
      dirty: Arc<AtomicBool>,    // cache invalidation flag, this flag is on when something been changed in attrs. Need a mechanism to update it from Comp.attr.set("some_attr", val) -> Comp.attr.dirty = true.
      // DAG relations:
      children: Vec<Tuple(src_uuid, children_attr)>  // nested child compositions: uuid of a source comp and it's children_attr: position, rotation, scale (Vec3), visibility, in (on what frame it starts in this Comp), speed=1.0, length = ? (calculated from internal src_uuid trim_out-trim_in * speed)
      // Compositing:
      compose_solo: bool,
      compose_mute: bool,
      compose_opacity: f32,
      compose_blend_mode: BlendMode,
      // State:
      selection: Vec<Uuid>       // This comp's internal selection, just like in Project, but it's different selection. Each Comp can has it's own selection.
}
```

Children - это вложенные композиции.
It should contain functions to map this Comp's frame to a nested Comp's local frame and back (accounting for it's in point and speed and such attributes).

**Ключевые атрибуты Comp:**

- `fps` (Float) - framerate
- `padding` (UInt) - padding в filename
- `name` (Str) - display name
- `format` (Str) - source format description

**Dirty Tracking:**
- `set()` → marks dirty
- `is_dirty()` → check flag
- `clear_dirty()` → after cache update (thread-safe)

**Hashing:**
- `hash_all()` - полный hash для cache key
- `hash_filtered(include, exclude)` - selective hash
- Keys sorted для determinism
- Floats hashed via `to_bits()`





Нужно подумать, можно ли как-то сделать Comp трейтом? 
Или загрузку media трейтом?
По умолчанию Comp - просто контейнер, он не предполагает загрузки файлов.
Но мне также нужны Comp которые читать media files.
Как лучше сделать?




### Project (entities/project.rs)
Контейнер всех Comp, Clips + cache management.

```rust
struct Project {
    // Все атрибуты также кочуют в Attrs: Project.attr.get()/set()
    media: Arc<RwLock<HashMap<Uuid, Comp>>>,   // uuid -> Comp (Uuid as key)
    order: Vec<Uuid>,                          // project list order (user can reorder clips and comps in the window with drag'n'drop, this is pure cosmetical action)
    selection: Vec<Uuid>,                      // selected UUIDs
    active: Option<Uuid>,                      // active comp UUID
    cache_manager: Arc<CacheManager>,
    global_cache: Arc<GlobalFrameCache>,       // Global Cache should be HashMap<UUID:[Frames]>, then removal is trivial.
    compositor: RefCell<CompositorType>,
}
```







### Player (player.rs)
Playback engine с frame-accurate timing.

```rust
struct Player {
    attr: Attrs {
      project: &Project,
      active_comp: Option<Uuid>,  // Uuid is Copy, no clone needed
      is_playing: bool,
      fps_base: f32,              // persistent base FPS
      fps_play: f32,              // current playback FPS (resets on stop)
      loop_enabled: bool,
      play_direction: f32,        // 1.0 forward, -1.0 backward
      last_frame_time: Option<Instant>,
    }
}
```

**JKL Controls:**
- `J` - jog backward (1x → 2x → 4x → 8x...)
- `K` - stop
- `L` - jog forward (1x → 2x → 4x → 8x...)
- Direction change resets speed to 1x

**FPS Presets:** 1, 2, 4, 8, 12, 24, 30, 60, 120, 240, 480, 960



### Attribute Editor (widgets/ar/ae_ui.rs)
Редактор атрибутов. В зависимости от типа атрибута строит интерфейс для работы с атрибутами: текстовые поле, цифры, слайдер, color picker, combobox, ещё что-то.
Изменения атрибутов сразу записываются в выбранные ноды.


----------

Project
Timeline
Viewport
Encoder
Preferences

Все части изолированы друг от друга и общаются исключительно через EventBus.
Они настолько изолированы что должны существовать вообще отдельно, как панель без основного приложения.
У них есть стандартизированные интерфейсы для посылки/приёма сигнала и данных




---

## Data Flow

### 1. Frame Loading Pipeline

```
User drops file/folder
        │
        ▼
┌───────────────────┐
│ Project.add_media │ → detect type (sequence/video)
└───────┬───────────┘
        │
        ▼
┌───────────────────┐
│ Comp::new_*       │ → create Comp with metadata
└───────┬───────────┘   (frame count, dimensions, fps)
        │
        ▼
┌───────────────────┐
│ Player.set_active │ → activate comp, emit CurrentFrameChanged
└───────┬───────────┘
        │
        ▼
┌───────────────────┐
│ Comp.get_frame()  │ → load/compose frame
└───────┬───────────┘
        │
    ┌───┴───┐
    │       │
    ▼       ▼
[Cache]  [Loader]
  hit?     │
    │      ▼
    │   Workers.execute_with_epoch()
    │      │
    │      ▼
    │   Loader::load_frame()
    │      │ (EXR/PNG/FFmpeg)
    │      ▼
    │   GlobalFrameCache.insert()
    │      │
    └──────┴──────▶ Frame → Compositor → GPU Texture → Viewport
```

### 2. Playback Loop

```
Player.update() [called at 60Hz]
        │
        ▼
┌───────────────────────────────┐
│ Check elapsed >= 1/fps_play   │
└───────┬───────────────────────┘
        │ yes
        ▼
┌───────────────────────────────┐
│ Player.advance_frame()        │
│ - handle loop/clamp           │
│ - Comp.set_current_frame()    │
└───────┬───────────────────────┘
        │
        ▼
┌───────────────────────────────┐
│ CompEvent::CurrentFrameChanged│
│ emitted via EventBus          │
└───────┬───────────────────────┘
        │
        ▼
┌───────────────────────────────┐
│ ViewportRenderer picks up     │
│ new frame ref from Player     │
└───────────────────────────────┘
```

### 3. Compositing Pipeline

```
Comp.get_frame(frame_idx)
        │
        ▼
┌───────────────────────────────┐
│ Collect visible layers at idx │
│ (filter: !mute, opacity > 0)  │
└───────┬───────────────────────┘
        │
        ▼
┌───────────────────────────────┐
│ Load each layer's frame       │
│ (recursive for nested comps)  │
└───────┬───────────────────────┘
        │
        ▼
┌───────────────────────────────┐
│ Compositor.blend_with_dim()   │
│ ┌─────────────────────────┐   │
│ │ CpuCompositor (fallback)│   │
│ │ GpuCompositor (OpenGL)  │   │
│ └─────────────────────────┘   │
└───────┬───────────────────────┘
        │
        ▼
┌───────────────────────────────┐
│ Blended Frame                 │
│ (status = min(all inputs))    │
└───────────────────────────────┘
```

### 4. Encoding Pipeline

```
EncodeDialog → EncoderSettings
        │
        ▼
┌───────────────────────────────┐
│ encode_comp()                 │
│ - validate first frame dims   │
│ - select encoder (HW → CPU)   │
└───────┬───────────────────────┘
        │
        ▼
┌───────────────────────────────┐
│ For each frame in play_range: │
│   1. Comp.get_frame()         │
│   2. Crop to target dims      │
│   3. Tonemap if HDR→8bit      │
│   4. Convert to RGB24/RGB48   │
│   5. SwsContext → YUV         │
│   6. Send to encoder          │
│   7. Write packets to muxer   │
└───────┬───────────────────────┘
        │
        ▼
┌───────────────────────────────┐
│ Flush encoder + write trailer │
│ → MP4/MOV output              │
└───────────────────────────────┘
```

---

## Sequence Detection (comp.rs)

Алгоритм детекции image sequences из dropped файлов.

### Pipeline

```
User drops files
      │
      ▼
┌─────────────────────────────┐
│ Comp::detect_from_paths()   │
└───────┬─────────────────────┘
        │
        ▼
┌─────────────────────────────┐
│ For each path:              │
│   is_video? → create_video_comp()
│   else → split_sequence_path()
└───────┬─────────────────────┘
        │
        ▼
┌─────────────────────────────┐
│ split_sequence_path(path)   │
│ "render.0001.exr"           │
│ → (prefix, number, ext, pad)│
│ → ("render.", 1, "exr", 4)  │
└───────┬─────────────────────┘
        │
        ▼
┌─────────────────────────────┐
│ detect_sequence_from_pattern│
│ glob("render.*.exr")        │
│ group by (prefix, ext)      │
│ select largest group        │
└───────┬─────────────────────┘
        │
        ▼
┌─────────────────────────────┐
│ Create Comp                 │
│ file_mask = "render.*.exr"  │
│ frame_range = [min, max]    │
│ dimensions from first frame │
└─────────────────────────────┘
```

### split_sequence_path()

Парсинг filename → (prefix, number, ext, padding).

```rust
"/path/seq.0001.exr"
       ↓
(prefix:  "/path/seq.",
 number:  1,
 ext:     "exr",
 padding: 4)  // длина "0001"
```

**Правила:**
- Ищем trailing digits в stem перед extension
- Padding = длина числовой части (учитывает leading zeros)
- Нет trailing digits → single file, не sequence

### Grouping & Deduplication

```rust
// Group by (prefix, ext)
groups: HashMap<(String, String), Vec<(number, path, padding)>>

// Select largest group as main sequence
let (key, frames) = groups.max_by_key(|v| v.len());

// Deduplicate comps by file_mask
unique: HashMap<String, Comp>  // mask → comp
```

### Video Detection

```rust
const VIDEO_EXTS: &[&str] = &["mp4", "mov", "avi", "mkv"];

// Also handles video@frame syntax: "video.mp4@17" → frame 17
fn parse_video_path(path) → (actual_path, Option<frame_idx>)
```

### Example

```
Input: /shots/render.0001.exr
       /shots/render.0002.exr
       /shots/render.0010.exr

Pattern: /shots/render.*.exr
Result:  Comp { file_mask: "/shots/render.*.exr",
                file_start: 1,
                file_end: 10,
                padding: 4 }
```

---

## Subsystems

### Cache System

**GlobalFrameCache** (global_cache.rs):
- LRU eviction по entry count
- Key: `(Uuid, frame_idx)` - 16-byte UUID + frame index
- Strategies: `LastOnly` (minimal RAM) | `All` (max perf)
- Stats: hits/misses/hit_rate

**CacheManager** (cache_man.rs):
- Memory tracking (usage vs limit)
- Epoch counter для cancellation
- `check_memory_limit()` → triggers eviction

### Worker Pool

**Workers** (workers.rs):
- Crossbeam work-stealing deques
- `execute()` - fire and forget
- `execute_with_epoch()` - cancellable by epoch change
- Recommended: `num_cpus * 3/4` threads

### Event System

**EventBus** (events.rs):
- `crossbeam` based
- Events: `AppEvent`, `ProjectEvent`, `CompEvent`, `PlayEvent`, `IOEvent`, `KeyEvent`, `MouseEvent`
- UI → EventBus → Player/Project mutations, playback, dialogs, everything.

**Key Events:**
- `CurrentFrameChanged` - playhead moved
- `LayerSelected([Uuids])` - layer selection (Uuids by value)
- `SetFrame(idx)` - jump to frame
- `PlaybackToggle`, `Stop`, `JogForward`, `JogBackward`

---

## UI Widgets

### Timeline (widgets/timeline/)

```
┌─────────────────────────────────────────────────────────────┐
│ [Toolbar] [View: Split|Canvas|Outline]                      │
├─────────────────────────────────────────────────────────────┤
│ Outline              │ Canvas      ▲ playhead               │
│ ┌──────────────────┐ │ ┌───────────────────────────────────┐│
│ │ Layer 1 [S][M]   │ │ │▓▓▓▓▓▓▓▓▓▓░░░░░░░░░░░░░░░░░░░░░░░░░││
│ │ Layer 2 [S][M]   │ │ │░░░░░░░▓▓▓▓▓▓▓▓▓▓▓▓▓▓░░░░░░░░░░░░░░││
│ │ Layer 3 [S][M]   │ │ │░░░░░░░░░░░░░░░░░▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓││
│ │                  │ │ │                                   ││
│ └──────────────────┘ │ └───────────────────────────────────┘│
└──────────────────────┴──────────────────────────────────────┘
```

**State:**
- `zoom` - pixels per frame
- `pan_offset` - horizontal scroll
- `view_mode` - Split | CanvasOnly | OutlineOnly
- `outline_width` - resizable splitter

**Drag operations:**
- Timeline pan (middle mouse)
- Layer move (horizontal + vertical reorder)
- Layer trim (left/right edges)
- Project item drop (add to timeline)

### Viewport (widgets/viewport/)

**Modes:**
- `AutoFit` - image fits window, adjusts on resize
- `Auto100` - 100% zoom, centered
- `Manual` - user-controlled zoom/pan

**Features:**
- GPU rendering via custom shaders
- Center-on-cursor zoom
- Viewport scrubbing (click-drag = timeline navigation)
- HDR→LDR tonemapping (ACES, Reinhard, Clamp)

**ViewportRenderer (renderer.rs):**
- PBO double-buffering для async texture upload
- Exposure/gamma controls для HDR preview
- Auto-detect HDR (F16/F32) vs LDR (U8)

```rust
struct ViewportRenderer {
    pbos: [Option<glow::Buffer>; 2],   // double-buffer PBOs
    pbo_index: usize,                   // ping-pong index
    exposure: f32,                      // default 1.0
    gamma: f32,                         // default 2.2 (sRGB)
    current_shaders: Shaders,
}
```

---

## GPU Shaders (shaders.rs)

GLSL #version 330 core. Загрузка: embedded → external (./shaders/*.glsl).

### Vertex Shader (общий для всех)

```glsl
layout (location = 0) in vec2 a_pos;
layout (location = 1) in vec2 a_uv;

uniform mat4 u_view;
uniform mat4 u_projection;

out vec2 v_uv;

void main() {
    gl_Position = u_projection * u_view * vec4(a_pos, 0.0, 1.0);
    v_uv = a_uv;
}
```

### Fragment Shader Uniforms

```glsl
uniform sampler2D u_texture;
uniform float u_exposure;  // Exposure multiplier (default 1.0)
uniform float u_gamma;     // Gamma correction (default 2.2)
uniform int u_is_hdr;      // 1 for HDR (F16/F32), 0 for LDR (U8)
```

### Embedded Shaders

| Name | Algorithm |
|------|-----------|
| `default` | Exposure + gamma: `pow(color * exposure, 1/gamma)` |
| `tonemap_reinhard` | Reinhard: `color / (1 + color)` → gamma |
| `tonemap_aces` | ACES Filmic curve → gamma |

**ACES Filmic:**
```glsl
vec3 ACESFilm(vec3 x) {
    float a = 2.51, b = 0.03;
    float c = 2.43, d = 0.59, e = 0.14;
    return clamp((x*(a*x + b))/(x*(c*x + d) + e), 0.0, 1.0);
}
```

**Reinhard:**
```glsl
vec3 ReinhardTonemap(vec3 color) {
    return color / (1.0 + color);
}
```

**HDR/LDR Path:**
- `u_is_hdr == 1`: apply exposure → tonemap → gamma
- `u_is_hdr == 0`: passthrough (already sRGB)

**Shader Directory:**
- External `.glsl` files в `./shaders/` переопределяют embedded
- Vertex shader всегда embedded (shared)

### Project Panel (widgets/project/)

- List of all comps in project
- Selection (multi-select supported)
- Drag to timeline
- Context menu (remove, duplicate, etc.)

### Dialogs

**Encode Dialog** (dialogs/encode/):
- Codec selection: H.264, H.265, AV1, ProRes
- Encoder impl: Auto (HW→CPU), Hardware only, Software only
- Quality: CRF (quality) | Bitrate (kbps)
- Presets per codec
- Progress bar + cancel

**Preferences** (dialogs/prefs/):
- Cache strategy
- Memory limits
- Keyboard bindings
- (Future: compositor backend)

---

## Supported Formats

### Input (via Loader)

| Format | Pixel Depth | Notes |
|--------|-------------|-------|
| EXR | F16, F32 | HDR, multi-channel |
| PNG | 8-bit, 16-bit | with alpha |
| JPG | 8-bit | lossy |
| TIFF | 8-bit, 16-bit | LZW/ZIP compression |
| TGA | 8-bit | with alpha |
| HDR | F32 | Radiance RGBE |
| Video | varies | via FFmpeg (MP4, MOV, AVI, MKV) |

---

## EXR Loading (loader.rs)

### Backends

| Backend | Feature Flag | Compression Support |
|---------|--------------|---------------------|
| `exrs` (via image crate) | default | ZIP, ZIPS, PIZ, PXR24, RLE, B44 |
| `openexr-rs` (C++ bindings) | `--openexr` | + DWAA, DWAB (lossy) |

**Build с полной поддержкой EXR:**
```bash
cargo xtask build --openexr
```

### Channel Handling

```
EXR File                    →    Frame (RGBA F16)
┌─────────────────┐              ┌─────────────┐
│ R, G, B channels│  ────────→   │ R, G, B, A  │
│ A channel (opt) │  or 1.0 ──→  │ (alpha=1.0  │
└─────────────────┘              │  if missing)│
                                 └─────────────┘
```

- Channels read: `R`, `G`, `B`, `A` (стандартные имена)
- Alpha fallback: если A отсутствует → `1.0`
- Data window → full image bounds
- Output format: `RgbaF16` (half float per channel)

### Colorspace

- EXR читается как **linear** (scene-referred)
- Colorspace conversion: **нет** (raw linear values)
- Tonemapping: выполняется в viewport shaders (ACES/Reinhard)
- Gamma: применяется при display (2.2 sRGB)

```
EXR (linear) → Exposure → Tonemap → Gamma → Display (sRGB)
```

### Error Handling

- DWAA/DWAB без openexr feature → `UnsupportedFormat` error
- Missing channels → fallback values (black + alpha 1.0)
- Corrupt file → `FrameError::Image`

### Output (via Encoder)

| Codec | Container | Encoders | Notes |
|-------|-----------|----------|-------|
| H.264 | MP4/MOV | libx264, NVENC, QSV, AMF, VideoToolbox | 8-bit |
| H.265 | MP4/MOV | libx265, NVENC, QSV, AMF, VideoToolbox | 8/10-bit |
| AV1 | MP4 | libsvtav1, libaom-av1, NVENC, QSV, AMF | 8/10-bit |
| ProRes | MOV | prores_ks | 10-bit 4:2:2 |

---

## Blend Modes

GPU compositor (GLSL) и CPU fallback поддерживают:

| Mode | Formula |
|------|---------|
| Normal | `top` |
| Screen | `1 - (1-bottom)*(1-top)` |
| Add | `min(bottom + top, 1)` |
| Subtract | `max(bottom - top, 0)` |
| Multiply | `bottom * top` |
| Divide | `bottom / max(top, 0.00001)` |
| Difference | `abs(bottom - top)` |

---

## Keyboard Shortcuts

### Playback
| Key | Action |
|-----|--------|
| Space / ↑ | Play/Pause toggle |
| K / . / ↓ | Stop |
| J / , | Jog backward (accelerates) |
| L / / | Jog forward (accelerates) |
| ` | Toggle loop |

### Navigation
| Key | Action |
|-----|--------|
| ← → | Step 1 frame |
| Shift+← → | Step 25 frames |
| Ctrl+← → | Jump to start/end |
| Home / 1 | Jump to start |
| End / 2 | Jump to end |
| ; | Previous layer edge |
| ' | Next layer edge |

### Work Area
| Key | Action |
|-----|--------|
| B | Set play range start |
| N | Set play range end |
| Ctrl+B | Reset play range |

### Layer Operations
| Key | Action |
|-----|--------|
| [ | Align layer start to cursor |
| ] | Align layer end to cursor |
| Alt+[ | Trim layer start to cursor |
| Alt+] | Trim layer end to cursor |
| Delete | Remove selected layer |

### Viewport
| Key | Action |
|-----|--------|
| F | Fit to view |
| A / H | 100% zoom |
| Scroll | Zoom (center on cursor) |
| Middle drag | Pan |
| Click drag | Scrub timeline |

### UI
| Key | Action |
|-----|--------|
| F1 | Help overlay |
| F2 | Project panel |
| F3 | Attributes panel |
| F4 | Encode dialog |
| F5 | Preferences |
| Z | Fullscreen |
| ESC | Exit fullscreen / Quit |

---

## Config & Paths

**Priority:**
1. CLI `--config-dir`
2. ENV `PLAYA_CONFIG_DIR`
3. Local folder (if playa.json exists)
4. Platform default:
   - Windows: `%APPDATA%\playa\`
   - macOS: `~/Library/Application Support/playa/`
   - Linux: `~/.config/playa/`

**Files:**
- `playa.json` - settings (persistent)
- `playa.log` - log file
- `playa_data.json` - cache metadata

---

## Future / TODO

- [ ] GPU compositor UI toggle (settings)
- [ ] OCIO color management
- [ ] Audio support
- [ ] Markers / keyframes
- [ ] Multi-view (A/B compare)
- [ ] Annotations / drawing tools
- [ ] Plugin system
