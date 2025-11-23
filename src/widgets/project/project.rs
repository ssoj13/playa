use std::path::PathBuf;

/// Project window actions result
#[derive(Default)]
pub struct ProjectActions {
    pub load_sequence: Option<PathBuf>,
    pub save_project: Option<PathBuf>,
    pub load_project: Option<PathBuf>,
    pub new_comp: bool,
    pub remove_comp: Option<String>, // comp/clip UUID to remove (unified)
    pub clear_all_comps: bool,       // clear all media
    pub hovered: bool,               // Hover state for input routing
    pub events: Vec<crate::events::AppEvent>, // queued events to send
}

impl ProjectActions {
    /// Create new empty ProjectActions
    pub fn new() -> Self {
        Self::default()
    }
}
