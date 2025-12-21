//! REST API implementation using rouille.
//!
//! # Purpose
//!
//! Core implementation of the HTTP REST API server. Handles incoming requests,
//! reads shared state for GET endpoints, and sends commands via channel for
//! POST endpoints that modify player state.
//!
//! # Key types
//!
//! - [`ApiServer`] - HTTP server runner, spawns background thread
//! - [`ApiCommand`] - enum of commands sent to main thread (Play, Pause, SetFrame, etc.)
//! - [`SharedApiState`] - thread-safe snapshots (player, comp, cache) updated by main thread
//! - [`PlayerSnapshot`], [`CompSnapshot`], [`CacheSnapshot`] - JSON-serializable state copies
//!
//! # Thread safety
//!
//! - `SharedApiState` uses `RwLock` for each field - main thread writes, HTTP handlers read
//! - `ApiCommand` sent via `mpsc::Sender` - thread-safe, non-blocking
//! - CORS headers added to all responses for browser access
//!
//! # Used by
//!
//! - `server/mod.rs` - re-exports public types
//! - `main.rs` - calls `ApiServer::start()`, receives `ApiCommand` via channel

use crossbeam_channel as crossbeam;
use eframe::egui;
use rouille::{Request, Response};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock, mpsc};
use std::thread;
use std::time::Duration;
use uuid::Uuid;

/// Commands sent from API handlers to main thread
#[derive(Debug)]
pub enum ApiCommand {
    /// Start playback
    Play,
    /// Pause playback
    Pause,
    /// Stop playback (pause + seek to start)
    Stop,
    /// Seek to specific frame
    SetFrame(i32),
    /// Set playback FPS
    SetFps(f32),
    /// Toggle loop mode
    ToggleLoop,
    /// Load sequence from path
    LoadSequence(String),
    /// Emit arbitrary event by name (JSON payload)
    EmitEvent { event_type: String, payload: String },
    /// Take screenshot of current frame, returns PNG bytes
    /// Exit the application
    Exit,
    /// Go to next frame
    NextFrame,
    /// Go to previous frame
    PrevFrame,
    Screenshot {
        /// If true, capture viewport render; if false, capture raw frame
        viewport_only: bool,
        /// Channel to send PNG bytes back
        response: crossbeam::Sender<Result<Vec<u8>, String>>,
    },
}

/// Player state snapshot for API responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerSnapshot {
    pub frame: i32,
    pub fps: f32,
    pub playing: bool,
    pub loop_enabled: bool,
    pub active_comp: Option<Uuid>,
}

/// Comp state snapshot for API responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompSnapshot {
    pub uuid: Uuid,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub duration: i32,
    pub in_frame: i32,
    pub out_frame: i32,
}

/// Cache stats for API responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheSnapshot {
    pub memory_used_mb: f32,
    pub memory_limit_mb: f32,
}

/// Full status response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusResponse {
    pub player: PlayerSnapshot,
    pub comp: Option<CompSnapshot>,
    pub cache: CacheSnapshot,
}

/// Shared state readable by API handlers (updated by main thread)
pub struct SharedApiState {
    pub player: RwLock<PlayerSnapshot>,
    pub comp: RwLock<Option<CompSnapshot>>,
    pub cache: RwLock<CacheSnapshot>,
    /// egui context for triggering immediate repaint (set lazily from main thread)
    pub egui_ctx: RwLock<Option<egui::Context>>,
}

impl Default for SharedApiState {
    fn default() -> Self {
        Self {
            player: RwLock::new(PlayerSnapshot {
                frame: 0,
                fps: 24.0,
                playing: false,
                loop_enabled: false,
                active_comp: None,
            }),
            comp: RwLock::new(None),
            cache: RwLock::new(CacheSnapshot {
                memory_used_mb: 0.0,
                memory_limit_mb: 0.0,
            }),
            egui_ctx: RwLock::new(None),
        }
    }
}

/// Request body for loading sequences
#[derive(Debug, Deserialize)]
struct LoadRequest {
    path: String,
}

/// Request body for emitting events
#[derive(Debug, Deserialize)]
struct EventRequest {
    event_type: String,
    #[serde(default)]
    payload: serde_json::Value,
}

/// Generic API response
#[derive(Serialize)]
struct ApiResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl ApiResponse {
    fn ok() -> Self {
        Self { success: true, message: None, error: None }
    }

    fn ok_msg(msg: &str) -> Self {
        Self { success: true, message: Some(msg.to_string()), error: None }
    }

    fn err(msg: &str) -> Self {
        Self { success: false, message: None, error: Some(msg.to_string()) }
    }
}

/// REST API server
pub struct ApiServer {
    port: u16,
    state: Arc<SharedApiState>,
    command_tx: mpsc::Sender<ApiCommand>,
}

impl ApiServer {
    /// Start the API server in a background thread.
    /// Returns the command receiver for the main thread to poll.
    pub fn start(port: u16, state: Arc<SharedApiState>) -> mpsc::Receiver<ApiCommand> {
        let (tx, rx) = mpsc::channel();

        let server = ApiServer {
            port,
            state,
            command_tx: tx,
        };

        thread::spawn(move || {
            server.run();
        });

        rx
    }

    fn run(self) {
        let addr = format!("0.0.0.0:{}", self.port);
        log::info!("API server starting on http://{}", addr);

        let state = self.state;
        let tx = self.command_tx;

        // Use Server::new for graceful error handling instead of start_server which panics
        match rouille::Server::new(&addr, move |request| {
            Self::handle_request(request, &state, &tx)
        }) {
            Ok(server) => {
                log::info!("API server listening on http://{}", addr);
                server.run();
            }
            Err(e) => {
                log::error!("Failed to start API server on port {}: {}", self.port, e);
                log::error!("This may be caused by another instance of playa already running.");
                log::error!("API server will not be available in this session.");
            }
        }
    }

    fn handle_request(
        request: &Request,
        state: &Arc<SharedApiState>,
        tx: &mpsc::Sender<ApiCommand>,
    ) -> Response {
        // CORS headers added to responses below

        // Handle preflight
        if request.method() == "OPTIONS" {
            return Response::empty_204().with_additional_header("Access-Control-Allow-Origin", "*")
                .with_additional_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
                .with_additional_header("Access-Control-Allow-Headers", "Content-Type");
        }

        // Handle paths with parameters manually (router! doesn't capture well)
        let path = request.url();
        if request.method() == "POST" {
            // /api/player/frame/{n}
            if let Some(frame_str) = path.strip_prefix("/api/player/frame/") {
                if let Ok(frame) = frame_str.parse::<i32>() {
                    return Self::send_command(tx, ApiCommand::SetFrame(frame))
                        .with_additional_header("Access-Control-Allow-Origin", "*");
                } else {
                    return Response::json(&ApiResponse::err("Invalid frame number"))
                        .with_status_code(400)
                        .with_additional_header("Access-Control-Allow-Origin", "*");
                }
            }
            // /api/player/fps/{n}
            if let Some(fps_str) = path.strip_prefix("/api/player/fps/") {
                if let Ok(fps) = fps_str.parse::<f32>() {
                    return Self::send_command(tx, ApiCommand::SetFps(fps))
                        .with_additional_header("Access-Control-Allow-Origin", "*");
                } else {
                    return Response::json(&ApiResponse::err("Invalid FPS value"))
                        .with_status_code(400)
                        .with_additional_header("Access-Control-Allow-Origin", "*");
                }
            }
        }

        let response = rouille::router!(request,
            // Status endpoints
            (GET) ["/api/status"] => {
                Self::get_status(state)
            },
            (GET) ["/api/player"] => {
                Self::get_player(state)
            },
            (GET) ["/api/comp"] => {
                Self::get_comp(state)
            },
            (GET) ["/api/cache"] => {
                Self::get_cache(state)
            },

            // Player control
            (POST) ["/api/player/play"] => {
                Self::send_command(tx, ApiCommand::Play)
            },
            (POST) ["/api/player/pause"] => {
                Self::send_command(tx, ApiCommand::Pause)
            },
            (POST) ["/api/player/stop"] => {
                Self::send_command(tx, ApiCommand::Stop)
            },
            (POST) ["/api/player/toggle-loop"] => {
                Self::send_command(tx, ApiCommand::ToggleLoop)
            },
            // Frame/FPS handled separately due to path params
            (POST) ["/api/player/next"] => {
                Self::send_command(tx, ApiCommand::NextFrame)
            },
            (POST) ["/api/player/prev"] => {
                Self::send_command(tx, ApiCommand::PrevFrame)
            },
            (POST) ["/api/app/exit"] => {
                Self::send_command(tx, ApiCommand::Exit)
            },

            (POST) ["/api/player/frame"] => {
                // Fallback - requires /api/player/frame/{n}
                Response::json(&ApiResponse::err("Missing frame number")).with_status_code(400)
            },

            // Project control
            (POST) ["/api/project/load"] => {
                Self::handle_load(request, tx)
            },

            // Generic event emission
            (POST) ["/api/event"] => {
                Self::handle_event(request, tx)
            },

            // Health check
            (GET) ["/api/health"] => {
                Response::json(&ApiResponse::ok_msg("playa API server"))
            },

            // Screenshot endpoints
            // /api/screenshot - full window capture via glReadPixels
            // /api/screenshot/frame - raw frame data only (no UI)
            (GET) ["/api/screenshot"] => {
                Self::handle_screenshot(tx, state, true)  // viewport_only=true means full window
            },
            (GET) ["/api/screenshot/frame"] => {
                Self::handle_screenshot(tx, state, false)  // viewport_only=false means raw frame
            },

            // Fallback
            _ => {
                Response::json(&ApiResponse::err("Not found")).with_status_code(404)
            }
        );

        // Add CORS headers to response
        response.with_additional_header("Access-Control-Allow-Origin", "*")
    }

    fn get_status(state: &Arc<SharedApiState>) -> Response {
        let player = state.player.read().unwrap().clone();
        let comp = state.comp.read().unwrap().clone();
        let cache = state.cache.read().unwrap().clone();

        Response::json(&StatusResponse { player, comp, cache })
    }

    fn get_player(state: &Arc<SharedApiState>) -> Response {
        let player = state.player.read().unwrap().clone();
        Response::json(&player)
    }

    fn get_comp(state: &Arc<SharedApiState>) -> Response {
        let comp = state.comp.read().unwrap().clone();
        match comp {
            Some(c) => Response::json(&c),
            None => Response::json(&ApiResponse::err("No active comp")).with_status_code(404),
        }
    }

    fn get_cache(state: &Arc<SharedApiState>) -> Response {
        let cache = state.cache.read().unwrap().clone();
        Response::json(&cache)
    }

    fn send_command(tx: &mpsc::Sender<ApiCommand>, cmd: ApiCommand) -> Response {
        match tx.send(cmd) {
            Ok(_) => Response::json(&ApiResponse::ok()),
            Err(e) => Response::json(&ApiResponse::err(&format!("Failed to send command: {}", e)))
                .with_status_code(500),
        }
    }

    fn handle_load(request: &Request, tx: &mpsc::Sender<ApiCommand>) -> Response {
        match rouille::input::json_input::<LoadRequest>(request) {
            Ok(req) => Self::send_command(tx, ApiCommand::LoadSequence(req.path)),
            Err(e) => Response::json(&ApiResponse::err(&format!("Invalid JSON: {}", e)))
                .with_status_code(400),
        }
    }

    fn handle_event(request: &Request, tx: &mpsc::Sender<ApiCommand>) -> Response {
        match rouille::input::json_input::<EventRequest>(request) {
            Ok(req) => {
                let payload = serde_json::to_string(&req.payload).unwrap_or_default();
                Self::send_command(tx, ApiCommand::EmitEvent {
                    event_type: req.event_type,
                    payload,
                })
            }
            Err(e) => Response::json(&ApiResponse::err(&format!("Invalid JSON: {}", e)))
                .with_status_code(400),
        }
    }

    /// Handle screenshot request - sends command and waits for JPEG response
    fn handle_screenshot(tx: &mpsc::Sender<ApiCommand>, state: &SharedApiState, viewport_only: bool) -> Response {
        // Trigger immediate repaint to minimize wait time
        if let Some(ctx) = state.egui_ctx.read().unwrap().as_ref() {
            ctx.request_repaint();
        }

        // Create oneshot channel for response
        let (resp_tx, resp_rx) = crossbeam::bounded(1);

        // Send screenshot command
        let cmd = ApiCommand::Screenshot {
            viewport_only,
            response: resp_tx,
        };

        if let Err(e) = tx.send(cmd) {
            return Response::json(&ApiResponse::err(&format!("Failed to send command: {}", e)))
                .with_status_code(500);
        }

        // Request repaint again after command is queued
        if let Some(ctx) = state.egui_ctx.read().unwrap().as_ref() {
            ctx.request_repaint();
        }

        // Wait for response with timeout
        match resp_rx.recv_timeout(Duration::from_secs(15)) {
            Ok(Ok(jpeg_bytes)) => {
                Response::from_data("image/jpeg", jpeg_bytes)
            }
            Ok(Err(err)) => {
                Response::json(&ApiResponse::err(&err)).with_status_code(500)
            }
            Err(_) => {
                Response::json(&ApiResponse::err("Screenshot timeout")).with_status_code(504)
            }
        }
    }
}
