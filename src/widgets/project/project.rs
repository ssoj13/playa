use std::path::PathBuf;
use uuid::Uuid;
use crate::event_bus::BoxedEvent;

/// Project window actions result
#[derive(Default)]
pub struct ProjectActions {
    pub load_sequence: Option<PathBuf>,
    pub save_project: Option<PathBuf>,
    pub load_project: Option<PathBuf>,
    pub new_comp: bool,
    pub remove_comp: Option<Uuid>,
    pub clear_all_comps: bool,
    pub hovered: bool,
    pub events: Vec<BoxedEvent>,
}

impl ProjectActions {
    pub fn new() -> Self {
        Self::default()
    }
}
