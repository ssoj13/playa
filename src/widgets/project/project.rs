use std::path::PathBuf;

/// Project window actions result
#[derive(Default)]
pub struct ProjectActions {
    pub load_sequence: Option<PathBuf>,
    pub save_project: Option<PathBuf>,
    pub load_project: Option<PathBuf>,
    pub set_active_comp: Option<String>, // comp UUID to activate (from click)
    pub new_comp: bool,
    pub remove_comp: Option<String>, // comp/clip UUID to remove (unified)
    pub clear_all_comps: bool,       // clear all media
    pub hovered: bool,               // Hover state for input routing
}

impl ProjectActions {
    /// Create new empty ProjectActions
    pub fn new() -> Self {
        Self::default()
    }
}

// Deprecated - use ProjectActions
pub type PlaylistActions = ProjectActions;
