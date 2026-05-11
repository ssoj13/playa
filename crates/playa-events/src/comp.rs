//! Composition / timeline layer events — cache invalidation and UI orchestration.

use serde_json::Value;
use uuid::Uuid;

// === Comp State Events ===

#[derive(Clone, Debug)]
pub struct CurrentFrameChangedEvent {
    pub comp_uuid: Uuid,
    pub old_frame: i32,
    pub new_frame: i32,
}

#[derive(Clone, Debug)]
pub struct LayersChangedEvent {
    pub comp_uuid: Uuid,
    pub affected_range: Option<(i32, i32)>,
}

#[derive(Clone, Debug)]
pub struct AttrsChangedEvent(pub Uuid);

#[derive(Clone, Debug)]
pub struct SetBookmarkEvent {
    pub comp_uuid: Uuid,
    pub slot: u8,
    pub frame: Option<i32>,
}

#[derive(Clone, Debug)]
pub struct JumpToBookmarkEvent {
    pub comp_uuid: Uuid,
    pub slot: u8,
}

// === Layer Operations ===

#[derive(Clone, Debug)]
pub struct AddLayerEvent {
    pub comp_uuid: Uuid,
    pub source_uuid: Uuid,
    pub start_frame: i32,
    pub insert_idx: Option<usize>,
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
    pub layer_uuids: Vec<Uuid>,
    pub visible: bool,
    pub solo: bool,
    pub opacity: f32,
    pub blend_mode: String,
    pub speed: f32,
}

/// User picked / cleared a track-matte source for a layer in the
/// timeline outline. The handler resolves `target_layer_uuid` to a
/// `RefNode` (creating one if needed) and sets the layer's
/// `mask_ref_uuid` attr. `None` clears the mask.
///
/// Channel is fixed to `Alpha` for v1 — finer control (Luma / per-channel)
/// goes through the Attribute Editor on the `RefNode` itself.
#[derive(Clone, Debug)]
pub struct LayerMaskRefChangedEvent {
    pub comp_uuid: Uuid,
    pub layer_uuid: Uuid,
    pub target_layer_uuid: Option<Uuid>,
}

/// Generic layer attribute batch (Attribute Editor).
/// Payload is JSON to keep this crate independent of `AttrValue` in the engine.
#[derive(Clone, Debug)]
pub struct SetLayerAttrsEvent {
    pub comp_uuid: Uuid,
    pub layer_uuids: Vec<Uuid>,
    pub attrs: Vec<(String, Value)>,
}

#[derive(Clone, Debug)]
pub struct SetLayerTransformsEvent {
    pub comp_uuid: Uuid,
    pub updates: Vec<(Uuid, [f32; 3], [f32; 3], [f32; 3])>,
}

#[derive(Clone, Debug)]
pub struct CompSelectionChangedEvent {
    pub comp_uuid: Uuid,
    pub selection: Vec<Uuid>,
    pub anchor: Option<Uuid>,
}

#[derive(Clone, Debug)]
pub struct HoverLayerEvent {
    pub comp_uuid: Uuid,
    pub layer_uuid: Option<Uuid>,
}

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

#[derive(Clone, Debug)]
pub struct DuplicateLayersEvent {
    pub comp_uuid: Uuid,
}

#[derive(Clone, Debug)]
pub struct CopyLayersEvent {
    pub comp_uuid: Uuid,
}

#[derive(Clone, Debug)]
pub struct PasteLayersEvent {
    pub comp_uuid: Uuid,
    pub target_frame: i32,
}

#[derive(Clone, Debug)]
pub struct SelectAllLayersEvent {
    pub comp_uuid: Uuid,
}

#[derive(Clone, Debug)]
pub struct ClearLayerSelectionEvent {
    pub comp_uuid: Uuid,
}

#[derive(Clone, Debug)]
pub struct SlideLayerEvent {
    pub comp_uuid: Uuid,
    pub layer_idx: usize,
    pub new_in: i32,
    pub new_trim_in: i32,
    pub new_trim_out: i32,
}

#[derive(Clone, Debug)]
pub struct ResetTrimsEvent {
    pub comp_uuid: Uuid,
}
