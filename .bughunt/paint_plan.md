# Wave 8 — Paint comp + RefNode + AINode + Generation history

> Финальный план. Заменяет предыдущие версии. После обсуждения залочены
> **три независимых primitive'а**: RefNode (named indirection),
> AINode (AI generation as media), Paint comp (raster editing). Они
> переиспользуются друг другом, но каждый ship'нется отдельной фазой
> и имеет смысл сам по себе.

---

## Цель

Дать пользователю полный AI compositing workflow:
1. Создавать paint comp с однокадровыми layer'ами
2. Рисовать кистью на layer'ах (mutates frame buffer + sidecar PNG)
3. Связывать layer'ы через AE-style track matte (любой layer → mask
   любого другого через named RefNode)
4. Создавать AI generations как first-class media nodes с полной
   воспроизводимостью (resolved seeds, content hashes, lineage)
5. Submit через NewAINode (text-to-video / image-to-video / inpaint /
   future: img2img, upscale, style transfer)
6. Регенерировать exact или iterate с tweaks через Generations history

---

## Архитектурные решения (LOCKED)

### B1. Три independent primitives, не один большой крейт

Никакого hermetic `playa-paint`. Painting + masking + AI генерация —
расширения существующих движков:
- `playa-engine`: новые NodeKind variants (Ref, AI), Layer attrs для
  mask references, frame buffer mutation surface
- `playa-ui`: paint widget (brush kernels + apply), attr editor
  расширения, layer panel UI, AI generation dialogs
- `playa-job-inpaint`: новый crate (mirror seedance) — provider для
  inpaint / img2img endpoints

### B2. RefNode — named indirection через `NodeKind`

```rust
// crates/playa-engine/src/entities/ref_node.rs (новый)
pub struct RefNode { pub attrs: Attrs }
// attrs: A_NAME, A_TARGET_UUID, A_CHANNEL
//   target: любой uuid в project.media — другой Layer (через layer.uuid()
//   resolved via parent comp), FileNode, CompNode, любой
//   channel: Channel enum (см. ниже)

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Channel {
    Composite,                // full RGBA — для AI input refs
    #[default]
    Alpha,                    // А канал — default для track matte
    Luminance,                // luma из RGB (Rec.709) — v1.1 (нужен tonemap для HDR)
    Red, Green, Blue,
}

impl RefNode {
    pub fn new(name: &str, target: Uuid, channel: Channel) -> Self;
    pub fn target(&self) -> Option<Uuid>;
    pub fn set_target(&mut self, target: Uuid);
    pub fn channel(&self) -> Channel;
    pub fn set_channel(&mut self, ch: Channel);
}

// NodeKind:
pub enum NodeKind {
    File(FileNode), Comp(CompNode), Camera(CameraNode),
    Text(TextNode),
    +Ref(RefNode),
    +AI(AINode),
}

// Node trait:
//   RefNode is_renderable() = false   (utility node, не виден на таймлайне)
//   RefNode is_listed() = true        (виден в Project tree)
```

### B3. Track matte через RefNode (Layer.mask_ref_uuid)

```rust
// Layer attrs:
const A_MASK_REF_UUID: &str = "mask_ref_uuid";  // Uuid → RefNode; nil = no mask
```

В `CompNode::compose_internal` после композиции каждого layer'а:
1. `if let Some(ref_uuid) = layer.mask_ref_uuid()`
2. `project.media.get(ref_uuid)` → `NodeKind::Ref(rn)`
3. `target = rn.target()`; ищем target's current frame (либо как Layer
   в текущем comp'е по `source_uuid`, либо как media node)
4. Извлекаем channel `rn.channel()` из target's pixel buffer
5. Multiply current layer's composited alpha by mask value
6. Edge cases: target deleted → ref orphan → skip mask + log warn;
   target.status != Loaded → skip + log trace

**Auto-create UX:** в Layer panel dropdown «Mask: [None | пик
layer X]» — при выборе layer X скрытно создаётся RefNode named
`"{layer.name}.alpha"` с target=X.uuid + channel=Alpha. Ref виден в
Project tree, можно переименовать / перенаправить target / переключить
channel вручную.

### B4. AINode — AI generation as first-class media

```rust
// crates/playa-engine/src/entities/ai_node.rs (новый)
pub struct AINode { pub attrs: Attrs }
```

Attrs:
| key | type | назначение |
|---|---|---|
| A_NAME | Str | "Cyberpunk wolf" |
| A_PROMPT | Str | пользовательский prompt |
| A_PROVIDER | Str | `"seedance.text_to_video"` etc. |
| A_INPUT_REFS | List<Uuid> | RefNode'ы (через indirection) |
| A_PARAMS_TEMPLATE | Json | provider-specific params БЕЗ seed (template для UI) |
| A_GENERATIONS | Json (Vec<Generation>) | история всех run'ов |
| A_ACTIVE_GENERATION | Uuid | какая из истории сейчас compose'ится |
| A_PARENT_NODE | Uuid опционально | если ноду создали через "Duplicate AINode" |

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Generation {
    pub uuid: Uuid,                          // unique per run
    pub timestamp_secs: u64,
    pub provider: String,                    // verbatim provider kind
    pub provider_version: Option<String>,    // если provider возвращает (model id etc)
    pub params: serde_json::Value,           // ВСЕ params С РАЗРЕШЁННЫМ seed
    pub input_snapshots: Vec<RefSnapshot>,
    pub job_id: Uuid,
    pub request_id: Option<String>,
    pub result_path: PathBuf,
    pub cost_usd: Option<f64>,
    pub parent_gen_uuid: Option<Uuid>,       // lineage chain
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RefSnapshot {
    pub ref_uuid: Uuid,
    pub target_uuid: Uuid,
    pub target_content_hash: String,         // SHA-256 of target's bytes
    pub channel: Channel,
}
```

**AINode is_renderable() = true** — comp'ы могут содержать Layer
ссылающиеся на AINode через `source_uuid` (как с FileNode). На compose
читается frame из текущего `A_ACTIVE_GENERATION.result_path`.

**Seed reproducibility (critical):**

`SubmitDialog.seed_text == ""` больше НЕ означает "let provider decide".
- На submit: `let resolved_seed: u64 = if empty { rand::random() } else { parsed };`
- `params["seed"] = resolved_seed` (concrete u64 идёт на provider)
- `Generation.params["seed"]` хранит то же значение
- Regenerate exact = copy Generation.params verbatim → submit → byte-identical (mod GPU FP indeterminism)

**Lineage operations:**
| Кнопка | Что делает |
|---|---|
| Generate new | Create new Generation в той же AINode, prompt/refs from current attrs, новый seed (если "auto"), submit |
| Regenerate exact | Pick selected Generation → copy params verbatim → submit (для debug / retry) |
| Iterate from | Pick selected → copy as new params, открыть Edit dialog → submit как child (`parent_gen_uuid` = selected) |
| Set active | Pick generation из истории → A_ACTIVE_GENERATION = its uuid → comp перекомпонуется с этой версией |
| Delete generation | Remove from history + delete result file (warn если active) |

**Result storage:**

```
<project_dir>/ai_results/{ainode_uuid}/
   ├── {gen_uuid_1}.mp4
   ├── {gen_uuid_2}.mp4
   └── manifest.json          # mirror of A_GENERATIONS — даже если проект потерян, метаданные читаются
```

`manifest.json` — disaster recovery: если проект .playa файл утерян,
manifest позволяет восстановить historу. Mirror, не source of truth
(source = A_GENERATIONS attr в project save).

### B5. Paint comp = обычный CompNode

Никакого spec'ального типа. Paint comp = CompNode со специфическим
содержимым: однокадровые layer'ы (источники = FileNode'ы указывающие
на PNG). Конвенция, не enum variant.

**Paint UI surface** — toggle в Viewport tab:
- Кнопка `🖌 Paint` в viewport toolbar (рядом с tool select)
- Toggle on → cursor становится brush, side panel показывает Paint
  toolbox (size/hardness/opacity sliders, brush/eraser/select switches)
- Toggle off → нормальный viewport mode

Активный «target layer» для рисования = selected layer в Layer panel.

### B6. Layer frame buffer mutation (copy-on-edit)

```rust
// crates/playa-engine/src/entities/file_node.rs (existing)
// Add: paint state tracking
struct PaintState {
    original_path: PathBuf,    // immutable original
    sidecar_path: PathBuf,     // <project>/paint/{uuid}.png
    dirty_buffer: Option<Vec<u8>>,  // in-memory edits not yet flushed to sidecar
    last_save_secs: u64,
}
```

Lifecycle:
1. First stroke на FileNode → copy `original_path` → `sidecar_path` →
   FileNode.path switches to sidecar. PaintState created.
2. Each stroke → mutate `dirty_buffer` (clone of cached PixelBuffer::U8)
   → swap into cached frame via `Arc::make_mut` → invalidate cache
   epoch для зависимых composite layers
3. Auto-save: idle 5s OR explicit "Save Paint" button → write
   `dirty_buffer` to `sidecar_path`, clear dirty_buffer
4. Project save: ensures all paint sidecars on disk

**Crash safety**: on app boot, scan project's `paint/` дир. Если
sidecar существует и FileNode ссылается на original — переключить
на sidecar (assume editing was in progress).

### B7. Submit flow через AINode

```
[User clicks "+ Generate" в Jobs tab OR "+ AI Layer" в Layer panel]
                       ↓
              [NewAINode wizard]
                       ↓
       Provider dropdown: text-to-video / image-to-video / inpaint
                       ↓
       Build input refs (auto-create RefNodes if user picks layers
       from current comp)
                       ↓
       Edit params (prompt, duration, resolution, seed (random by
       default but resolved to u64 immediately))
                       ↓
       [Submit]
                       ↓
       1. Create AINode в project.media
       2. Compute input_snapshots с SHA-256 каждого ref target
       3. Create Generation { uuid, timestamp, params (resolved seed),
          provider, input_snapshots, parent_gen_uuid=None }
       4. queue.submit(provider_kind, params + ainode_uuid as context)
       5. Generation.job_id = id
       6. Push Generation onto AINode.A_GENERATIONS
       7. AINode.A_ACTIVE_GENERATION = generation.uuid
       8. AINode placed in active comp at user's position (Layer
          с source_uuid = AINode.uuid())
                       ↓
       [Job runs through existing queue pipeline]
                       ↓
       JobEvent::Completed → result_path → assign в Generation.result_path
       → save A_GENERATIONS attr back to AINode → comp re-composes,
       AINode-source-layers теперь показывают frame'ы из result_path
```

### B8. Provider crate: `playa-job-inpaint`

Mirror `playa-job-seedance` структуры:
- `InpaintProvider impl JobProvider`
- kinds: `"inpaint.flux_kontext"`, `"inpaint.flux_pro_v1_1"` (выбрать
  один при старте Phase 6)
- params: `{prompt, image_url (data URL or fal storage), mask_url, ...}`
- run: POST → poll → download → result: `{png_path, request_id, model_version, ...}`
- estimate_cost_usd: per-provider rate
- Регистрируется в `playa-app::build_default_job_queue` рядом с
  Seedance providers

### B9. AttrEditor extensions

Для AINode selected:
- Provider radio
- Prompt textarea
- Input refs list: each row = `[ref name dropdown] [channel radio] [×]`,
  + button "Add ref" (creates RefNode in project.media)
- Params: dynamic fields per provider (provider exposes JSON schema —
  v2; v1 hardcode 3-4 known providers)
- Seed: u64 input + `🎲 random` button (regenerates) + `🔒 lock` toggle
- Generations panel:
  ```
  ▸ 2026-05-10 16:23   active   gen_xyz
  ▸ 2026-05-10 15:11            gen_abc   (parent=gen_xyz)
  ```
- Action buttons: `Generate new` `Regenerate exact` `Iterate` `Set active`

Для RefNode selected:
- Target picker: dropdown all named nodes in project (with content type
  icon)
- Channel radio: Composite / Alpha / R / G / B / Luminance (latter
  greyed in v1)

Для Layer selected:
- Existing attrs (position, scale, opacity, blend_mode, etc.)
- + `Mask Ref:` dropdown → all RefNodes in project (None / ref1 /
  ref2 / ...) → set/clear Layer.mask_ref_uuid

---

## Phase breakdown

### Phase 1 — Track matte foundation
Engine: `RefNode` + `Channel` + NodeKind variant + Layer attr
`mask_ref_uuid` + compose_internal masking step + cache invalidation
hook + frozen JSON tests + `is_listed/is_renderable` Project tree
support. ~700 LOC. ~1.5 дня.

**Useful standalone**: track matte — fundamental compositing feature,
не paint-specific. Может shipnуться как auto-merge даже если Phase 2+
не делаем.

### Phase 1b — Layer panel + AttrEditor для RefNode
UI: per-layer Mask dropdown (auto-create RefNode), Project tree
shows RefNodes, AttrEditor handles RefNode selection. ~300 LOC.
~half-day.

### Phase 2 — Paint UI + brush kernels
New module `playa-ui/src/paint/`: brush/apply/tools/widget/panel.
Pure-fn `apply_brush(buffer, brush, x, y, color)` (unit-testable
without egui). Viewport `🖌 Paint` toggle. Side panel toolbox.
Strokes mutate cached frame in-place, invalidate cache (no disk yet).
~700 LOC. ~1 день.

### Phase 3 — Copy-on-edit sidecar storage
`FileNode::PaintState` + first-stroke copy + auto-save by idle +
crash recovery scan + "Save Paint" button. ~400 LOC. ~half-day.

### Phase 4 — New Paint Layer (W×H) button
"+ Paint Layer" → создаёт transparent RGBA PNG в `<project>/paint/`
→ import как FileNode → add to comp. ~150 LOC. couple hours.

### Phase 5 — Undo/redo (paint scope)
ActionStack per FileNode в edit-сессии. 50-stroke ring, snapshot per
stroke (diff-rect → v1.1). Ctrl+Z / Ctrl+Shift+Z. ~250 LOC. ~half-day.

### Phase 6 — AINode + Generation history (engine)
New `AINode` + `Generation` + `RefSnapshot` types. NodeKind variant.
Compose path: AINode source layer → fetch from active_generation.result_path.
SHA-256 content hashing util. manifest.json mirror writer. Engine
unit tests: lineage chain, active swap, snapshot validation. ~600 LOC.
~1 день.

### Phase 7 — `playa-job-inpaint` crate
Mirror seedance: provider impl, MockHttp tests, register in
build_default_job_queue. Pick provider endpoint при старте phase
(flux-pro/v1.1 inpainting vs runway vs seedream — research). ~500
LOC. ~1 день.

### Phase 8 — Submit flow via AINode (host wiring)
NewAINode wizard в Jobs tab + "+ AI Layer" в Layer panel.
SubmitDialog (или новый AINodeDialog?) с provider picker + ref editor
+ params + seed-resolved-at-submit. Generation creation +
result writing on Completed. Existing US-15 v2 auto-attach path
заменяется на "fill ActiveGeneration.result_path + invalidate
AINode layer caches". ~700 LOC. ~1 день.

### Phase 9 — AttrEditor для AINode
Generations history panel (sortable). Generate / Regenerate exact /
Iterate / Set active actions. Lineage display. ~400 LOC. ~half-day.

### Phase 10 (v1.1, optional polish)
- Luma channel support (HDR tonemap path)
- Fill / Rect select / Color picker tools
- Diff-rect undo (memory efficient)
- AISettings prefs slice (default seed lock, retention policy)
- Multi-frame paint (per-frame edits on animated layer)
- AINode chain workflow (output of A → input of B)

**v1 total (Phases 1-9): ~8-10 фокусных дней.**

---

## Order & checkpointing

Order: **1 → 1b → 6 → 7 → 8 → 9 → 2 → 3 → 4 → 5**.

Reason: AI workflow (gen + history + provider + submit) — это
независимая фича, не требует paint. Paint работает поверх RefNode
foundation. Если в середине осознаем что приоритеты сместились —
можем остановиться после Phase 9 (full AI workflow ships без paint)
или после Phase 5 (paint + masks ships без AINode).

**Checkpoints для коммита:**
- Phase 1 + 1b → 1 PR (foundation + UI вместе)
- Phase 6 → 1 PR (AINode engine alone — без provider пока nothing
  происходит, но JSON шейп залочен)
- Phase 7 → 1 PR (inpaint provider standalone tested)
- Phase 8 + 9 → 1 PR (host wiring + full UX)
- Phase 2 + 3 → 1 PR (paint UI + persistence together)
- Phase 4 → 1 PR
- Phase 5 → 1 PR

---

## Open questions (deferred to phase starts)

| # | Question | Phase |
|---|---|---|
| 1 | Channel default for mask: Alpha (locked) vs Luma | locked Alpha |
| 2 | Inpaint provider: flux-pro v1.1 vs runway gen-fill vs seedream | Phase 7 |
| 3 | Auto-save interval for paint (5s? configurable?) | Phase 3 |
| 4 | manifest.json schema versioning | Phase 6 |
| 5 | Cross-comp ref resolution: same comp only v1 vs project-wide | Phase 1 |
| 6 | Project-dir API resolution (where is `<project_dir>`?) | Phase 3 |
| 7 | Comp resolution inheritance from first layer | Phase 4 |
| 8 | AttrValue::Reference variant vs two-attr storage | Phase 1 |
| 9 | Result_path collision (regenerate exact overwrites?) | Phase 6 |

---

## Risk table

| Risk | Mitigation |
|---|---|
| Track matte cache invalidation correctness | Phase 1 unit tests: changing target invalidates dependent layers |
| Arc<PixelBuffer> mutation race с compose | Phase 2: Arc::make_mut on stroke, compose уже за фрейм-lock |
| Sidecar bloat on large projects | Phase 3: lifecycle (delete layer → delete sidecar); manual cleanup tool |
| Track matte miss non-loaded source | Phase 1: skip + log; track matte best-effort |
| AINode result file lost mid-load | Phase 8: missing result_path → AINode shows "missing" placeholder + offer regen |
| Generation params drift if provider API changes | Phase 6: params JSON verbatim; provider version stored; warn on regen mismatch |
| Seed determinism varies by GPU/CPU | Document; offer "exact reproduction not guaranteed cross-host" |
| AINode lineage chain deep nesting UI | Phase 9: max-depth visualization; flatten in display |
| Content hash mismatch on regen | Phase 6: warn + ask user "input changed, continue?" |
| Storage growth from generations history | Phase 9: AISettings retention policy + per-node cap |
| Inpaint mask polarity (white vs black) | Phase 7: verify per-provider; document expected convention |

---

## Что выйдет за scope v1 (Phases 1-9)

- Animated paint (per-frame strokes on multi-frame source)
- Vector layers / paths
- Multi-frame inpaint with temporal consistency
- Stylus pressure
- Layer groups / adjustment layers
- Non-destructive filters
- Provider chain auto-composition (output of A becomes input of B
  automatically)
- AINode batch ladder (one node, N seed variations as Generations
  in one submit) — partially overlap с existing batch_mode

Эти приходят после v1 если UX устоится и есть запрос.

---

## Migration / API contract

**Backwards compat**: все новые attrs используют `#[serde(default)]` →
старые playa.json saves загружаются. NodeKind enum gains variants —
serde tag automatically handles new vs old. На load старого файла
RefNode / AINode просто отсутствуют → проект работает как раньше.

**Workspace deps additions:**
- `sha2` (для content hashing) — workspace dep
- `rand` (для seed generation) — workspace dep
- Существующий `base64` уже в playa-app

**Phase 1 уже **готов к старту** — все дизайны зафиксированы, файлы
известны, edge cases имеют mitigation, тесты сформулированы.**
