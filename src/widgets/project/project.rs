//! Project panel actions and state.

use crate::core::event_bus::BoxedEvent;

/// Project panel result - all actions via events
#[derive(Default)]
pub struct ProjectActions {
    pub hovered: bool,
    pub events: Vec<BoxedEvent>,
}

impl ProjectActions {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push event to be dispatched
    pub fn send<E: crate::core::event_bus::Event>(&mut self, event: E) {
        self.events.push(Box::new(event));
    }
}
