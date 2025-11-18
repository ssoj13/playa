// Event-driven architecture for Playa
//
// This module implements a message bus pattern using crossbeam channels.
// All UI interactions, keyboard shortcuts, and system events are converted
// to AppEvent messages and dispatched through the EventBus.

use std::path::PathBuf;

/// Main application event enum.
/// All user interactions, keyboard shortcuts, and system events are represented as AppEvents.
#[derive(Debug, Clone)]
pub enum AppEvent {
    // ===== Playback Control =====
    /// Start playback
    Play,
    /// Pause playback
    Pause,
    /// Stop playback and reset to start
    Stop,
    /// Set current frame to specific value
    SetFrame(usize),
    /// Step forward one frame
    StepForward,
    /// Step backward one frame
    StepBackward,
    /// Jump to start of playback range
    JumpToStart,
    /// Jump to end of playback range
    JumpToEnd,

    // ===== Project Management =====
    /// Add clip from file path
    AddClip(PathBuf),
    /// Add multiple clips from paths
    AddClips(Vec<PathBuf>),
    /// Add new composition with specified parameters
    AddComp { name: String, fps: f32 },
    /// Remove media (clip or comp) by UUID
    RemoveMedia(String),
    /// Save project to file
    SaveProject(PathBuf),
    /// Load project from file
    LoadProject(PathBuf),

    // ===== Timeline / Drag-and-Drop =====
    /// Start dragging media item
    DragStart { media_uuid: String },
    /// Update drag position
    DragMove { mouse_pos: (f32, f32) },
    /// Drop media at target location
    DragDrop {
        target_comp: String,
        frame: usize,
    },
    /// Cancel drag operation
    DragCancel,

    // ===== Selection =====
    /// Select media item by UUID
    SelectMedia(String),
    /// Select layer in timeline
    SelectLayer(usize),
    /// Deselect all items
    DeselectAll,

    // ===== UI State =====
    /// Toggle playlist panel visibility
    TogglePlaylist,
    /// Toggle help overlay visibility
    ToggleHelp,
    /// Toggle attribute editor panel visibility
    ToggleAttributeEditor,
    /// Zoom viewport by factor
    ZoomViewport(f32),
    /// Reset viewport zoom to fit
    ResetViewport,

    // ===== Keyboard Shortcuts =====
    /// Generic hotkey pressed event with window context
    /// Format: "hotkey.<key>:pressed" with window prefix
    HotkeyPressed {
        key: String,
        window: HotkeyWindow,
    },
    /// Generic hotkey released event
    HotkeyReleased {
        key: String,
        window: HotkeyWindow,
    },
}

/// Window context for hotkeys
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyWindow {
    /// Global hotkeys (app-level)
    Global,
    /// Viewport window
    Viewport,
    /// Timeline window
    Timeline,
    /// Project panel
    Project,
    /// Attribute editor
    AttributeEditor,
}

/// Event bus for message passing between components.
///
/// Uses crossbeam unbounded channels for lock-free, multi-producer multi-consumer messaging.
/// This allows any component to send events and the main app to process them in order.
pub struct EventBus {
    tx: crossbeam::channel::Sender<AppEvent>,
    rx: crossbeam::channel::Receiver<AppEvent>,
}

impl EventBus {
    /// Create a new event bus
    pub fn new() -> Self {
        let (tx, rx) = crossbeam::channel::unbounded();
        Self { tx, rx }
    }

    /// Send an event to the bus (non-blocking)
    pub fn send(&self, event: AppEvent) {
        let _ = self.tx.send(event);
    }

    /// Try to receive an event (non-blocking)
    /// Returns None if no events are available
    pub fn try_recv(&self) -> Option<AppEvent> {
        self.rx.try_recv().ok()
    }

    /// Get a clone of the sender for passing to other components
    pub fn sender(&self) -> crossbeam::channel::Sender<AppEvent> {
        self.tx.clone()
    }

    /// Drain all pending events and return them as a vector
    pub fn drain(&self) -> Vec<AppEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.rx.try_recv() {
            events.push(event);
        }
        events
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_bus_send_receive() {
        let bus = EventBus::new();
        bus.send(AppEvent::Play);
        bus.send(AppEvent::Pause);

        let event1 = bus.try_recv();
        assert!(matches!(event1, Some(AppEvent::Play)));

        let event2 = bus.try_recv();
        assert!(matches!(event2, Some(AppEvent::Pause)));

        let event3 = bus.try_recv();
        assert!(event3.is_none());
    }

    #[test]
    fn test_event_bus_drain() {
        let bus = EventBus::new();
        bus.send(AppEvent::Play);
        bus.send(AppEvent::Pause);
        bus.send(AppEvent::Stop);

        let events = bus.drain();
        assert_eq!(events.len(), 3);
    }

    #[test]
    fn test_sender_clone() {
        let bus = EventBus::new();
        let sender = bus.sender();

        sender.send(AppEvent::Play).unwrap();
        let event = bus.try_recv();
        assert!(matches!(event, Some(AppEvent::Play)));
    }
}
