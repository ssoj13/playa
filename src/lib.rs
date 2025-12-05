//! PLAYA - Image sequence player library
//!
//! Re-exports all modules for use by binary targets.

pub mod shell;
pub mod cache_man;
pub mod cli;
pub mod config;
pub mod dialogs;
pub mod entities;
pub mod event_bus;
pub mod global_cache;
pub mod main_events;
pub mod player;
pub mod player_events;
pub mod project_events;
pub mod ui;
pub mod utils;
pub mod widgets;
pub mod workers;

// Re-export commonly used types
pub use cache_man::CacheManager;
pub use entities::{Project, Comp, Frame, Attrs, AttrValue};
pub use event_bus::{EventBus, EventEmitter, CompEventEmitter, BoxedEvent, downcast_event};
pub use global_cache::{GlobalFrameCache, CacheStrategy};
pub use player::Player;
