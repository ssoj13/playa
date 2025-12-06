//! Composition events for cache invalidation and UI updates.
//!
//! # Event Hierarchy for Cache Invalidation
//!
//! There are two events that trigger frame cache invalidation:
//!
//! ## [`LayersChangedEvent`]
//! Emitted when layer **structure** changes (add/remove/move/reorder).
//! Has optional `affected_range` to limit cache clearing to specific frames.
//!
//! ## [`AttrsChangedEvent`]
//! Emitted when layer **attributes** change (opacity, blend_mode, transforms, etc.).
//! Clears entire comp cache since any attribute could affect all frames.
//!
//! Both events trigger the same handler in `main.rs` that:
//! 1. Increments cache epoch (cancels pending worker tasks)
//! 2. Clears cached frames
//! 3. Calls `invalidate_cascade()` for parent comps
//!
//! # Emitting Events
//!
//! Use [`Comp::set_child_attr`] or [`Comp::set_child_attrs`] to modify layer
//! attributes - they automatically emit `AttrsChangedEvent`.
//!
//! For structural changes, emit `LayersChangedEvent` directly after modifying
//! the children list.

use uuid::Uuid;

// === Comp State Events ===

/// Emitted when the playhead moves to a different frame.
#[derive(Clone, Debug)]
pub struct CurrentFrameChangedEvent {
    pub comp_uuid: Uuid,
    pub old_frame: i32,
    pub new_frame: i32,
}

/// Emitted when layer structure changes (add/remove/move/reorder).
///
/// Handler in `main.rs` clears affected frame range from cache.
/// If `affected_range` is None, entire comp cache is cleared.
#[derive(Clone, Debug)]
pub struct LayersChangedEvent {
    pub comp_uuid: Uuid,
    /// Optional frame range that was affected (start, end inclusive)
    pub affected_range: Option<(i32, i32)>,
}

/// Emitted when layer or comp attributes change (opacity, blend_mode, etc.).
///
/// Triggers full cache invalidation for the comp since attribute changes
/// can affect any frame. Emitted automatically by [`Comp::set_child_attr`]
/// and [`Comp::set_child_attrs`].
///
/// Handler in `main.rs`:
/// - Increments cache epoch to cancel pending worker tasks
/// - Clears all cached frames for this comp
/// - Triggers `invalidate_cascade()` for parent comps
#[derive(Clone, Debug)]
pub struct AttrsChangedEvent(pub Uuid);

// === Layer Operations ===

#[derive(Clone, Debug)]
pub struct AddLayerEvent {
    pub comp_uuid: Uuid,
    pub source_uuid: Uuid,
    pub start_frame: i32,
    pub target_row: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct RemoveLayerEvent {
    pub comp_uuid: Uuid,
    pub layer_idx: usize,
}

#[derive(Clone, Debug)]
pub struct RemoveSelectedLayerEvent;

#[derive(Clone, Debug)]
pub struct MoveLayerEvent {
    pub comp_uuid: Uuid,
    pub layer_idx: usize,
    pub new_start: i32,
}

#[derive(Clone, Debug)]
pub struct ReorderLayerEvent {
    pub comp_uuid: Uuid,
    pub from_idx: usize,
    pub to_idx: usize,
}

#[derive(Clone, Debug)]
pub struct MoveAndReorderLayerEvent {
    pub comp_uuid: Uuid,
    pub layer_idx: usize,
    pub new_start: i32,
    pub new_idx: usize,
}

#[derive(Clone, Debug)]
pub struct SetLayerPlayStartEvent {
    pub comp_uuid: Uuid,
    pub layer_idx: usize,
    pub new_play_start: i32,
}

#[derive(Clone, Debug)]
pub struct SetLayerPlayEndEvent {
    pub comp_uuid: Uuid,
    pub layer_idx: usize,
    pub new_play_end: i32,
}

#[derive(Clone, Debug)]
pub struct AlignLayersStartEvent(pub Uuid);

#[derive(Clone, Debug)]
pub struct AlignLayersEndEvent(pub Uuid);

#[derive(Clone, Debug)]
pub struct TrimLayersStartEvent(pub Uuid);

#[derive(Clone, Debug)]
pub struct TrimLayersEndEvent(pub Uuid);

#[derive(Clone, Debug)]
pub struct LayerAttributesChangedEvent {
    pub comp_uuid: Uuid,
    pub layer_uuids: Vec<Uuid>,  // Multiple layers for multi-selection support
    pub visible: bool,
    pub opacity: f32,
    pub blend_mode: String,
    pub speed: f32,
}

// === Comp Selection ===

#[derive(Clone, Debug)]
pub struct CompSelectionChangedEvent {
    pub comp_uuid: Uuid,
    pub selection: Vec<Uuid>,
    pub anchor: Option<Uuid>,
}

// === Comp Play Area ===

#[derive(Clone, Debug)]
pub struct SetCompPlayStartEvent {
    pub comp_uuid: Uuid,
    pub frame: i32,
}

#[derive(Clone, Debug)]
pub struct SetCompPlayEndEvent {
    pub comp_uuid: Uuid,
    pub frame: i32,
}

#[derive(Clone, Debug)]
pub struct ResetCompPlayAreaEvent(pub Uuid);

// === Layer Clipboard Operations ===

/// Duplicate selected layers in-place (Ctrl-D)
/// Creates copies of selected layers and inserts them above originals
#[derive(Clone, Debug)]
pub struct DuplicateLayersEvent {
    pub comp_uuid: Uuid,
}

/// Copy selected layers to clipboard (Ctrl-C)
#[derive(Clone, Debug)]
pub struct CopyLayersEvent {
    pub comp_uuid: Uuid,
}

/// Paste layers from clipboard at playhead position (Ctrl-V)
#[derive(Clone, Debug)]
pub struct PasteLayersEvent {
    pub comp_uuid: Uuid,
    pub target_frame: i32,
}
