//! Generic attribute storage shared across core types.
//!
//! Used by Frame, Clip, Layer, Comp, Project.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Generic attribute value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AttrValue {
    Str(String),
    Int(i32),
    UInt(u32),
    Float(f32),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    Mat3([[f32; 3]; 3]),
    Mat4([[f32; 4]; 4]),
}

/// Attribute container: string key → typed value.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Attrs {
    #[serde(default)]
    map: HashMap<String, AttrValue>,
}

impl Attrs {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn set(&mut self, key: impl Into<String>, value: AttrValue) {
        self.map.insert(key.into(), value);
    }

    pub fn get(&self, key: &str) -> Option<&AttrValue> {
        self.map.get(key)
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        match self.map.get(key) {
            Some(AttrValue::Str(s)) => Some(s),
            _ => None,
        }
    }

    pub fn get_float(&self, key: &str) -> Option<f32> {
        match self.map.get(key) {
            Some(AttrValue::Float(v)) => Some(*v),
            _ => None,
        }
    }
}

