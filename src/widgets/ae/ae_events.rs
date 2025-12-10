//! Attribute Editor-specific events.
//!
//! Kept separate to mirror other widget event modules and keep routing consistent.

/// Emitted when the user drags the Project/Attributes splitter so that other panels
/// can persist the updated ratio.
#[derive(Clone, Debug)]
pub struct AttributesSplitChangedEvent(pub f32);

/// Emitted when the Attributes panel gains or loses focus; useful for context-aware hotkeys.
#[derive(Clone, Debug)]
pub struct AttributesFocusChangedEvent(pub bool);
