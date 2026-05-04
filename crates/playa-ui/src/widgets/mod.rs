//! UI Widgets - modular, reusable UI components
//!
//! Each widget is self-contained and communicates via EventBus. Drag payload state that crosses
//! panels (e.g. project → timeline) lives in [`dnd`].

pub mod actions;
pub mod ae;
pub mod dnd;
pub mod file_dialogs;
pub mod node_editor;
pub mod project;
pub mod status;
pub mod timeline;
pub mod viewport;
