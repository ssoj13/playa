//! Project panel / media pool events.

use std::path::PathBuf;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct AddClipEvent(pub PathBuf);

#[derive(Clone, Debug)]
pub struct AddClipsEvent(pub Vec<PathBuf>);

#[derive(Clone, Debug)]
pub struct AddFolderEvent(pub PathBuf);

#[derive(Clone, Debug)]
pub struct AddCompEvent {
    pub name: String,
    pub fps: f32,
}

#[derive(Clone, Debug)]
pub struct AddCameraEvent {
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct AddTextEvent {
    pub name: String,
    pub text: String,
}

/// Create an empty `AINode` in the project. The provider defaults to
/// `"seedance.text_to_video"`; the user edits prompt / provider / etc.
/// via the standard Attribute Editor afterwards.
#[derive(Clone, Debug)]
pub struct AddAINodeEvent {
    pub name: String,
    pub provider: String,
}

/// Submit the AINode's current attrs as a new `Generation`. Handler
/// resolves auto-fields (seed), builds the `Generation` record with
/// input_snapshots, calls `queue.submit` with `ainode_uuid` +
/// `gen_uuid` in params, and pushes the Generation onto the AINode's
/// history (making it active).
#[derive(Clone, Debug)]
pub struct GenerateAINodeEvent(pub Uuid);

/// Mark a specific Generation as the active one for compose.
#[derive(Clone, Debug)]
pub struct SetActiveGenerationEvent {
    pub ainode_uuid: Uuid,
    pub gen_uuid: Uuid,
}

/// Remove a Generation from the AINode's history. If it was active,
/// the next-most-recent generation becomes active (or none if it was
/// the last).
#[derive(Clone, Debug)]
pub struct DeleteGenerationEvent {
    pub ainode_uuid: Uuid,
    pub gen_uuid: Uuid,
}

#[derive(Clone, Debug)]
pub struct RemoveMediaEvent(pub Uuid);

#[derive(Clone, Debug)]
pub struct RemoveSelectedMediaEvent;

#[derive(Clone, Debug)]
pub struct ClearAllMediaEvent;

#[derive(Clone, Debug)]
pub struct SaveProjectEvent(pub PathBuf);

#[derive(Clone, Debug)]
pub struct LoadProjectEvent(pub PathBuf);

#[derive(Clone, Debug)]
pub struct QuickSaveEvent;

#[derive(Clone, Debug)]
pub struct OpenProjectDialogEvent;

#[derive(Clone, Debug)]
pub struct SelectMediaEvent(pub Uuid);

#[derive(Clone, Debug)]
pub struct ProjectSelectionChangedEvent {
    pub selection: Vec<Uuid>,
    pub anchor: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct ProjectActiveChangedEvent {
    pub uuid: Uuid,
    pub target_frame: Option<i32>,
}

impl ProjectActiveChangedEvent {
    pub fn new(uuid: Uuid) -> Self {
        Self {
            uuid,
            target_frame: None,
        }
    }

    pub fn with_frame(uuid: Uuid, frame: i32) -> Self {
        Self {
            uuid,
            target_frame: Some(frame),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ProjectPreviousCompEvent;

#[derive(Clone, Debug)]
pub struct ClearCacheEvent;

#[derive(Clone, Debug)]
pub struct SelectionFocusEvent(pub Vec<Uuid>);
