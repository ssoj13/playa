//! PLAYA - Image sequence player library
//!
//! Re-exports all modules for use by binary targets.

// Core engine (cache, events, player, workers)
pub mod core;

// App modules
pub mod cli;
pub mod config;
pub mod dialogs;
pub mod entities;
pub mod help;
pub mod main_events;
pub mod shell;
pub mod ui;
pub mod utils;
pub mod widgets;

// Re-export commonly used types from core
pub use core::cache_man::CacheManager;
pub use core::event_bus::{downcast_event, BoxedEvent, CompEventEmitter, EventBus, EventEmitter};
pub use core::global_cache::{CacheStrategy, GlobalFrameCache};
pub use core::player::Player;

// Re-export entities
pub use entities::{Attrs, AttrValue, Comp, Frame, Project};
