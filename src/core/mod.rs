//! Core engine modules - cache, events, player, workers
//!
//! These modules form the playback engine, independent of UI.

pub mod cache_man;
pub mod event_bus;
pub mod global_cache;
pub mod player;
pub mod player_events;
pub mod project_events;
pub mod workers;

// Re-exports for convenience
pub use cache_man::{CacheManager, PreloadStrategy};
pub use event_bus::EventBus;
pub use global_cache::{CacheStats, CacheStrategy, GlobalFrameCache};
pub use player::Player;
pub use workers::Workers;
