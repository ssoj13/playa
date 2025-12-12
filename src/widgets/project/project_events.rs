//! Project management and selection events.

use std::path::PathBuf;
use uuid::Uuid;

// === Project Management ===

#[derive(Clone, Debug)]
pub struct AddClipEvent(pub PathBuf);

#[derive(Clone, Debug)]
pub struct AddClipsEvent(pub Vec<PathBuf>);

/// Add folder event - scans directory recursively for media files
#[derive(Clone, Debug)]
pub struct AddFolderEvent(pub PathBuf);

#[derive(Clone, Debug)]
pub struct AddCompEvent {
    pub name: String,
    pub fps: f32,
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

/// Quick save event - saves to last known path or shows dialog
#[derive(Clone, Debug)]
pub struct QuickSaveEvent;

/// Open project dialog event - shows file picker
#[derive(Clone, Debug)]
pub struct OpenProjectDialogEvent;

// === Selection ===

#[derive(Clone, Debug)]
pub struct SelectMediaEvent(pub Uuid);

#[derive(Clone, Debug)]
pub struct ProjectSelectionChangedEvent {
    pub selection: Vec<Uuid>,
    pub anchor: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct ProjectActiveChangedEvent(pub Uuid);

/// Navigate back to previous comp (U key)
#[derive(Clone, Debug)]
pub struct ProjectPreviousCompEvent;
