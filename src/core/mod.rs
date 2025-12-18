//! Core engine modules - cache, events, player, workers
//!
//! These modules form the playback engine, independent of UI.

pub mod cache_man;
pub mod debounced_preloader;
pub mod event_bus;
pub mod global_cache;
pub mod player;
pub mod player_events;
pub mod workers;

// Re-exports for convenience
pub use cache_man::{CacheManager, PreloadStrategy};
pub use debounced_preloader::DebouncedPreloader;
pub use event_bus::EventBus;
pub use global_cache::{CacheStats, GlobalFrameCache};
// CacheStrategy moved to entities::traits for dependency inversion
pub use player::Player;
pub use workers::Workers;
