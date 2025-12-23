//! Node editor events.

/// Fit all nodes in view
#[derive(Clone, Debug)]
pub struct NodeEditorFitAllEvent;

/// Fit selected nodes (or all if none selected)
#[derive(Clone, Debug)]
pub struct NodeEditorFitSelectedEvent;

/// Re-layout nodes in tree arrangement
#[derive(Clone, Debug)]
pub struct NodeEditorLayoutEvent;
