//! Re-export event bus from `playa-events` (single messaging layer).

pub use playa_events::{
    BoxedEvent, CompEventEmitter, Event, EventBus, EventEmitter, downcast_event,
};
