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

pub use playa_ui::{dialogs, help, ui, widgets};

pub use playa_app::cli;
pub use playa_app::config;
pub use playa_app::run_app;
pub use playa_app::{app, main_events, runner, server, shell};

// Re-export commonly used types from core
pub use core::cache_man::CacheManager;
pub use core::event_bus::{BoxedEvent, CompEventEmitter, EventBus, EventEmitter, downcast_event};
pub use core::global_cache::GlobalFrameCache;
pub use core::player::Player;
pub use entities::CacheStrategy;

// Re-export entities
pub use entities::{AttrValue, Attrs, Comp, Frame, Project};
