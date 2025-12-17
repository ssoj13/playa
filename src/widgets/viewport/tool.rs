//! Viewport tool modes and events.
//!
//! Tools control how the viewport handles input:
//! - Select: viewport scrubbing, no manipulation
//! - Move/Rotate/Scale: layer transform manipulation via gizmo

/// Active tool mode for viewport manipulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolMode {
    #[default]
    Select, // Q - viewport scrubber, no gizmo
    Move,   // W - translate
    Rotate, // E - rotate
    Scale,  // R - scale
}

impl ToolMode {
    /// All tool modes in order.
    pub const ALL: [ToolMode; 4] = [
        ToolMode::Select,
        ToolMode::Move,
        ToolMode::Rotate,
        ToolMode::Scale,
    ];

    /// Convert to string for attr storage.
    pub fn as_str(&self) -> &'static str {
        match self {
            ToolMode::Select => "select",
            ToolMode::Move => "move",
            ToolMode::Rotate => "rotate",
            ToolMode::Scale => "scale",
        }
    }

    /// Parse from attr string.
    pub fn from_str(s: &str) -> Self {
        match s {
            "move" => ToolMode::Move,
            "rotate" => ToolMode::Rotate,
            "scale" => ToolMode::Scale,
            _ => ToolMode::Select,
        }
    }

    /// Display name for UI.
    pub fn display_name(&self) -> &'static str {
        match self {
            ToolMode::Select => "Select",
            ToolMode::Move => "Move",
            ToolMode::Rotate => "Rotate",
            ToolMode::Scale => "Scale",
        }
    }

    /// Hotkey for this tool.
    pub fn hotkey(&self) -> &'static str {
        match self {
            ToolMode::Select => "Q",
            ToolMode::Move => "W",
            ToolMode::Rotate => "E",
            ToolMode::Scale => "R",
        }
    }
}

/// Event to change current viewport tool.
#[derive(Clone)]
pub struct SetToolEvent(pub ToolMode);

/// Get current tool mode from project.
pub fn current_tool(project: &crate::entities::Project) -> ToolMode {
    ToolMode::from_str(&project.tool())
}
