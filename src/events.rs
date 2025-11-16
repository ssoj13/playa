//! Event system for composition changes and frame loading coordination.
//!
//! Events are emitted when significant state changes occur (frame changes, layers modified)
//! and handled by the main application to trigger side effects (loading frames, updating UI).

use crossbeam::channel::Sender;

/// Events related to composition state changes
#[derive(Debug, Clone)]
pub enum CompEvent {
    /// Current frame position changed in a composition
    CurrentFrameChanged {
        comp_uuid: String,
        old_frame: usize,
        new_frame: usize,
    },

    /// Layers were added, removed, or reordered in a composition
    LayersChanged { comp_uuid: String },

    /// Composition timeline changed (start/end/fps)
    TimelineChanged { comp_uuid: String },
}

/// Event sender wrapper for compositions
///
/// Compositions hold this sender to emit events when their state changes.
#[derive(Clone, Debug)]
pub struct CompEventSender {
    sender: Option<Sender<CompEvent>>,
}

impl CompEventSender {
    /// Create event sender (connected to channel)
    pub fn new(sender: Sender<CompEvent>) -> Self {
        Self {
            sender: Some(sender),
        }
    }

    /// Create dummy sender (for tests or when events not needed)
    pub fn dummy() -> Self {
        Self { sender: None }
    }

    /// Emit event (silent if no receiver)
    pub fn emit(&self, event: CompEvent) {
        if let Some(ref tx) = self.sender {
            let _ = tx.send(event); // Ignore send errors (receiver might be dropped)
        }
    }
}

impl Default for CompEventSender {
    fn default() -> Self {
        Self::dummy()
    }
}
