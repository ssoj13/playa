//! `RefNode` — named indirection node.
//!
//! A `RefNode` lives in `project.media` like other nodes. It carries a
//! `target_uuid` (any node in the project) plus a `channel` selector.
//! Layers and AI nodes consume `RefNode`s by uuid:
//!
//! - Track matte: `Layer.mask_ref_uuid = <ref_uuid>` — comp resolves the
//!   ref, reads the chosen channel of the target's frame, multiplies the
//!   layer's composited alpha.
//! - AI input: `AINode.input_refs = [ref_uuid, ...]` — each ref resolves
//!   to source pixels (typically `Channel::Composite` for image inputs,
//!   `Channel::Alpha` for masks).
//!
//! Resolution is **best-effort**: missing target → ref returns `None` and
//! consumers skip the mask / drop the input with a log warning. No hard
//! errors at render time.
//!
//! Why a separate node (vs an inline attr on Layer):
//! - Reusable: 5 layers can reference the same `RefNode` and editing the
//!   target / channel once updates all consumers.
//! - Discoverable: appears in the Project tree with a name.
//! - Decoupled lifecycle: deleting a layer doesn't auto-delete its mask
//!   ref; user owns the cleanup.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::attr_schemas::REF_SCHEMA;
use super::attrs::{AttrValue, Attrs};
use super::frame::Frame;
use super::keys::{A_CHANNEL, A_NAME, A_TARGET_UUID, A_UUID};
use super::node::{ComputeContext, Node};

/// Channel selector used by `RefNode` consumers when sampling a target
/// frame. Stored as a string in attrs for forward-compat with future
/// additions; unknown strings fall back to [`Channel::Alpha`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Channel {
    /// Full RGBA, typical for AI image inputs.
    Composite,
    /// Alpha channel only — default for track matte.
    #[default]
    Alpha,
    /// Luminance computed from RGB (Rec.709). Requires LDR input
    /// — HDR frames must be tonemapped first. Consumers may downgrade
    /// to `Alpha` and log a warning if the source is non-U8.
    Luminance,
    Red,
    Green,
    Blue,
}

impl Channel {
    /// Persistent wire form. Stable across versions.
    pub fn as_str(self) -> &'static str {
        match self {
            Channel::Composite => "composite",
            Channel::Alpha => "alpha",
            Channel::Luminance => "luminance",
            Channel::Red => "red",
            Channel::Green => "green",
            Channel::Blue => "blue",
        }
    }

    /// Inverse of [`Self::as_str`]. Unknown strings fall back to
    /// [`Channel::Alpha`] (the most permissive / least surprising choice
    /// for legacy / corrupted saves).
    pub fn from_str(s: &str) -> Self {
        match s {
            "composite" => Channel::Composite,
            "alpha" => Channel::Alpha,
            "luminance" => Channel::Luminance,
            "red" => Channel::Red,
            "green" => Channel::Green,
            "blue" => Channel::Blue,
            _ => Channel::Alpha,
        }
    }
}

/// Named indirection node: `target_uuid` + `channel`.
///
/// Lives in `Project.media`. `is_renderable()` is false (utility node —
/// not visible on the timeline); `is_listed()` is true so it appears in
/// the Project tree.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RefNode {
    pub attrs: Attrs,
}

impl RefNode {
    /// Construct a new reference. Fresh uuid; target + channel + name
    /// stored verbatim.
    pub fn new(name: &str, target: Uuid, channel: Channel) -> Self {
        let mut attrs = Attrs::with_schema(&*REF_SCHEMA);
        attrs.set(A_UUID, AttrValue::Uuid(Uuid::new_v4()));
        attrs.set(A_NAME, AttrValue::Str(name.to_string()));
        attrs.set(A_TARGET_UUID, AttrValue::Uuid(target));
        attrs.set(A_CHANNEL, AttrValue::Str(channel.as_str().to_string()));
        attrs.clear_dirty();
        Self { attrs }
    }

    /// Construct with a specific uuid (used by deserialization paths /
    /// tests that need stable ids).
    pub fn with_uuid(name: &str, uuid: Uuid, target: Uuid, channel: Channel) -> Self {
        let mut node = Self::new(name, target, channel);
        node.attrs.set(A_UUID, AttrValue::Uuid(uuid));
        node.attrs.clear_dirty();
        node
    }

    /// The node this ref points at. `None` when the target attr is
    /// missing or `Uuid::nil()` (unset). Existence of that node in
    /// `project.media` is **not** checked here — that's the consumer's
    /// responsibility at resolve time.
    pub fn target(&self) -> Option<Uuid> {
        let u = self.attrs.get_uuid(A_TARGET_UUID)?;
        if u.is_nil() { None } else { Some(u) }
    }

    /// Set or clear the target. Pass [`Uuid::nil`] to clear (semantically
    /// equivalent to `None`).
    pub fn set_target(&mut self, target: Uuid) {
        self.attrs.set(A_TARGET_UUID, AttrValue::Uuid(target));
    }

    /// Channel selector. Defaults to [`Channel::Alpha`] when the attr is
    /// absent or holds an unknown string.
    pub fn channel(&self) -> Channel {
        self.attrs
            .get_str(A_CHANNEL)
            .map(Channel::from_str)
            .unwrap_or_default()
    }

    pub fn set_channel(&mut self, channel: Channel) {
        self.attrs
            .set(A_CHANNEL, AttrValue::Str(channel.as_str().to_string()));
    }

    /// Re-bind this node's attr storage to [`REF_SCHEMA`] after
    /// deserialization. Mirrors `FileNode::attach_schema` /
    /// `CompNode::attach_schema` — `Project::attach_schemas` calls it
    /// on every node in the media pool at load time so dirty-tracking
    /// + UI metadata work correctly.
    pub fn attach_schema(&mut self) {
        self.attrs.attach_schema(&*REF_SCHEMA);
    }
}

impl Node for RefNode {
    fn uuid(&self) -> Uuid {
        self.attrs.get_uuid(A_UUID).unwrap_or_else(Uuid::nil)
    }

    fn name(&self) -> &str {
        self.attrs.get_str(A_NAME).unwrap_or("Ref")
    }

    fn node_type(&self) -> &'static str {
        "Ref"
    }

    fn attrs(&self) -> &Attrs {
        &self.attrs
    }

    fn attrs_mut(&mut self) -> &mut Attrs {
        &mut self.attrs
    }

    fn inputs(&self) -> Vec<Uuid> {
        // The ref's target IS its sole semantic input. Returning it here
        // lets graph-walk tooling (impact analysis, dependency tracking)
        // see the connection.
        self.target().into_iter().collect()
    }

    /// Refs don't produce pixels. Consumers compute their effective frame
    /// by looking up the target node and sampling [`Self::channel`].
    fn compute(&self, _frame: i32, _ctx: &ComputeContext) -> Option<Frame> {
        None
    }

    fn is_dirty(&self, _ctx: Option<&ComputeContext>) -> bool {
        self.attrs.is_dirty()
    }

    fn mark_dirty(&self) {
        self.attrs.mark_dirty();
    }

    fn clear_dirty(&self) {
        self.attrs.clear_dirty();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_round_trips_through_string() {
        for ch in [
            Channel::Composite,
            Channel::Alpha,
            Channel::Luminance,
            Channel::Red,
            Channel::Green,
            Channel::Blue,
        ] {
            assert_eq!(Channel::from_str(ch.as_str()), ch);
        }
    }

    #[test]
    fn channel_default_is_alpha() {
        assert_eq!(Channel::default(), Channel::Alpha);
    }

    #[test]
    fn channel_unknown_string_falls_back_to_alpha() {
        assert_eq!(Channel::from_str("zalgo"), Channel::Alpha);
        assert_eq!(Channel::from_str(""), Channel::Alpha);
    }

    #[test]
    fn new_ref_carries_target_and_channel() {
        let target = Uuid::new_v4();
        let r = RefNode::new("MaskFromBg", target, Channel::Composite);
        assert_eq!(r.name(), "MaskFromBg");
        assert_eq!(r.target(), Some(target));
        assert_eq!(r.channel(), Channel::Composite);
        assert_eq!(r.node_type(), "Ref");
        assert_ne!(r.uuid(), Uuid::nil());
    }

    #[test]
    fn nil_target_resolves_as_none() {
        let r = RefNode::new("Unset", Uuid::nil(), Channel::Alpha);
        assert_eq!(r.target(), None);
    }

    #[test]
    fn set_target_and_channel_update_attrs() {
        let mut r = RefNode::new("R", Uuid::nil(), Channel::Alpha);
        let new_target = Uuid::new_v4();
        r.set_target(new_target);
        r.set_channel(Channel::Luminance);
        assert_eq!(r.target(), Some(new_target));
        assert_eq!(r.channel(), Channel::Luminance);
    }

    #[test]
    fn inputs_lists_target_when_set() {
        let target = Uuid::new_v4();
        let r = RefNode::new("R", target, Channel::Alpha);
        assert_eq!(r.inputs(), vec![target]);

        let unset = RefNode::new("Unset", Uuid::nil(), Channel::Alpha);
        assert!(unset.inputs().is_empty());
    }

    #[test]
    fn serde_round_trip_preserves_fields() {
        let target = Uuid::new_v4();
        let original = RefNode::new("MaskFromBg", target, Channel::Luminance);
        let json = serde_json::to_string(&original).expect("serialize");
        let restored: RefNode = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored.uuid(), original.uuid());
        assert_eq!(restored.name(), original.name());
        assert_eq!(restored.target(), Some(target));
        assert_eq!(restored.channel(), Channel::Luminance);
    }

    #[test]
    fn with_uuid_pins_identity() {
        let stable_uuid = Uuid::new_v4();
        let target = Uuid::new_v4();
        let r = RefNode::with_uuid("Stable", stable_uuid, target, Channel::Alpha);
        assert_eq!(r.uuid(), stable_uuid);
    }
}
