use std::path::PathBuf;

/// Project window actions result
#[derive(Default)]
pub struct ProjectActions {
    pub load_sequence: Option<PathBuf>,
    pub save_project: Option<PathBuf>,
    pub load_project: Option<PathBuf>,
    pub remove_clip: Option<String>,     // clip UUID to remove
    pub set_active_comp: Option<String>, // comp UUID to activate (from double-click)
    pub new_comp: bool,
    pub remove_comp: Option<String>,     // comp UUID to remove
    pub clear_all_comps: bool,           // clear all compositions
}

impl ProjectActions {
    /// Create new empty ProjectActions
    pub fn new() -> Self {
        Self::default()
    }
}

// Deprecated - use ProjectActions
pub type PlaylistActions = ProjectActions;
