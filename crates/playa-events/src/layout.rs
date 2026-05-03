//! Layout serialization events.

#[derive(Clone, Debug)]
pub struct ResetLayoutEvent;

#[derive(Clone, Debug)]
pub struct LayoutSelectedEvent(pub String);

#[derive(Clone, Debug)]
pub struct LayoutCreatedEvent(pub Option<String>);

#[derive(Clone, Debug)]
pub struct LayoutDeletedEvent(pub String);

#[derive(Clone, Debug)]
pub struct LayoutUpdatedEvent;

#[derive(Clone, Debug)]
pub struct LayoutRenamedEvent(pub String, pub String);
