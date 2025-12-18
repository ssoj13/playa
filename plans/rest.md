# Playa REST API Server - Implementation Plan

## Overview

Встроенный HTTP сервер для удалённого управления плеером. Работает в отдельном потоке, не блокирует UI.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    Main Thread (egui 60fps)                      │
│                                                                  │
│  ┌─────────┐  ┌─────────┐  ┌──────────┐  ┌─────────────────┐    │
│  │ Player  │  │ Project │  │ EventBus │  │ rest_command_rx │    │
│  │         │  │         │  │          │  │ (Receiver)      │    │
│  └────┬────┘  └────┬────┘  └────┬─────┘  └────────┬────────┘    │
│       │            │            │                 │              │
│       └────────────┴────────────┴─────────────────┘              │
│                                 ▲                                │
│                                 │ try_recv() - non-blocking      │
└─────────────────────────────────┼────────────────────────────────┘
                                  │
                    crossbeam::channel (lock-free MPSC)
                                  │
┌─────────────────────────────────┼────────────────────────────────┐
│                    REST Server Thread                            │
│                                 │                                │
│  ┌──────────────┐    ┌─────────┴───────┐    ┌────────────────┐  │
│  │ tiny_http    │───▶│ Request Router  │───▶│ command_tx     │  │
│  │ Server       │    │                 │    │ (Sender)       │  │
│  │ :8080        │    │ /play /pause    │    │                │  │
│  └──────────────┘    │ /frame /status  │    └────────────────┘  │
│         ▲            └─────────────────┘                        │
│         │                                                        │
│    HTTP Request                                                  │
└──────────────────────────────────────────────────────────────────┘
```

## Dependencies

```toml
# Cargo.toml - добавить:
[dependencies]
tiny_http = "0.12"           # Минимальный sync HTTP server
crossbeam-channel = "0.5"    # Уже есть, но не используется
```

**Почему tiny_http:**
- Zero async runtime (не тянет tokio)
- Простой blocking API
- ~50KB compiled size
- Достаточно для REST API

## File Structure

```
src/
├── core/
│   ├── mod.rs              # добавить: pub mod rest;
│   ├── rest/
│   │   ├── mod.rs          # pub use
│   │   ├── server.rs       # HTTP server thread
│   │   ├── commands.rs     # RestCommand enum
│   │   ├── handlers.rs     # Request handlers
│   │   └── responses.rs    # JSON response builders
│   ├── player.rs
│   └── event_bus.rs
```

## API Endpoints

### Player Control

| Method | Endpoint | Description | Request | Response |
|--------|----------|-------------|---------|----------|
| `POST` | `/api/v1/play` | Start playback | - | `{"ok": true}` |
| `POST` | `/api/v1/pause` | Pause playback | - | `{"ok": true}` |
| `POST` | `/api/v1/stop` | Stop (go to start) | - | `{"ok": true}` |
| `POST` | `/api/v1/toggle` | Toggle play/pause | - | `{"ok": true, "playing": bool}` |
| `GET` | `/api/v1/status` | Get player state | - | See below |

**Status Response:**
```json
{
  "playing": true,
  "frame": 42,
  "fps": 24.0,
  "loop": true,
  "range": [0, 100],
  "comp": {
    "uuid": "...",
    "name": "Main Comp",
    "width": 1920,
    "height": 1080,
    "duration": 120
  }
}
```

### Frame Control

| Method | Endpoint | Description | Request | Response |
|--------|----------|-------------|---------|----------|
| `GET` | `/api/v1/frame` | Get current frame | - | `{"frame": 42}` |
| `POST` | `/api/v1/frame?frame=17` | Seek to frame | query: `frame` | `{"ok": true, "frame": 17}` |
| `POST` | `/api/v1/step?count=1` | Step frames | query: `count` (±) | `{"ok": true, "frame": 43}` |
| `POST` | `/api/v1/jog?direction=1` | JKL jog | query: `direction` (±1) | `{"ok": true, "speed": 2.0}` |

### Project & Media

| Method | Endpoint | Description | Response |
|--------|----------|-------------|----------|
| `GET` | `/api/v1/project` | Project info | `{"name": "...", "path": "...", "modified": bool}` |
| `GET` | `/api/v1/media` | List all media | `[{uuid, name, type, duration}, ...]` |
| `GET` | `/api/v1/media/{uuid}` | Media details | Full node info |
| `GET` | `/api/v1/comps` | List compositions | `[{uuid, name, width, height, fps}, ...]` |
| `GET` | `/api/v1/comps/{uuid}` | Comp details | Full comp info with layers |
| `GET` | `/api/v1/comps/{uuid}/layers` | List layers | `[{uuid, name, in, out, visible}, ...]` |

### Render (Advanced)

| Method | Endpoint | Description | Response |
|--------|----------|-------------|----------|
| `GET` | `/api/v1/render/frame` | Current frame as PNG | `image/png` binary |
| `GET` | `/api/v1/render/frame?format=exr` | Current frame as EXR | `image/x-exr` binary |
| `GET` | `/api/v1/render/thumbnail/{uuid}` | Media thumbnail | `image/png` binary |

## Implementation

### Step 1: Commands Enum

```rust
// src/core/rest/commands.rs

use crossbeam_channel::Sender as OneshotSender;
use uuid::Uuid;

/// Commands sent from REST thread to main thread
#[derive(Debug)]
pub enum RestCommand {
    // Player control
    Play,
    Pause,
    Stop,
    Toggle,
    
    // Frame control
    SetFrame(i32),
    Step(i32),
    Jog(i32),  // direction: 1 or -1
    
    // Queries (need response)
    GetStatus(OneshotSender<PlayerStatus>),
    GetFrame(OneshotSender<i32>),
    GetMedia(OneshotSender<Vec<MediaInfo>>),
    GetComps(OneshotSender<Vec<CompInfo>>),
    GetCompDetails(Uuid, OneshotSender<Option<CompDetails>>),
    
    // Render
    RenderCurrentFrame(OneshotSender<Option<Vec<u8>>>),
}

#[derive(Debug, Clone, Serialize)]
pub struct PlayerStatus {
    pub playing: bool,
    pub frame: i32,
    pub fps: f32,
    pub loop_enabled: bool,
    pub range: (i32, i32),
    pub comp: Option<CompInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MediaInfo {
    pub uuid: Uuid,
    pub name: String,
    pub node_type: String,
    pub duration: i32,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompInfo {
    pub uuid: Uuid,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub fps: f32,
    pub duration: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompDetails {
    pub info: CompInfo,
    pub layers: Vec<LayerInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LayerInfo {
    pub uuid: Uuid,
    pub name: String,
    pub source_uuid: Uuid,
    pub in_frame: i32,
    pub out_frame: i32,
    pub visible: bool,
    pub solo: bool,
    pub opacity: f32,
}
```

### Step 2: HTTP Server

```rust
// src/core/rest/server.rs

use tiny_http::{Server, Request, Response, Method, StatusCode};
use crossbeam_channel::{Sender, bounded};
use std::io::Cursor;
use std::time::Duration;

use super::commands::*;

pub struct RestServer {
    port: u16,
    command_tx: Sender<RestCommand>,
}

impl RestServer {
    pub fn new(port: u16, command_tx: Sender<RestCommand>) -> Self {
        Self { port, command_tx }
    }
    
    /// Run server (blocking - call from dedicated thread)
    pub fn run(&self) {
        let addr = format!("127.0.0.1:{}", self.port);
        let server = match Server::http(&addr) {
            Ok(s) => {
                log::info!("REST API server started on http://{}", addr);
                s
            }
            Err(e) => {
                log::error!("Failed to start REST server: {}", e);
                return;
            }
        };
        
        for request in server.incoming_requests() {
            let response = self.handle_request(&request);
            if let Err(e) = request.respond(response) {
                log::warn!("Failed to send response: {}", e);
            }
        }
    }
    
    fn handle_request(&self, req: &Request) -> Response<Cursor<Vec<u8>>> {
        let path = req.url().split('?').next().unwrap_or("");
        let method = req.method();
        
        log::debug!("REST: {} {}", method, req.url());
        
        match (method, path) {
            // Player control
            (&Method::Post, "/api/v1/play") => {
                self.send_command(RestCommand::Play);
                json_ok()
            }
            (&Method::Post, "/api/v1/pause") => {
                self.send_command(RestCommand::Pause);
                json_ok()
            }
            (&Method::Post, "/api/v1/stop") => {
                self.send_command(RestCommand::Stop);
                json_ok()
            }
            (&Method::Post, "/api/v1/toggle") => {
                self.send_command(RestCommand::Toggle);
                json_ok()
            }
            
            // Frame control
            (&Method::Get, "/api/v1/frame") => {
                self.query_frame()
            }
            (&Method::Post, "/api/v1/frame") => {
                let frame = parse_query_i32(req.url(), "frame").unwrap_or(0);
                self.send_command(RestCommand::SetFrame(frame));
                json_response(200, &format!(r#"{{"ok":true,"frame":{}}}"#, frame))
            }
            (&Method::Post, "/api/v1/step") => {
                let count = parse_query_i32(req.url(), "count").unwrap_or(1);
                self.send_command(RestCommand::Step(count));
                json_ok()
            }
            
            // Status
            (&Method::Get, "/api/v1/status") => {
                self.query_status()
            }
            
            // Media
            (&Method::Get, "/api/v1/media") => {
                self.query_media()
            }
            
            // Comps
            (&Method::Get, "/api/v1/comps") => {
                self.query_comps()
            }
            
            // CORS preflight
            (&Method::Options, _) => {
                cors_preflight()
            }
            
            // 404
            _ => {
                json_response(404, r#"{"error":"not found"}"#)
            }
        }
    }
    
    fn send_command(&self, cmd: RestCommand) {
        if let Err(e) = self.command_tx.send(cmd) {
            log::warn!("Failed to send REST command: {}", e);
        }
    }
    
    fn query_status(&self) -> Response<Cursor<Vec<u8>>> {
        let (tx, rx) = bounded(1);
        self.send_command(RestCommand::GetStatus(tx));
        
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(status) => {
                let json = serde_json::to_string(&status).unwrap_or_default();
                json_response(200, &json)
            }
            Err(_) => json_response(500, r#"{"error":"timeout"}"#)
        }
    }
    
    fn query_frame(&self) -> Response<Cursor<Vec<u8>>> {
        let (tx, rx) = bounded(1);
        self.send_command(RestCommand::GetFrame(tx));
        
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(frame) => json_response(200, &format!(r#"{{"frame":{}}}"#, frame)),
            Err(_) => json_response(500, r#"{"error":"timeout"}"#)
        }
    }
    
    fn query_media(&self) -> Response<Cursor<Vec<u8>>> {
        let (tx, rx) = bounded(1);
        self.send_command(RestCommand::GetMedia(tx));
        
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(media) => {
                let json = serde_json::to_string(&media).unwrap_or_default();
                json_response(200, &json)
            }
            Err(_) => json_response(500, r#"{"error":"timeout"}"#)
        }
    }
    
    fn query_comps(&self) -> Response<Cursor<Vec<u8>>> {
        let (tx, rx) = bounded(1);
        self.send_command(RestCommand::GetComps(tx));
        
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(comps) => {
                let json = serde_json::to_string(&comps).unwrap_or_default();
                json_response(200, &json)
            }
            Err(_) => json_response(500, r#"{"error":"timeout"}"#)
        }
    }
}

// === Helpers ===

fn json_response(status: u16, body: &str) -> Response<Cursor<Vec<u8>>> {
    let data = body.as_bytes().to_vec();
    Response::new(
        StatusCode(status),
        vec![
            tiny_http::Header::from_bytes("Content-Type", "application/json").unwrap(),
            tiny_http::Header::from_bytes("Access-Control-Allow-Origin", "*").unwrap(),
        ],
        Cursor::new(data),
        Some(body.len()),
        None,
    )
}

fn json_ok() -> Response<Cursor<Vec<u8>>> {
    json_response(200, r#"{"ok":true}"#)
}

fn cors_preflight() -> Response<Cursor<Vec<u8>>> {
    Response::new(
        StatusCode(204),
        vec![
            tiny_http::Header::from_bytes("Access-Control-Allow-Origin", "*").unwrap(),
            tiny_http::Header::from_bytes("Access-Control-Allow-Methods", "GET, POST, OPTIONS").unwrap(),
            tiny_http::Header::from_bytes("Access-Control-Allow-Headers", "Content-Type").unwrap(),
        ],
        Cursor::new(vec![]),
        Some(0),
        None,
    )
}

fn parse_query_i32(url: &str, key: &str) -> Option<i32> {
    let query = url.split('?').nth(1)?;
    for pair in query.split('&') {
        let mut kv = pair.split('=');
        if kv.next() == Some(key) {
            return kv.next()?.parse().ok();
        }
    }
    None
}
```

### Step 3: Main Thread Integration

```rust
// src/main.rs - изменения

use crossbeam_channel::{unbounded, Receiver, Sender};
use crate::core::rest::{RestServer, RestCommand, PlayerStatus, MediaInfo, CompInfo};

pub struct PlayaApp {
    // ... existing fields ...
    
    // REST API
    rest_command_rx: Receiver<RestCommand>,
    rest_enabled: bool,
}

impl Default for PlayaApp {
    fn default() -> Self {
        // ... existing code ...
        
        // REST server setup
        let (rest_tx, rest_rx) = unbounded();
        let rest_port = 8080; // TODO: from settings
        let rest_enabled = true; // TODO: from settings
        
        if rest_enabled {
            std::thread::Builder::new()
                .name("rest-server".into())
                .spawn(move || {
                    let server = RestServer::new(rest_port, rest_tx);
                    server.run();
                })
                .expect("Failed to spawn REST server thread");
        }
        
        Self {
            rest_command_rx: rest_rx,
            rest_enabled,
            // ... other fields ...
        }
    }
}

impl eframe::App for PlayaApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // Handle REST commands (non-blocking)
        self.handle_rest_commands();
        
        // ... rest of update() ...
    }
}

impl PlayaApp {
    fn handle_rest_commands(&mut self) {
        // Process all pending commands (non-blocking)
        while let Ok(cmd) = self.rest_command_rx.try_recv() {
            match cmd {
                // Fire-and-forget commands
                RestCommand::Play => {
                    self.player.play();
                }
                RestCommand::Pause => {
                    self.player.pause();
                }
                RestCommand::Stop => {
                    self.player.stop();
                    self.event_bus.emit(SetFrameEvent(0));
                }
                RestCommand::Toggle => {
                    if self.player.is_playing() {
                        self.player.pause();
                    } else {
                        self.player.play();
                    }
                }
                RestCommand::SetFrame(f) => {
                    self.event_bus.emit(SetFrameEvent(f));
                }
                RestCommand::Step(count) => {
                    self.player.step(count, &self.project);
                }
                RestCommand::Jog(dir) => {
                    if dir > 0 {
                        self.player.jog_forward();
                    } else {
                        self.player.jog_backward();
                    }
                }
                
                // Query commands (need response)
                RestCommand::GetStatus(tx) => {
                    let status = self.build_player_status();
                    let _ = tx.send(status);
                }
                RestCommand::GetFrame(tx) => {
                    let _ = tx.send(self.player.current_frame());
                }
                RestCommand::GetMedia(tx) => {
                    let media = self.build_media_list();
                    let _ = tx.send(media);
                }
                RestCommand::GetComps(tx) => {
                    let comps = self.build_comp_list();
                    let _ = tx.send(comps);
                }
                RestCommand::GetCompDetails(uuid, tx) => {
                    let details = self.build_comp_details(uuid);
                    let _ = tx.send(details);
                }
                RestCommand::RenderCurrentFrame(tx) => {
                    let png = self.render_current_frame_png();
                    let _ = tx.send(png);
                }
            }
        }
    }
    
    fn build_player_status(&self) -> PlayerStatus {
        let comp_info = self.player.active_comp()
            .and_then(|uuid| self.project.get_comp(uuid))
            .map(|comp| CompInfo {
                uuid: comp.uuid(),
                name: comp.name().to_string(),
                width: comp.width(),
                height: comp.height(),
                fps: comp.fps(),
                duration: comp.duration(),
            });
        
        PlayerStatus {
            playing: self.player.is_playing(),
            frame: self.player.current_frame(),
            fps: self.player.fps_play(),
            loop_enabled: self.player.loop_enabled(),
            range: self.player.play_range(),
            comp: comp_info,
        }
    }
    
    fn build_media_list(&self) -> Vec<MediaInfo> {
        self.project.media.read().unwrap()
            .values()
            .map(|node| MediaInfo {
                uuid: node.uuid(),
                name: node.name().to_string(),
                node_type: node.node_type().to_string(),
                duration: node.duration(),
                width: node.width(),
                height: node.height(),
            })
            .collect()
    }
    
    fn build_comp_list(&self) -> Vec<CompInfo> {
        self.project.media.read().unwrap()
            .values()
            .filter_map(|node| node.as_comp())
            .map(|comp| CompInfo {
                uuid: comp.uuid(),
                name: comp.name().to_string(),
                width: comp.width(),
                height: comp.height(),
                fps: comp.fps(),
                duration: comp.duration(),
            })
            .collect()
    }
    
    fn build_comp_details(&self, uuid: Uuid) -> Option<CompDetails> {
        let media = self.project.media.read().unwrap();
        let comp = media.get(&uuid)?.as_comp()?;
        
        Some(CompDetails {
            info: CompInfo {
                uuid: comp.uuid(),
                name: comp.name().to_string(),
                width: comp.width(),
                height: comp.height(),
                fps: comp.fps(),
                duration: comp.duration(),
            },
            layers: comp.layers.iter().map(|l| LayerInfo {
                uuid: l.uuid(),
                name: l.name().to_string(),
                source_uuid: l.source_uuid(),
                in_frame: l.in_frame(),
                out_frame: l.out_frame(),
                visible: l.is_visible(),
                solo: l.is_solo(),
                opacity: l.opacity(),
            }).collect(),
        })
    }
    
    fn render_current_frame_png(&self) -> Option<Vec<u8>> {
        // TODO: Get current frame from cache and encode as PNG
        // let frame = self.get_current_rendered_frame()?;
        // frame.to_png_bytes()
        None
    }
}
```

### Step 4: Settings UI

```rust
// src/core/settings.rs - добавить секцию

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RestSettings {
    pub enabled: bool,
    pub port: u16,
    pub allow_remote: bool,  // false = 127.0.0.1 only
}

impl Default for RestSettings {
    fn default() -> Self {
        Self {
            enabled: false,  // Disabled by default for security
            port: 8080,
            allow_remote: false,
        }
    }
}

// В UI настроек добавить:
// [ ] Enable REST API server
// Port: [8080]
// [ ] Allow remote connections (dangerous!)
```

## Testing

### curl Examples

```bash
# Start playback
curl -X POST http://localhost:8080/api/v1/play

# Pause
curl -X POST http://localhost:8080/api/v1/pause

# Toggle
curl -X POST http://localhost:8080/api/v1/toggle

# Get status
curl http://localhost:8080/api/v1/status
# {"playing":true,"frame":42,"fps":24.0,"loop":true,"range":[0,100],"comp":{...}}

# Seek to frame 100
curl -X POST "http://localhost:8080/api/v1/frame?frame=100"

# Step forward 5 frames
curl -X POST "http://localhost:8080/api/v1/step?count=5"

# Step backward 1 frame
curl -X POST "http://localhost:8080/api/v1/step?count=-1"

# List all media
curl http://localhost:8080/api/v1/media
# [{"uuid":"...","name":"footage.mov","type":"File","duration":120},...]

# List compositions
curl http://localhost:8080/api/v1/comps
# [{"uuid":"...","name":"Main","width":1920,"height":1080,"fps":24.0},...]
```

### Python Client Example

```python
import requests

class PlayaClient:
    def __init__(self, host="localhost", port=8080):
        self.base = f"http://{host}:{port}/api/v1"
    
    def play(self):
        return requests.post(f"{self.base}/play").json()
    
    def pause(self):
        return requests.post(f"{self.base}/pause").json()
    
    def stop(self):
        return requests.post(f"{self.base}/stop").json()
    
    def toggle(self):
        return requests.post(f"{self.base}/toggle").json()
    
    def status(self):
        return requests.get(f"{self.base}/status").json()
    
    def frame(self):
        return requests.get(f"{self.base}/frame").json()["frame"]
    
    def seek(self, frame):
        return requests.post(f"{self.base}/frame", params={"frame": frame}).json()
    
    def step(self, count=1):
        return requests.post(f"{self.base}/step", params={"count": count}).json()
    
    def media(self):
        return requests.get(f"{self.base}/media").json()
    
    def comps(self):
        return requests.get(f"{self.base}/comps").json()

# Usage:
client = PlayaClient()
client.play()
print(client.status())
client.seek(50)
client.pause()
```

## Security

1. **Default: localhost only** - `127.0.0.1:8080`, не `0.0.0.0`
2. **Disabled by default** - пользователь должен включить в настройках
3. **CORS headers** - для browser clients
4. **No auth for localhost** - доверяем локальным приложениям
5. **Rate limiting** (future) - защита от спама
6. **Read-only mode** (future) - запретить управление, только status

## Implementation Checklist

- [ ] Add `tiny_http` to Cargo.toml
- [ ] Create `src/core/rest/mod.rs`
- [ ] Create `src/core/rest/commands.rs` - RestCommand enum
- [ ] Create `src/core/rest/server.rs` - HTTP server
- [ ] Integrate in main.rs - spawn thread, handle commands
- [ ] Add REST settings to Settings struct
- [ ] Add REST settings UI panel
- [ ] Test basic endpoints with curl
- [ ] Add logging for requests
- [ ] Document API in README

## Future Enhancements

- WebSocket support for real-time updates (frame changes, playback state)
- Authentication for remote access
- Render queue management via API
- Project open/save/export via API
- Layer manipulation (visibility, transforms)
- Keyframe editing via API
