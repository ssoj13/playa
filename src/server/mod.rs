//! REST API server for remote control of the player.
//!
//! Runs in a background thread and communicates with main loop via channels.
//! Provides endpoints for status queries and player control.

mod api;

pub use api::{ApiCommand, ApiServer, SharedApiState, PlayerSnapshot, CompSnapshot};
