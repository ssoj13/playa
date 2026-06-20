# playa-engine — working notebook

> Local notebook for this crate. Keep current: what / why / how / gotchas / TODO.
> Authorship of the workflow conventions: Alex Joss <joss13@gmail.com>.

## What
Core scene/compositing engine: entities (Node/Comp/FileNode/…), the attribute
system (`Attrs`/`AttrValue`), frame model, compositor, project (de)serialization.

## Attribute system (`src/entities/attrs.rs`)
- `AttrValue` — 15 variants: Bool/Str/Int8/Int(i32)/**Int64(i64)**/UInt/Float(f32)/
  Vec3/Vec4/Mat3/Mat4/Uuid/List/Map/Set/Json. serde-derived. Custom Hash +
  PartialEq (NOT derived — Float compares by bits; Hash has NO catch-all, so any
  new variant MUST add a Hash arm or the crate won't compile).
- `Attrs` = `HashMap<String, AttrValue>` + dirty flag + optional `AttrSchema`
  (runtime-only overlay; never rejects free-form keys). Free-form keys are fully
  supported, editable, and serialized.
- `Attrs::merge(other)` — absorb another attr set, overwriting on collision.

## Metadata flow (load → edit → persist → round-trip)
1. `Loader::header(path)` (loader.rs) → full `Attrs` via `attrs_from_io`
   (`av_from_kv` bridges every `playa_io::AttrKv` → `AttrValue`; arrays → `List`,
   matrices reshape row-major).
2. `create_sequence/single_file_node` (file_node.rs) call `node.attrs.merge(attrs)`
   to absorb the COMPLETE source header (every `exr:*` attr) onto the FileNode —
   NOT just width/height. (Before this they cherry-picked 4 keys and dropped the
   rest.)
3. `node.attrs` → attribute editor (renders + edits dynamic keys, incl. Int64,
   List, Mat3/Mat4) → persisted by `Project::to_json` (plain serde over the map).
4. Round-trip on encode lives in `playa-ui` (`encode.rs::write_exr_frame` reads
   the `exr:` keys back via `core_attr_from_engine`).

## GOTCHA
- `AttrValue::Int(i32)` vs `Int64(i64)`: absorbed EXR ints land in `Int64`
  (lossless for timecode/keycode). Don't narrow to `Int`.
- Adding an `AttrValue` variant ripples to: Hash (attrs.rs), the editor match
  (`playa-ui ae_ui.rs:243`, exhaustive `&mut` match — NO catch-all), and the
  encode reverse bridge. The compiler finds them; build the full workspace.

## Build
Native build pulls ffmpeg → use `python bootstrap.py b --debug`
(→ `cargo xtask build`, sets vcpkg manifest env + MSVC). A bare
`cargo check -p playa-engine` fails on ffmpeg-sys-next without that env.

## TODO
- [ ] FLAG_READONLY is defined but never enforced in the UI — derived width/
      height/fps are editable today (can desync). Wire `AttrDef::is_readonly()`.
- [ ] Optional provenance flag to distinguish pristine source metadata from
      user-edited overrides.
