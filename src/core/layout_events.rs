//! Layout serialization events.
//!
//! Events for managing named UI layouts (dock splits, timeline state, viewport state).
//! Layouts are stored in AppSettings and persisted automatically via serde.

/// Save current layout to project attrs (legacy, kept for compatibility).
#[derive(Clone, Debug)]
pub struct SaveLayoutEvent;

/// Load layout from project attrs (legacy, kept for compatibility).
#[derive(Clone, Debug)]
pub struct LoadLayoutEvent;

/// Reset layout to defaults.
#[derive(Clone, Debug)]
pub struct ResetLayoutEvent;

/// Select a named layout from settings.layouts.
/// Applies the layout configuration to dock_state, timeline_state, viewport_state.
#[derive(Clone, Debug)]
pub struct LayoutSelectedEvent(pub String);

/// Create a new named layout by duplicating current UI state.
/// If name is None, auto-generates name like "Layout 2", "Layout 3", etc.
#[derive(Clone, Debug)]
pub struct LayoutCreatedEvent(pub Option<String>);

/// Delete a named layout from settings.layouts.
/// If deleted layout was current, current_layout becomes empty.
#[derive(Clone, Debug)]
pub struct LayoutDeletedEvent(pub String);

/// Update the current layout with current UI state.
/// Called when dock splits, timeline, or viewport state changes.
/// Does nothing if current_layout is empty or not found.
#[derive(Clone, Debug)]
pub struct LayoutUpdatedEvent;
