//! Effects system for layer post-processing.
//!
//! Effects are applied to layer frames before compositing, in order.
//! Each effect is an Attrs wrapper with effect-specific schema.
//!
//! # Architecture
//!
//! ```text
//! Layer
//!   └── effects: Vec<Effect>
//!         ├── Effect { type: GaussianBlur, attrs: {radius: 5.0} }
//!         └── Effect { type: BrightnessContrast, attrs: {brightness: 0.1} }
//!
//! compose_internal():
//!   source_frame = load_source()
//!   for effect in layer.effects:
//!       source_frame = effects::apply(source_frame, effect)
//!   transform(source_frame, ...)  // effects applied BEFORE transform
//!   blend(source_frame, ...)
//! ```
//!
//! # Effect Types
//!
//! | Type | Parameters | Description |
//! |------|------------|-------------|
//! | **GaussianBlur** | `radius: 0-100` | Separable blur, O(n*r) per pass |
//! | **BrightnessContrast** | `brightness: -1..1`, `contrast: -1..1` | Color adjustment |
//! | **AdjustHSV** | `hue_shift: -180..180`, `saturation: 0..2`, `value: 0..2` | HSV color space |
//!
//! # UI Integration
//!
//! Effects are managed in the Attribute Editor (F3) under the "Effects" section:
//! - Add effects via "+" dropdown
//! - Toggle enabled/disabled with checkbox
//! - Reorder with arrow buttons
//! - Remove with "x" button
//! - Parameters edited via DragValue widgets
//!
//! # Usage
//!
//! ```ignore
//! // Add effect to layer
//! let effect = Effect::new(EffectType::GaussianBlur);
//! layer.effects.push(effect);
//!
//! // Modify effect parameters
//! if let Some(fx) = layer.effects.first_mut() {
//!     fx.attrs.set("radius", AttrValue::Float(10.0));
//! }
//!
//! // Effects are automatically applied in compose_internal()
//! ```
//!
//! # Adding New Effects
//!
//! 1. Add variant to `EffectType` enum
//! 2. Create schema constant (e.g., `FX_MY_EFFECT_SCHEMA`)
//! 3. Create implementation file (e.g., `my_effect.rs`) with `apply()` function
//! 4. Add module to `pub mod` and match arms in `schema()` and `apply()`

pub mod blur;
pub mod brightness;
pub mod hsv;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::attrs::{AttrDef, AttrSchema, AttrType, AttrValue, Attrs, FLAG_DAG, FLAG_DISPLAY, FLAG_KEYABLE};
use crate::entities::frame::Frame;

// ============================================================================
// Effect Type Enum
// ============================================================================

/// Supported effect types.
/// Each type has its own schema defining available parameters.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EffectType {
    /// Gaussian blur with configurable radius
    GaussianBlur,
    /// Brightness and contrast adjustment
    BrightnessContrast,
    /// HSV color space adjustments (hue shift, saturation, value)
    AdjustHSV,
}

impl EffectType {
    /// Get human-readable name for UI display
    pub fn display_name(&self) -> &'static str {
        match self {
            EffectType::GaussianBlur => "Gaussian Blur",
            EffectType::BrightnessContrast => "Brightness/Contrast",
            EffectType::AdjustHSV => "Adjust HSV",
        }
    }

    /// Get the attribute schema for this effect type
    pub fn schema(&self) -> &'static AttrSchema {
        match self {
            EffectType::GaussianBlur => &FX_GAUSSIAN_BLUR_SCHEMA,
            EffectType::BrightnessContrast => &FX_BRIGHTNESS_CONTRAST_SCHEMA,
            EffectType::AdjustHSV => &FX_HSV_ADJUST_SCHEMA,
        }
    }

    /// All available effect types for UI dropdown
    pub fn all() -> &'static [EffectType] {
        &[
            EffectType::GaussianBlur,
            EffectType::BrightnessContrast,
            EffectType::AdjustHSV,
        ]
    }
}

// ============================================================================
// Effect Schemas (lazy_static)
// ============================================================================

use std::sync::LazyLock;

// Flag shorthand for effect parameters (DAG + display + keyable)
const FX: u8 = FLAG_DAG | FLAG_DISPLAY | FLAG_KEYABLE;

/// Gaussian Blur schema: radius parameter
const BLUR_ATTRS: &[AttrDef] = &[
    // radius: blur radius in pixels (0 = no blur, higher = more blur)
    AttrDef::with_ui_order("radius", AttrType::Float, FX, &["0", "100", "0.5"], 0.0),
];

/// Brightness/Contrast schema
const BC_ATTRS: &[AttrDef] = &[
    // brightness: -1.0 (black) to 1.0 (white), 0.0 = no change
    AttrDef::with_ui_order("brightness", AttrType::Float, FX, &["-1", "1", "0.01"], 0.0),
    // contrast: -1.0 (gray) to 1.0 (high contrast), 0.0 = no change
    AttrDef::with_ui_order("contrast", AttrType::Float, FX, &["-1", "1", "0.01"], 1.0),
];

/// HSV Adjust schema
const HSV_ATTRS: &[AttrDef] = &[
    // hue_shift: -180 to 180 degrees rotation on color wheel
    AttrDef::with_ui_order("hue_shift", AttrType::Float, FX, &["-180", "180", "1"], 0.0),
    // saturation: 0.0 (grayscale) to 2.0 (oversaturated), 1.0 = no change
    AttrDef::with_ui_order("saturation", AttrType::Float, FX, &["0", "2", "0.01"], 1.0),
    // value: 0.0 (black) to 2.0 (overbright), 1.0 = no change
    AttrDef::with_ui_order("value", AttrType::Float, FX, &["0", "2", "0.01"], 2.0),
];

/// Schema for Gaussian Blur effect
pub static FX_GAUSSIAN_BLUR_SCHEMA: LazyLock<AttrSchema> = LazyLock::new(|| {
    AttrSchema::new("FX_GaussianBlur", BLUR_ATTRS)
});

/// Schema for Brightness/Contrast effect
pub static FX_BRIGHTNESS_CONTRAST_SCHEMA: LazyLock<AttrSchema> = LazyLock::new(|| {
    AttrSchema::new("FX_BrightnessContrast", BC_ATTRS)
});

/// Schema for HSV Adjust effect
pub static FX_HSV_ADJUST_SCHEMA: LazyLock<AttrSchema> = LazyLock::new(|| {
    AttrSchema::new("FX_AdjustHSV", HSV_ATTRS)
});

// ============================================================================
// Effect Struct
// ============================================================================

/// Effect instance attached to a layer.
/// Contains effect type, parameters (Attrs), and enabled state.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Effect {
    /// Unique identifier for this effect instance
    pub uuid: Uuid,
    /// Type of effect (determines schema and processing)
    pub effect_type: EffectType,
    /// Effect parameters (schema depends on effect_type)
    pub attrs: Attrs,
    /// Whether effect is active (disabled effects are skipped)
    pub enabled: bool,
    /// Collapsed state in UI (for persistence)
    #[serde(default)]
    pub collapsed: bool,
}

impl Effect {
    /// Create new effect with default parameters
    pub fn new(effect_type: EffectType) -> Self {
        let mut attrs = Attrs::with_schema(effect_type.schema());

        // Set default values based on effect type
        match effect_type {
            EffectType::GaussianBlur => {
                attrs.set("radius", AttrValue::Float(5.0));
            }
            EffectType::BrightnessContrast => {
                attrs.set("brightness", AttrValue::Float(0.0));
                attrs.set("contrast", AttrValue::Float(0.0));
            }
            EffectType::AdjustHSV => {
                attrs.set("hue_shift", AttrValue::Float(0.0));
                attrs.set("saturation", AttrValue::Float(1.0));
                attrs.set("value", AttrValue::Float(1.0));
            }
        }

        attrs.clear_dirty();

        Self {
            uuid: Uuid::new_v4(),
            effect_type,
            attrs,
            enabled: true,
            collapsed: false,
        }
    }

    /// Get effect display name
    pub fn name(&self) -> &'static str {
        self.effect_type.display_name()
    }
}

// ============================================================================
// Effect Application
// ============================================================================

/// Apply an effect to a frame, returning modified frame.
/// Returns None if effect processing fails.
pub fn apply(frame: &Frame, effect: &Effect) -> Option<Frame> {
    if !effect.enabled {
        return Some(frame.clone());
    }

    match effect.effect_type {
        EffectType::GaussianBlur => blur::apply(frame, &effect.attrs),
        EffectType::BrightnessContrast => brightness::apply(frame, &effect.attrs),
        EffectType::AdjustHSV => hsv::apply(frame, &effect.attrs),
    }
}

/// Apply all effects from a list to a frame, in order.
/// Skips disabled effects. Returns original frame if list is empty.
pub fn apply_all(mut frame: Frame, effects: &[Effect]) -> Option<Frame> {
    for effect in effects {
        frame = apply(&frame, effect)?;
    }
    Some(frame)
}
