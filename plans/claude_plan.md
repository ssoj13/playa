# Playa: Plan for 3D Transforms & REST API

## Executive Summary

Исследование кодовой базы Playa для добавления:
1. **3D трансформаций** - полноценные XYZ rotation, Z-translation, XYZ scale
2. **REST API сервера** - управление плеером через HTTP endpoints

---

## Part 1: Current Transform System Analysis

### 1.1 Gizmo System (src/widgets/viewport/gizmo.rs)

**Текущие возможности:**
- Библиотека `transform_gizmo_egui`
- 4 инструмента: Select (Q), Move (W), Rotate (E), Scale (R)
- **Только 2D**: XY translation, Z rotation, XY uniform scale

**Ключевые функции:**
```rust
// Конвертация Layer -> Gizmo space
fn layer_to_gizmo_transform(layer, mode) -> GizmoTransform {
    // Нормализует scale для Move/Rotate (force 1.0)
    // Инвертирует Y (AE convention: Y down)
    // Конвертирует degrees -> radians
    // ИГНОРИРУЕТ rotation X/Y (только Z используется)
}

// Конвертация Gizmo -> Layer
fn gizmo_to_layer_transform(result) -> (pos, rot, scale) {
    // Извлекает quaternion -> Euler XYZ
    // Конвертирует radians -> degrees
    // Инвертирует Y обратно
}
```

**Ограничения для 3D:**
- `layer_to_gizmo_transform()` игнорирует rotation[0], rotation[1]
- Гизмо настроен на ортографическую проекцию (2D viewport)
- Нет Z-axis manipulation в Move tool

### 1.2 Transform Application (src/entities/transform.rs)

**Текущий CPU transform:**
```rust
pub fn transform_frame(src, canvas, position, rotation_z, scale, pivot) -> Frame {
    // Использует только rotation_z (2D)
    // Affine2 матрица (2x3)
    // Параллельная обработка через rayon
}

pub fn build_inverse_matrix_3x3(...) -> [f32; 9] {
    // Матрица для GPU (готова, но не используется)
    // Только 2D трансформ
}
```

**Для 3D потребуется:**
- `Mat4` вместо `Affine2`
- Полный Euler XYZ rotation order
- Perspective-correct sampling
- Depth buffer для Z-sorting

### 1.3 Layer Attributes (src/entities/attr_schemas.rs)

**Уже есть Vec3 для всех трансформов:**
```rust
// TRANSFORM schema entries:
("position", Vec3([0.0, 0.0, 0.0]), DAG_DISP_KEY),  // XYZ ready!
("rotation", Vec3([0.0, 0.0, 0.0]), DAG_DISP_KEY),  // XYZ ready!
("scale",    Vec3([1.0, 1.0, 1.0]), DAG_DISP_KEY),  // XYZ ready!
("pivot",    Vec3([0.0, 0.0, 0.0]), DAG_DISP_KEY),  // XYZ ready!
```

**Вывод:** Структура данных УЖЕ поддерживает 3D! Нужно только:
- UI для редактирования Z/rotX/rotY
- Rendering pipeline с 3D матрицами

### 1.4 Camera Node (src/entities/camera_node.rs)

**Полностью реализована:**
```rust
impl CameraNode {
    fn view_matrix(&self) -> Mat4 {
        // Look-at mode или Euler rotation mode
    }
    
    fn projection_matrix(&self, aspect: f32) -> Mat4 {
        // Perspective projection (FOV, near, far)
    }
    
    fn view_projection_matrix(&self, aspect: f32) -> Mat4 {
        // Combined VP matrix
    }
}
```

**Атрибуты камеры:**
- position, rotation, scale, pivot (стандартные)
- fov: 39.6 degrees (AE default)
- near_clip, far_clip
- point_of_interest (look-at target)
- use_poi: bool (look-at vs rotation mode)

**Статус:** Готова к использованию, но НЕ интегрирована в композитинг.

---

## Part 2: 3D Transform Implementation Plan

### 2.0 Architecture: Camera Integration

**Ключевая идея:** CameraNode уже полностью готова. Нужно:
1. В `compose_internal()` найти слой-камеру
2. Получить view_projection matrix
3. Передать в transform функции

**Поиск камеры в композе:**
```rust
// comp_node.rs - в начале compose_internal()
fn find_camera(&self, ctx: &ComputeContext) -> Option<&CameraNode> {
    for layer in &self.layers {
        if let Some(node) = ctx.media.get(&layer.source_uuid()) {
            if let Some(cam) = node.as_camera() {
                return Some(cam);
            }
        }
    }
    None
}
```

**Default camera (fallback):**
```rust
fn default_camera(comp_width: f32, comp_height: f32) -> CameraParams {
    CameraParams {
        // Позиция так чтобы comp занимал весь viewport при FOV 39.6°
        position: [comp_width / 2.0, comp_height / 2.0, -comp_height / (2.0 * tan(19.8°))],
        poi: [comp_width / 2.0, comp_height / 2.0, 0.0],
        fov: 39.6,
        near: 1.0,
        far: 10000.0,
    }
}
```

**Использование в compose_internal:**
```rust
// В начале compose_internal:
let camera = self.find_camera(ctx);
let aspect = comp_width as f32 / comp_height as f32;
let vp_matrix = match camera {
    Some(cam) => cam.view_projection_matrix(aspect),
    None => default_orthographic_matrix(comp_width, comp_height),
};

// При обработке каждого слоя:
let model_matrix = build_model_matrix_3d(pos, rot, scl, pvt, layer_center);
let mvp = vp_matrix * model_matrix;
frame = transform_frame_3d(&frame, canvas, mvp);
```

### 2.1 Phase 1: Data & UI (Low Risk)

**Шаг 1.1: Attribute Editor UI**
- Файл: `src/widgets/attr_editor.rs`
- Добавить редактирование rotation[0], rotation[1] (сейчас только rotation[2])
- Добавить редактирование position[2] (Z depth)
- Добавить редактирование scale[2]

**Шаг 1.2: Timeline Keyframe Support**
- Файл: `src/widgets/timeline/keyframe_editor.rs`
- Убедиться что все 3 компонента Vec3 keyframeable

### 2.2 Phase 2: Gizmo 3D Mode (Medium Risk)

**Шаг 2.1: Gizmo Configuration**
```rust
// gizmo.rs - добавить 3D режимы
pub enum GizmoMode {
    Mode2D,  // Текущее поведение
    Mode3D,  // Новый режим
}

// В 3D режиме:
// - Move: XYZ axes
// - Rotate: XYZ trackball или Euler handles
// - Scale: XYZ non-uniform
```

**Шаг 2.2: Viewport Camera для 3D**
- Отдельная "edit camera" для viewport (не composition camera)
- Orbit/Pan/Zoom controls
- Переключение между Top/Front/Side/Perspective views

**Шаг 2.3: Матрицы для Gizmo**
```rust
fn build_gizmo_matrices_3d(viewport, camera) -> (view: Mat4, proj: Mat4) {
    // Использовать CameraNode для perspective projection
    // Или custom orbit camera для viewport
}
```

### 2.3 Phase 3: 3D Compositor (High Complexity)

**Шаг 3.1: GPU Transform Path**
- Файл: `src/entities/gpu_compositor.rs` (уже существует!)
- Модифицировать шейдер для Mat4 transforms
- Добавить depth buffer
- Z-sorting слоёв

**Шаг 3.2: CPU Fallback**
- Файл: `src/entities/transform.rs`
- `transform_frame_3d()` с perspective projection
- Значительно медленнее GPU, но нужен для compatibility

**Шаг 3.3: Composition Camera Integration**
```rust
// comp_node.rs - compose_internal()
fn compose_internal(...) -> Option<Frame> {
    // 1. Get active camera (или default)
    let camera = self.get_camera_node()?;
    let vp_matrix = camera.view_projection_matrix(aspect);
    
    // 2. Transform layers in 3D
    for layer in layers {
        let model_matrix = layer.build_model_matrix();  // NEW
        let mvp = vp_matrix * model_matrix;
        // Apply 3D transform...
    }
    
    // 3. Z-sort and composite
    layers.sort_by(|a, b| a.z_depth().cmp(&b.z_depth()));
}
```

### 2.4 Transform Order (AE-compatible)

```
Final = T(anchor) * T(position) * Rz * Ry * Rx * S(scale) * T(-anchor)

Where:
- anchor = layer center + pivot offset
- Rotation order: Z * Y * X (AE standard)
```

### 2.5 Риски и Considerations

| Аспект | Риск | Mitigation |
|--------|------|------------|
| Performance | High - 3D transforms CPU-heavy | GPU compositor path |
| Compatibility | Medium - existing projects | Default to 2D mode, opt-in 3D |
| UX Complexity | High - 3D navigation hard | Good defaults, 2D/3D toggle |
| Gizmo library | Low - supports 3D | Already capable |

---

## Part 3: REST API Server Implementation Plan

### 3.1 Current Architecture

**Player System (src/core/player.rs):**
```rust
impl Player {
    pub fn play(&mut self)
    pub fn pause(&mut self)  
    pub fn stop(&mut self)
    pub fn set_frame(&mut self, frame: i32)
    pub fn step(&mut self, count: i32)
    pub fn is_playing(&self) -> bool
    pub fn current_frame(&self) -> i32
}
```

**Event Bus (src/core/event_bus.rs):**
```rust
// Существующие события:
TogglePlayPauseEvent
StopEvent
SetFrameEvent(i32)
StepForwardEvent
StepBackwardEvent
JogForwardEvent / JogBackwardEvent
```

**Threading Model:**
- Main thread: egui UI loop (60Hz)
- Worker threads: frame loading (crossbeam work-stealing)
- NO async runtime currently

### 3.2 Recommended HTTP Server

**Option A: tiny_http (Recommended)**
```toml
[dependencies]
tiny_http = "0.12"  # Minimal, sync, no async runtime needed
```
- Pros: Zero dependencies, simple, blocking API
- Cons: No async, но для REST API это OK

**Option B: axum + tokio**
```toml
[dependencies]
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
axum = "0.7"
```
- Pros: Modern, async, middleware ecosystem
- Cons: Heavy dependency tree, complexity

**Recommendation:** Start with `tiny_http` for simplicity. Migrate to axum later if needed.

### 3.3 Architecture Design

```
┌─────────────────────────────────────────────────────────────┐
│                      Main Thread (egui)                      │
│  ┌─────────┐  ┌─────────┐  ┌──────────┐  ┌───────────────┐  │
│  │ Player  │  │ Project │  │ EventBus │  │ REST Receiver │  │
│  └────┬────┘  └────┬────┘  └────┬─────┘  └───────┬───────┘  │
│       │            │            │                │           │
│       └────────────┴────────────┴────────────────┘           │
│                           ▲                                  │
└───────────────────────────┼──────────────────────────────────┘
                            │ crossbeam channel
                            │
┌───────────────────────────┼──────────────────────────────────┐
│                    REST Server Thread                         │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────────┐   │
│  │ tiny_http   │───▶│ Router      │───▶│ Command Sender  │   │
│  │ listener    │    │ /play, etc  │    │ (to main thread)│   │
│  └─────────────┘    └─────────────┘    └─────────────────┘   │
└──────────────────────────────────────────────────────────────┘
```

### 3.4 REST API Endpoints

```
GET  /api/v1/status          → { playing, frame, fps, comp_name }
POST /api/v1/play            → Start playback
POST /api/v1/pause           → Pause playback  
POST /api/v1/stop            → Stop and go to start
POST /api/v1/toggle          → Toggle play/pause

GET  /api/v1/frame           → Current frame number
POST /api/v1/frame?frame=17  → Seek to frame 17
POST /api/v1/step?count=1    → Step forward/backward

GET  /api/v1/media           → List all media nodes
GET  /api/v1/media/{uuid}    → Get media node details
GET  /api/v1/comps           → List all compositions
GET  /api/v1/comps/{uuid}    → Get composition details

GET  /api/v1/project         → Project metadata
POST /api/v1/project/open    → Open project file (body: path)
POST /api/v1/project/save    → Save current project

GET  /api/v1/render/frame    → Render current frame as PNG/EXR
```

### 3.5 Implementation Steps

**Step 1: Add Dependencies**
```toml
# Cargo.toml
[dependencies]
tiny_http = "0.12"
crossbeam-channel = "0.5"  # Already in deps but unused
serde_json = "1.0"  # Already present
```

**Step 2: Create REST Module**
```
src/
├── core/
│   ├── rest_server.rs      # NEW: HTTP server thread
│   ├── rest_commands.rs    # NEW: Command enum & handlers
│   └── mod.rs              # Add pub mod rest_server
```

**Step 3: REST Server Implementation**
```rust
// src/core/rest_server.rs
use tiny_http::{Server, Response, Method};
use crossbeam_channel::Sender;

pub enum RestCommand {
    Play,
    Pause,
    Stop,
    SetFrame(i32),
    Step(i32),
    GetStatus(oneshot::Sender<PlayerStatus>),
    ListMedia(oneshot::Sender<Vec<MediaInfo>>),
}

pub struct RestServer {
    port: u16,
    command_tx: Sender<RestCommand>,
}

impl RestServer {
    pub fn new(port: u16, command_tx: Sender<RestCommand>) -> Self {
        Self { port, command_tx }
    }
    
    pub fn run(&self) {
        let server = Server::http(format!("127.0.0.1:{}", self.port)).unwrap();
        
        for request in server.incoming_requests() {
            let response = self.handle_request(&request);
            request.respond(response).ok();
        }
    }
    
    fn handle_request(&self, req: &Request) -> Response<Cursor<Vec<u8>>> {
        match (req.method(), req.url()) {
            (Method::Post, "/api/v1/play") => {
                self.command_tx.send(RestCommand::Play).ok();
                json_response(200, r#"{"ok": true}"#)
            }
            (Method::Post, "/api/v1/pause") => {
                self.command_tx.send(RestCommand::Pause).ok();
                json_response(200, r#"{"ok": true}"#)
            }
            (Method::Post, "/api/v1/frame") => {
                let frame = parse_query_param(req.url(), "frame");
                self.command_tx.send(RestCommand::SetFrame(frame)).ok();
                json_response(200, r#"{"ok": true}"#)
            }
            (Method::Get, "/api/v1/status") => {
                let (tx, rx) = oneshot::channel();
                self.command_tx.send(RestCommand::GetStatus(tx)).ok();
                match rx.recv_timeout(Duration::from_secs(1)) {
                    Ok(status) => json_response(200, &serde_json::to_string(&status).unwrap()),
                    Err(_) => json_response(500, r#"{"error": "timeout"}"#),
                }
            }
            _ => json_response(404, r#"{"error": "not found"}"#),
        }
    }
}
```

**Step 4: Integration in main.rs**
```rust
// main.rs
use crossbeam_channel::{unbounded, Receiver};

struct PlayaApp {
    // ... existing fields ...
    rest_command_rx: Receiver<RestCommand>,
}

impl Default for PlayaApp {
    fn default() -> Self {
        let (rest_tx, rest_rx) = unbounded();
        
        // Spawn REST server thread
        let port = 8080;  // Or from settings
        std::thread::spawn(move || {
            let server = RestServer::new(port, rest_tx);
            server.run();
        });
        
        Self {
            rest_command_rx: rest_rx,
            // ...
        }
    }
}

impl eframe::App for PlayaApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // Handle REST commands (non-blocking)
        while let Ok(cmd) = self.rest_command_rx.try_recv() {
            match cmd {
                RestCommand::Play => self.event_bus.emit(TogglePlayPauseEvent),
                RestCommand::Pause => self.player.pause(),
                RestCommand::SetFrame(f) => self.event_bus.emit(SetFrameEvent(f)),
                RestCommand::GetStatus(tx) => {
                    let status = PlayerStatus {
                        playing: self.player.is_playing(),
                        frame: self.player.current_frame(),
                        // ...
                    };
                    tx.send(status).ok();
                }
                // ...
            }
        }
        
        // ... rest of update() ...
    }
}
```

**Step 5: Settings UI**
```rust
// src/widgets/settings.rs
pub struct ServerSettings {
    pub enabled: bool,
    pub port: u16,  // default 8080
    pub allow_remote: bool,  // default false (127.0.0.1 only)
}
```

### 3.6 Security Considerations

1. **Default: localhost only** - `127.0.0.1:8080`
2. **Optional remote access** - user must explicitly enable
3. **No authentication** for local - trust localhost
4. **Optional API key** for remote access
5. **Rate limiting** - prevent DoS
6. **Read-only mode option** - disable destructive operations

### 3.7 Testing

```bash
# Play
curl -X POST http://localhost:8080/api/v1/play

# Pause  
curl -X POST http://localhost:8080/api/v1/pause

# Seek to frame 17
curl -X POST "http://localhost:8080/api/v1/frame?frame=17"

# Get status
curl http://localhost:8080/api/v1/status
# {"playing":true,"frame":17,"fps":24.0,"comp":"Main Comp"}

# List media
curl http://localhost:8080/api/v1/media
# [{"uuid":"...","name":"footage.mov","type":"video","duration":120}]
```

---

## Part 4: Implementation Priority

### Phase 1: REST API (1-2 days)
1. Add `tiny_http` dependency
2. Create `rest_server.rs` module
3. Implement basic endpoints: play/pause/stop/frame/status
4. Add settings UI for port configuration
5. Test with curl

### Phase 2: 3D Transform UI (2-3 days)
1. Extend Attribute Editor for XYZ rotation/position
2. Add 2D/3D mode toggle in viewport
3. Update gizmo for 3D mode option

### Phase 3: 3D Compositor (1 week+)
1. Modify GPU compositor shaders for Mat4
2. Integrate CameraNode into compose pipeline
3. Implement depth sorting
4. CPU fallback path

---

## Appendix: Key Files Reference

| Component | File | Lines |
|-----------|------|-------|
| Gizmo | `src/widgets/viewport/gizmo.rs` | 335 |
| Transform | `src/entities/transform.rs` | 370 |
| Camera | `src/entities/camera_node.rs` | 265 |
| Compositor | `src/entities/gpu_compositor.rs` | 600+ |
| Player | `src/core/player.rs` | 200+ |
| Event Bus | `src/core/event_bus.rs` | 150+ |
| CompNode | `src/entities/comp_node.rs` | 1000+ |
| Attrs Schema | `src/entities/attr_schemas.rs` | 200+ |

---

*Generated: 2024-12-17*
