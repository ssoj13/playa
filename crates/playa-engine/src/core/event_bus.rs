//! Re-export event bus from `playa-events` (single messaging layer).

pub use playa_events::{downcast_event, BoxedEvent, CompEventEmitter, Event, EventBus, EventEmitter};
