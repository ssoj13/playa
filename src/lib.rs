//! PLAYA - Image sequence player library
//!
//! Re-exports all modules for use by binary targets.

// Clippy: allow complex signatures in compositor/timeline (refactoring TODO)
#![allow(clippy::too_many_arguments)]
#![allow(clippy::module_inception)]

// Engine: cache, events, player, workers, entities, loaders, compositor
pub use playa_engine::{core, defaults, entities, utils};
/// Typed events + [`playa_events::bus::EventBus`] (single cross-layer messaging path).
pub use playa_events;

// Main application (PlayaApp struct and related)
pub mod app;

// Application runner (entry point for CLI and Python)
pub mod runner;
pub use runner::run_app;

// App modules
pub mod cli;
pub mod config;
pub mod main_events;
pub mod server;
pub mod shell;

/// UI (widgets, dialogs, help, composition) — extracted to the `playa-ui` crate.
pub use playa_ui::{dialogs, help, ui, widgets};

// Re-export commonly used types from core
pub use core::cache_man::CacheManager;
pub use core::event_bus::{downcast_event, BoxedEvent, CompEventEmitter, EventBus, EventEmitter};
pub use core::global_cache::GlobalFrameCache;
pub use entities::CacheStrategy;
pub use core::player::Player;

// Re-export entities
pub use entities::{Attrs, AttrValue, Comp, Frame, Project};
