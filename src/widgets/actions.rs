//! Shared action queue for widgets that emit events.

use crate::core::event_bus::{BoxedEvent, Event};

/// Widget actions result - all actions via events.
#[derive(Default)]
pub struct ActionQueue {
    pub hovered: bool,
    pub events: Vec<BoxedEvent>,
}

impl ActionQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push event to be dispatched.
    pub fn send<E: Event>(&mut self, event: E) {
        self.events.push(Box::new(event));
    }
}
