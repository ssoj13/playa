//! PLAYA - Image sequence player library
//!
//! Re-exports all modules for use by binary targets.

// Clippy: allow complex signatures in compositor/timeline (refactoring TODO)
#![allow(clippy::too_many_arguments)]
#![allow(clippy::module_inception)]

// Core engine (cache, events, player, workers)
pub mod core;

// Main application (PlayaApp struct and related)
pub mod app;

// Application runner (entry point for CLI and Python)
pub mod runner;
pub use runner::run_app;

// App modules
pub mod cli;
pub mod config;
pub mod dialogs;
pub mod entities;
pub mod help;
pub mod main_events;
pub mod server;
pub mod shell;
pub mod ui;
pub mod utils;
pub mod widgets;

// Re-export commonly used types from core
pub use core::cache_man::CacheManager;
pub use core::event_bus::{downcast_event, BoxedEvent, CompEventEmitter, EventBus, EventEmitter};
pub use core::global_cache::GlobalFrameCache;
pub use entities::CacheStrategy;
pub use core::player::Player;

// Re-export entities
pub use entities::{Attrs, AttrValue, Comp, Frame, Project};
