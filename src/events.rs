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
    /// Toggle play/pause
    TogglePlayPause,
    /// Stop playback and reset to start
    Stop,
    /// Set current frame to specific value
    SetFrame(i32),
    /// Step forward one frame
    StepForward,
    /// Step backward one frame
    StepBackward,
    /// Step forward 25 frames
    StepForwardLarge,
    /// Step backward 25 frames
    StepBackwardLarge,
    /// Jump to start of playback range
    JumpToStart,
    /// Jump to end of playback range
    JumpToEnd,
    /// Jump to previous layer edge (timeline)
    JumpToPrevEdge,
    /// Jump to next layer edge (timeline)
    JumpToNextEdge,
    /// Previous clip
    PreviousClip,
    /// Next clip
    NextClip,

    // ===== Project Management =====
    /// Add clip from file path
    AddClip(PathBuf),
    /// Add multiple clips from paths
    AddClips(Vec<PathBuf>),
    /// Add new composition with specified parameters
    AddComp { name: String, fps: f32 },
    /// Remove media (clip or comp) by UUID
    RemoveMedia(String),
    /// Remove all currently selected media items
    RemoveSelectedMedia,
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
    DragDrop { target_comp: String, frame: usize },
    /// Cancel drag operation
    DragCancel,

    // ===== Layer Operations =====
    /// Add layer to composition
    AddLayer {
        comp_uuid: String,
        source_uuid: String,
        start_frame: i32,
        target_row: Option<usize>, // Optional target visual row for insertion
    },
    /// Remove layer from composition
    RemoveLayer { comp_uuid: String, layer_idx: usize },
    /// Move layer to new start position
    MoveLayer {
        comp_uuid: String,
        layer_idx: usize,
        new_start: i32,
    },
    /// Reorder layer vertically within a composition
    ReorderLayer {
        comp_uuid: String,
        from_idx: usize,
        to_idx: usize,
    },
    /// Move and reorder layer in one op (drag)
    MoveAndReorderLayer {
        comp_uuid: String,
        layer_idx: usize,
        new_start: i32,
        new_idx: usize,
    },
    /// Adjust layer play start (trim in)
    SetLayerPlayStart {
        comp_uuid: String,
        layer_idx: usize,
        new_play_start: i32,
    },
    /// Adjust layer play end (trim out)
    SetLayerPlayEnd {
        comp_uuid: String,
        layer_idx: usize,
        new_play_end: i32,
    },
    /// Remove selected layer
    RemoveSelectedLayer,

    // ===== Selection =====
    /// Select media item by UUID
    SelectMedia(String),
    /// Set project selection (ordered UUIDs) with optional anchor index (runtime)
    ProjectSelectionChanged {
        selection: Vec<String>,
        anchor: Option<usize>,
    },
    /// Set active project item (UUID)
    ProjectActiveChanged(String),
    /// Select layer in timeline
    CompSelectionChanged {
        comp_uuid: String,
        selection: Vec<usize>,
        anchor: Option<usize>,
    },

    // ===== UI State =====
    /// Toggle playlist panel visibility
    TogglePlaylist,
    /// Toggle help overlay visibility
    ToggleHelp,
    /// Toggle attribute editor panel visibility
    ToggleAttributeEditor,
    /// Toggle settings dialog
    ToggleSettings,
    /// Toggle encode dialog
    ToggleEncodeDialog,
    /// Toggle fullscreen
    ToggleFullscreen,
    /// Toggle loop mode
    ToggleLoop,
    /// Toggle frame numbers display
    ToggleFrameNumbers,
    /// Timeline zoom changed (Right/Timeline)
    TimelineZoomChanged(f32),
    /// Timeline pan offset changed (frames)
    TimelinePanChanged(f32),
    /// Timeline snap toggle
    TimelineSnapChanged(bool),
    /// Timeline work area lock toggled
    TimelineLockWorkAreaChanged(bool),
    /// Fit all clips in timeline to view (canvas_width in pixels)
    TimelineFitAll(f32),
    /// Fit timeline using last known canvas width
    TimelineFit,
    /// Reset timeline zoom to 1.0
    TimelineResetZoom,
    /// Zoom viewport by factor
    ZoomViewport(f32),
    /// Reset viewport zoom to fit
    ResetViewport,
    /// Fit viewport to frame
    FitViewport,
    /// Set viewport to 100% zoom
    Viewport100,

    // ===== Play Range Control =====
    /// Set play range start at current frame
    SetPlayRangeStart,
    /// Set play range end at current frame
    SetPlayRangeEnd,
    /// Reset play range to full
    ResetPlayRange,
    /// Set comp play area start to absolute frame
    SetCompPlayStart { comp_uuid: String, frame: i32 },
    /// Set comp play area end to absolute frame
    SetCompPlayEnd { comp_uuid: String, frame: i32 },
    /// Reset comp play area to full
    ResetCompPlayArea { comp_uuid: String },

    // ===== FPS Control =====
    /// Increase base FPS
    IncreaseFPS,
    /// Decrease base FPS
    DecreaseFPS,
}

/// Window context for hotkeys
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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

/// Composition-level events emitted by Comp instances.
/// These events notify the app about changes within a composition.
#[derive(Debug, Clone)]
pub enum CompEvent {
    /// Current frame changed in a composition
    CurrentFrameChanged {
        comp_uuid: String,
        old_frame: i32,
        new_frame: i32,
    },
    /// Layers were modified (added, removed, reordered)
    LayersChanged { comp_uuid: String },
    /// Timeline settings changed (play range, etc.)
    TimelineChanged { comp_uuid: String },
}

/// Event sender for Comp to emit CompEvents.
/// Wraps a channel sender for type safety.
#[derive(Clone, Debug)]
pub struct CompEventSender {
    tx: Option<crossbeam::channel::Sender<CompEvent>>,
}

impl CompEventSender {
    /// Create a new CompEventSender
    pub fn new(tx: crossbeam::channel::Sender<CompEvent>) -> Self {
        Self { tx: Some(tx) }
    }

    /// Create a dummy sender (for initialization, before event system is set up)
    pub fn dummy() -> Self {
        Self { tx: None }
    }

    /// Emit a CompEvent
    pub fn emit(&self, event: CompEvent) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(event);
        }
    }
}

impl Default for CompEventSender {
    fn default() -> Self {
        Self::dummy()
    }
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
