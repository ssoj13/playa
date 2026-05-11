//! `AINode` — AI generation as first-class media.
//!
//! An `AINode` represents an asynchronous AI generation (text-to-video,
//! image-to-video, inpaint, img2img, upscale, etc.) whose output is
//! a clip / image that other comps reference like a regular `FileNode`.
//!
//! # Reproducibility contract
//!
//! Every submission produces a [`Generation`] record. The record
//! captures the **resolved** parameters that were sent to the provider
//! — `"seed": "auto"` is resolved to a concrete `u64` at submit time so
//! a future "Regenerate exact" with the same record reproduces byte-
//! identical (or near-identical, mod GPU floating-point indeterminism)
//! output. Each input reference also snapshots its target's content
//! hash so the engine can warn when the inputs have drifted from the
//! state at submit time.
//!
//! # Storage
//!
//! Generations live as a JSON array under [`A_GENERATIONS`] (using
//! `AttrValue::Json` to keep `Attrs` permissive about nested
//! structures). The active generation is identified by uuid in
//! [`A_ACTIVE_GENERATION`].
//!
//! Result files live in `<project_dir>/ai_results/{ainode_uuid}/{gen_uuid}.{ext}`.
//! A sidecar `manifest.json` mirrors the generation history for
//! disaster recovery — if the `.playa` project file is lost, the
//! manifest is enough to rebuild the AINode.
//!
//! # Phase 6 scope
//!
//! This module ships the **data model + helper accessors**. The
//! `compute()` impl returns a [`FrameStatus::Placeholder`] frame until
//! Phase 8 wires the submit flow and on-completion result loading.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::attr_schemas::AI_SCHEMA;
use super::attrs::{AttrValue, Attrs};
use super::frame::Frame;
use super::keys::{
    A_ACTIVE_GENERATION, A_GENERATIONS, A_INPUT_REFS, A_NAME, A_PARAMS_TEMPLATE, A_PROMPT,
    A_PROVIDER, A_UUID,
};
use super::node::{ComputeContext, Node};
use super::ref_node::Channel;

/// A single immutable record of one AI generation run. Records are
/// **never mutated after creation** — regenerating an AINode pushes a
/// new `Generation` onto the history; editing the prompt etc. updates
/// `A_PARAMS_TEMPLATE` but doesn't touch existing Generations.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Generation {
    /// Unique per run. Distinct from `AINode.uuid` (one AINode has
    /// many Generations).
    pub uuid: Uuid,
    pub timestamp_secs: u64,
    /// Verbatim provider kind, e.g. `"seedance.text_to_video"`.
    pub provider: String,
    /// Optional model version returned by the provider (if any).
    pub provider_version: Option<String>,
    /// All parameters sent to the provider, with `"seed"` and any
    /// other auto-fields RESOLVED to concrete values. Verbatim copy
    /// stored for reproducibility.
    pub params: Value,
    /// Snapshot of each input ref at submit time (target uuid +
    /// content hash + channel). Engine warns on regen when current
    /// hashes don't match.
    pub input_snapshots: Vec<RefSnapshot>,
    /// Linked job in [`crate::core::JobQueue`]. Once the job completes,
    /// `result_path` is filled in.
    pub job_id: Uuid,
    /// Provider-returned request id (e.g. fal `request_id`). Optional
    /// because not every provider exposes one.
    pub request_id: Option<String>,
    /// Disk path to the produced asset (mp4 / png). Empty until the
    /// job completes.
    pub result_path: PathBuf,
    /// Cost reported by the provider in USD, if known.
    pub cost_usd: Option<f64>,
    /// `uuid` of the Generation this was iterated from ("Iterate from"
    /// in the UI). Forms a lineage chain.
    pub parent_gen_uuid: Option<Uuid>,
}

/// Content + identity snapshot of one input ref at submit time. Allows
/// the engine to detect "input drifted" before a regen would emit a
/// different result than the original generation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RefSnapshot {
    /// `RefNode` uuid (the indirection layer).
    pub ref_uuid: Uuid,
    /// Target node uuid (what the ref pointed at).
    pub target_uuid: Uuid,
    /// SHA-256 of the target's relevant bytes at submit time. For a
    /// `FileNode` this is the file content; for a `CompNode` it's a
    /// hash of the composited frame at the chosen frame index.
    pub target_content_hash: String,
    pub channel: Channel,
}

/// AI generation node. Lives in `Project.media` like a `FileNode`. Comp
/// layers reference it via `Layer.source_uuid`; on compose, the active
/// `Generation.result_path` provides the pixel data.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AINode {
    pub attrs: Attrs,
}

impl AINode {
    /// Construct a new AINode with a name and provider kind. All other
    /// attrs default to empty / sensible values; caller populates
    /// `prompt`, `input_refs`, `params_template` before the first
    /// submission.
    pub fn new(name: &str, provider: &str) -> Self {
        let mut attrs = Attrs::with_schema(&*AI_SCHEMA);
        attrs.set(A_UUID, AttrValue::Uuid(Uuid::new_v4()));
        attrs.set(A_NAME, AttrValue::Str(name.to_string()));
        attrs.set(A_PROVIDER, AttrValue::Str(provider.to_string()));
        attrs.set(A_PROMPT, AttrValue::Str(String::new()));
        attrs.set(A_PARAMS_TEMPLATE, AttrValue::Json("{}".to_string()));
        attrs.set(A_INPUT_REFS, AttrValue::Json("[]".to_string()));
        attrs.set(A_GENERATIONS, AttrValue::Json("[]".to_string()));
        attrs.set(A_ACTIVE_GENERATION, AttrValue::Uuid(Uuid::nil()));
        attrs.clear_dirty();
        Self { attrs }
    }

    pub fn with_uuid(name: &str, provider: &str, uuid: Uuid) -> Self {
        let mut node = Self::new(name, provider);
        node.attrs.set(A_UUID, AttrValue::Uuid(uuid));
        node.attrs.clear_dirty();
        node
    }

    pub fn prompt(&self) -> String {
        self.attrs
            .get_str(A_PROMPT)
            .map(|s| s.to_string())
            .unwrap_or_default()
    }

    pub fn set_prompt(&mut self, prompt: impl Into<String>) {
        self.attrs.set(A_PROMPT, AttrValue::Str(prompt.into()));
    }

    pub fn provider(&self) -> String {
        self.attrs
            .get_str(A_PROVIDER)
            .map(|s| s.to_string())
            .unwrap_or_default()
    }

    pub fn set_provider(&mut self, provider: impl Into<String>) {
        self.attrs.set(A_PROVIDER, AttrValue::Str(provider.into()));
    }

    /// Deserialise `A_INPUT_REFS` to a `Vec<Uuid>`. Returns empty on
    /// missing attr or unparseable JSON.
    pub fn input_refs(&self) -> Vec<Uuid> {
        self.attrs
            .get_str(A_INPUT_REFS)
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default()
    }

    pub fn set_input_refs(&mut self, refs: &[Uuid]) {
        let json = serde_json::to_string(refs).unwrap_or_else(|_| "[]".to_string());
        self.attrs.set(A_INPUT_REFS, AttrValue::Json(json));
    }

    /// Deserialise `A_PARAMS_TEMPLATE` to `serde_json::Value`. Returns
    /// `Value::Null` on missing / unparseable.
    pub fn params_template(&self) -> Value {
        self.attrs
            .get_str(A_PARAMS_TEMPLATE)
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or(Value::Null)
    }

    pub fn set_params_template(&mut self, params: &Value) {
        let json = serde_json::to_string(params).unwrap_or_else(|_| "{}".to_string());
        self.attrs.set(A_PARAMS_TEMPLATE, AttrValue::Json(json));
    }

    /// Full history of generations, ordered by insertion. Returns empty
    /// on missing / unparseable.
    pub fn generations(&self) -> Vec<Generation> {
        self.attrs
            .get_str(A_GENERATIONS)
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default()
    }

    fn write_generations(&mut self, gens: &[Generation]) {
        let json = serde_json::to_string(gens).unwrap_or_else(|_| "[]".to_string());
        self.attrs.set(A_GENERATIONS, AttrValue::Json(json));
    }

    /// Append a new generation to the history and make it active.
    /// Returns the generation's uuid for the caller to track.
    pub fn add_generation(&mut self, generation: Generation) -> Uuid {
        let id = generation.uuid;
        let mut all = self.generations();
        all.push(generation);
        self.write_generations(&all);
        self.attrs.set(A_ACTIVE_GENERATION, AttrValue::Uuid(id));
        id
    }

    /// Replace an existing generation in-place by uuid. Used when the
    /// job completes and the result path / cost / metadata become
    /// available. No-op when the uuid isn't in history.
    pub fn update_generation(&mut self, generation: Generation) {
        let mut all = self.generations();
        if let Some(slot) = all.iter_mut().find(|g| g.uuid == generation.uuid) {
            *slot = generation;
            self.write_generations(&all);
        }
    }

    /// Currently-active generation's uuid. Nil = none yet (e.g. a
    /// freshly-created AINode that hasn't been submitted).
    pub fn active_generation_uuid(&self) -> Option<Uuid> {
        let u = self.attrs.get_uuid(A_ACTIVE_GENERATION)?;
        if u.is_nil() { None } else { Some(u) }
    }

    /// Resolve [`Self::active_generation_uuid`] to the matching
    /// `Generation` record. None when no active generation or the
    /// referenced uuid no longer exists in history (e.g. user deleted
    /// it).
    pub fn active_generation(&self) -> Option<Generation> {
        let active = self.active_generation_uuid()?;
        self.generations().into_iter().find(|g| g.uuid == active)
    }

    /// Make `gen_uuid` the active generation. Caller is responsible for
    /// passing a uuid that exists in the history — this method doesn't
    /// validate (caching the validation cost is the consumer's call).
    pub fn set_active_generation(&mut self, gen_uuid: Uuid) {
        self.attrs
            .set(A_ACTIVE_GENERATION, AttrValue::Uuid(gen_uuid));
    }

    /// Delete a generation from history. If it was active, the next
    /// most-recent generation (or none) becomes active.
    pub fn remove_generation(&mut self, gen_uuid: Uuid) {
        let mut all = self.generations();
        let was_active = self.active_generation_uuid() == Some(gen_uuid);
        all.retain(|g| g.uuid != gen_uuid);
        self.write_generations(&all);
        if was_active {
            let new_active = all.last().map(|g| g.uuid).unwrap_or_else(Uuid::nil);
            self.attrs
                .set(A_ACTIVE_GENERATION, AttrValue::Uuid(new_active));
        }
    }

    /// Re-bind attr storage to [`AI_SCHEMA`] after deserialization.
    /// Mirrors `FileNode::attach_schema`.
    pub fn attach_schema(&mut self) {
        self.attrs.attach_schema(&*AI_SCHEMA);
    }
}

impl Node for AINode {
    fn uuid(&self) -> Uuid {
        self.attrs.get_uuid(A_UUID).unwrap_or_else(Uuid::nil)
    }

    fn name(&self) -> &str {
        self.attrs.get_str(A_NAME).unwrap_or("AI")
    }

    fn node_type(&self) -> &'static str {
        "AI"
    }

    fn attrs(&self) -> &Attrs {
        &self.attrs
    }

    fn attrs_mut(&mut self) -> &mut Attrs {
        &mut self.attrs
    }

    fn inputs(&self) -> Vec<Uuid> {
        // Input refs ARE the semantic inputs. Graph-walk tools see them
        // here for free.
        self.input_refs()
    }

    /// Phase 6 stub: returns `None` so comps fall back to placeholder.
    /// Phase 8 wires this to load the active generation's `result_path`
    /// via the existing `FileNode` decoding path / worker pool.
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

/// Compute a SHA-256 hash of arbitrary bytes, returned as lowercase
/// hex string. Used to populate `RefSnapshot.target_content_hash` at
/// submit time.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        use std::fmt::Write;
        write!(s, "{b:02x}").unwrap();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_gen(provider: &str) -> Generation {
        Generation {
            uuid: Uuid::new_v4(),
            timestamp_secs: 1234567890,
            provider: provider.to_string(),
            provider_version: None,
            params: serde_json::json!({"prompt": "wolf", "seed": 42}),
            input_snapshots: vec![],
            job_id: Uuid::new_v4(),
            request_id: None,
            result_path: PathBuf::new(),
            cost_usd: None,
            parent_gen_uuid: None,
        }
    }

    #[test]
    fn new_carries_name_and_provider() {
        let ai = AINode::new("Wolf", "seedance.text_to_video");
        assert_eq!(ai.name(), "Wolf");
        assert_eq!(ai.provider(), "seedance.text_to_video");
        assert_eq!(ai.prompt(), "");
        assert!(ai.generations().is_empty());
        assert_eq!(ai.active_generation_uuid(), None);
    }

    #[test]
    fn add_generation_makes_it_active() {
        let mut ai = AINode::new("Wolf", "seedance.t2v");
        let g = make_gen("seedance.t2v");
        let g_uuid = g.uuid;
        let added = ai.add_generation(g);
        assert_eq!(added, g_uuid);
        assert_eq!(ai.active_generation_uuid(), Some(g_uuid));
        let gens = ai.generations();
        assert_eq!(gens.len(), 1);
        assert_eq!(gens[0].uuid, g_uuid);
    }

    #[test]
    fn add_second_gen_takes_over_active() {
        let mut ai = AINode::new("Wolf", "seedance.t2v");
        let g1 = make_gen("seedance.t2v");
        let g2 = make_gen("seedance.t2v");
        let g1_uuid = g1.uuid;
        let g2_uuid = g2.uuid;
        ai.add_generation(g1);
        ai.add_generation(g2);
        assert_eq!(ai.active_generation_uuid(), Some(g2_uuid));
        // Switch back to g1 explicitly.
        ai.set_active_generation(g1_uuid);
        assert_eq!(ai.active_generation_uuid(), Some(g1_uuid));
    }

    #[test]
    fn update_generation_replaces_by_uuid() {
        let mut ai = AINode::new("Wolf", "seedance.t2v");
        let mut g = make_gen("seedance.t2v");
        let g_uuid = g.uuid;
        ai.add_generation(g.clone());
        g.result_path = PathBuf::from("/tmp/wolf.mp4");
        g.cost_usd = Some(1.21);
        ai.update_generation(g);
        let restored = ai.active_generation().expect("active gen");
        assert_eq!(restored.uuid, g_uuid);
        assert_eq!(restored.result_path, PathBuf::from("/tmp/wolf.mp4"));
        assert_eq!(restored.cost_usd, Some(1.21));
    }

    #[test]
    fn remove_active_picks_next_or_clears() {
        let mut ai = AINode::new("Wolf", "seedance.t2v");
        let g1 = make_gen("seedance.t2v");
        let g2 = make_gen("seedance.t2v");
        let g1_uuid = g1.uuid;
        let g2_uuid = g2.uuid;
        ai.add_generation(g1);
        ai.add_generation(g2);
        // g2 is active; removing it must fall back to g1.
        ai.remove_generation(g2_uuid);
        assert_eq!(ai.active_generation_uuid(), Some(g1_uuid));
        // Removing the last one clears active.
        ai.remove_generation(g1_uuid);
        assert_eq!(ai.active_generation_uuid(), None);
        assert!(ai.generations().is_empty());
    }

    #[test]
    fn input_refs_round_trip_via_json() {
        let mut ai = AINode::new("Wolf", "inpaint.flux");
        let r1 = Uuid::new_v4();
        let r2 = Uuid::new_v4();
        ai.set_input_refs(&[r1, r2]);
        assert_eq!(ai.input_refs(), vec![r1, r2]);
    }

    #[test]
    fn params_template_round_trip() {
        let mut ai = AINode::new("Wolf", "seedance.t2v");
        let p = serde_json::json!({"duration": 4, "resolution": "480p"});
        ai.set_params_template(&p);
        assert_eq!(ai.params_template(), p);
    }

    #[test]
    fn inputs_lists_input_refs() {
        let mut ai = AINode::new("Wolf", "inpaint.flux");
        let r1 = Uuid::new_v4();
        ai.set_input_refs(&[r1]);
        assert_eq!(ai.inputs(), vec![r1]);
    }

    #[test]
    fn serde_round_trip_preserves_history() {
        let mut ai = AINode::new("Wolf", "seedance.t2v");
        ai.set_prompt("a wolf");
        let g = make_gen("seedance.t2v");
        ai.add_generation(g.clone());
        let json = serde_json::to_string(&ai).expect("serialize");
        let restored: AINode = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored.name(), "Wolf");
        assert_eq!(restored.prompt(), "a wolf");
        let restored_gens = restored.generations();
        assert_eq!(restored_gens.len(), 1);
        assert_eq!(restored_gens[0].uuid, g.uuid);
        assert_eq!(restored_gens[0].params, g.params);
    }

    #[test]
    fn sha256_hex_known_values() {
        // SHA-256("abc") is a canonical test vector.
        let h = sha256_hex(b"abc");
        assert_eq!(
            h,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // Empty input is also well-known.
        let h0 = sha256_hex(b"");
        assert_eq!(
            h0,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
