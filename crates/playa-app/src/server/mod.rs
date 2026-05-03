//! REST API server for remote control of the player.
//!
//! # Purpose
//!
//! Provides HTTP REST API for remote control of playa from external tools,
//! scripts, web interfaces, or other applications. Enables automation and
//! integration with pipelines.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────┐       mpsc::channel        ┌──────────────────────┐
//! │   API Server Thread     │  ───── ApiCommand ──────▶  │   Main Thread        │
//! │   (rouille HTTP)        │                            │   (egui loop)        │
//! │                         │                            │                      │
//! │  POST /api/player/play  │  ──▶ ApiCommand::Play ──▶  │  emit(TogglePlay)    │
//! │  POST /api/frame/100    │  ──▶ SetFrame(100) ────▶   │  emit(SetFrameEvent) │
//! └─────────────────────────┘                            └──────────────────────┘
//!          │                                                      │
//!          │  Arc<RwLock<SharedApiState>>                         │
//!          │◀──────────── read snapshots ─────────────────────────│
//!          │                                             updates each frame
//! ```
//!
//! - **rouille** - sync HTTP server (simpler than async axum/tokio)
//! - **mpsc channel** - commands from HTTP handlers to main thread
//! - **SharedApiState** - read-only state snapshots updated by main thread
//!
//! # Dependencies
//!
//! - `rouille` - HTTP server
//! - `serde` / `serde_json` - JSON serialization
//! - `uuid` - comp/node identifiers
//!
//! # Used by
//!
//! - `main.rs` - starts server via `ApiServer::start()`, polls commands in main loop
//! - `prefs.rs` - settings UI for enable/disable and port configuration
//!
//! # Endpoints
//!
//! | Method | Path                    | Description                |
//! |--------|-------------------------|----------------------------|
//! | GET    | `/api/status`           | Full status (player/comp/cache) |
//! | GET    | `/api/player`           | Player state only          |
//! | GET    | `/api/comp`             | Active comp info           |
//! | GET    | `/api/cache`            | Cache memory stats         |
//! | GET    | `/api/health`           | Health check               |
//! | POST   | `/api/player/play`      | Start playback             |
//! | POST   | `/api/player/pause`     | Pause playback             |
//! | POST   | `/api/player/stop`      | Stop (pause + seek to 0)   |
//! | POST   | `/api/player/frame/{n}` | Seek to frame n            |
//! | POST   | `/api/player/fps/{n}`   | Set playback FPS           |
//! | POST   | `/api/player/toggle-loop` | Toggle loop mode         |
//! | POST   | `/api/project/load`     | Load sequence (JSON body)  |
//! | POST   | `/api/event`            | Emit custom event          |

mod api;

pub use api::{ApiCommand, ApiServer, CacheSnapshot, CompSnapshot, PlayerSnapshot, SharedApiState};
