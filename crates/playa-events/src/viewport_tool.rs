//! Viewport tool modes and bus events (`SetToolEvent`).

/// Active tool mode for viewport manipulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolMode {
    #[default]
    Select,
    Move,
    Rotate,
    Scale,
}

impl ToolMode {
    pub const ALL: [ToolMode; 4] = [
        ToolMode::Select,
        ToolMode::Move,
        ToolMode::Rotate,
        ToolMode::Scale,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            ToolMode::Select => "select",
            ToolMode::Move => "move",
            ToolMode::Rotate => "rotate",
            ToolMode::Scale => "scale",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "move" => ToolMode::Move,
            "rotate" => ToolMode::Rotate,
            "scale" => ToolMode::Scale,
            _ => ToolMode::Select,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            ToolMode::Select => "Select",
            ToolMode::Move => "Move",
            ToolMode::Rotate => "Rotate",
            ToolMode::Scale => "Scale",
        }
    }

    pub fn hotkey(&self) -> &'static str {
        match self {
            ToolMode::Select => "Q",
            ToolMode::Move => "W",
            ToolMode::Rotate => "E",
            ToolMode::Scale => "R",
        }
    }
}

/// Change current viewport tool (see [`ToolMode`]).
#[derive(Clone, Debug)]
pub struct SetToolEvent(pub ToolMode);
