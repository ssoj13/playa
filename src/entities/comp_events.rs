//! Composition events.

use uuid::Uuid;

// === Comp State Events ===

#[derive(Clone, Debug)]
pub struct CurrentFrameChangedEvent {
    pub comp_uuid: Uuid,
    pub old_frame: i32,
    pub new_frame: i32,
}

#[derive(Clone, Debug)]
pub struct LayersChangedEvent(pub Uuid);

#[derive(Clone, Debug)]
pub struct TimelineChangedEvent(pub Uuid);

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
    pub layer_uuid: Uuid,
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
