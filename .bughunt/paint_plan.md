# Paint comp + track matte — переписанный план #2

> Старая версия предлагала hermetic `playa-paint` crate с собственным
> canvas. **Эта версия её заменяет.** Painting встраивается в
> существующий CompNode pipeline: paint comp = обычный Comp с
> однокадровыми layer'ами, рисование = мутация frame buffer слоя.

## Цель

Дать пользователю:
1. Создать comp типа «paint» с одним или несколькими однокадровыми
   PNG слоями.
2. Рисовать кистью на каждом слое (brush mutates frame buffer →
   compositor рекомпозит → viewport показывает обновлённый результат).
3. Использовать AE-style track matte: любой слой может быть
   замаскирован альфой/luma другого слоя из того же comp'а.
4. Отправить comp на inpaint provider: один из слоёв = source, другой
   (через dropdown) = mask. Результат auto-attach как новый node.

## Архитектурные решения (LOCKED после обсуждения)

### B1. Никакого hermetic `playa-paint` крейта

Painting встраивается в `playa-engine` (mutable frame buffer +
sidecar storage) + `playa-ui` (paint widget + tool palette).
Композитор — существующий `CompNode::compose_internal`. Layer model
— существующий `LayerNode`.

### B2. AE-style track matte для масок

LayerNode добавляет:

```rust
pub mask_source: Option<Uuid>,  // None = no mask, Some(uuid) = matted by layer Uuid
pub mask_channel: MaskChannel,  // Alpha / Luminance (default Alpha)
```

В `CompNode::compose_internal` после композиции каждого layer'а:
- Если `mask_source.is_some()`:
  - Look up referenced layer's frame
  - Read its alpha (or luma per `mask_channel`)
  - Multiply current composite layer's alpha by mask value
- Layer, который ЯВЛЯЕТСЯ mask_source для кого-то, **всё равно**
  блендится в visual composite (как в AE — track matte source видим
  по умолчанию, но AE имеет toggle «hide matte layer»; v1 не делаем
  toggle, видим всегда).

UI: в Layer panel рядом с каждой строкой — dropdown
«Mask: [None | LayerName1 | LayerName2 | ...]».

### B3. Copy-on-edit sidecar storage

Когда юзер делает первый stroke на layer'е:
1. Engine копирует оригинальный PNG в `<project_dir>/paint/{layer_uuid}.png`
   (или `<cache_dir>/playa/paint-unsaved/{project_uuid}/{layer_uuid}.png`
   если проект не сохранён).
2. FileNode.path переключается на sidecar.
3. Дальнейшие strokes мутируют in-memory `PixelBuffer::U8` (cached
   frame). Cache invalidates → CompNode перекомпонует → viewport
   обновляется.
4. На «Save All Paint» (или auto-save by idle): in-memory buffer
   пишется в sidecar PNG.

Оригинал юзера никогда не трогается. `paint/` дир рядом с проектом
портабельна (zip → перенос).

### B4. «New Paint Layer (W×H)» button

В Layer panel paint comp'а — кнопка. По клику:
- Открыть диалог: `Width: [comp.width] Height: [comp.height]`
  (defaults), + button `Use comp resolution`.
- Создать transparent RGBA PNG (все пиксели 0,0,0,0).
- Сохранить в `<project_dir>/paint/blank_{ts}.png`.
- Импортировать как FileNode → добавить в comp.

### B5. Paint UI surface

Два варианта где живёт paint mode (выбираем при старте Phase 2):
- **A**: Новый `DockTab::Paint`. Когда активный comp выбран и
  пользователь переключился на этот таб — рендерим viewport
  composite + brush overlay. Tools в side panel.
- **B**: Paint **mode toggle** в существующем Viewport tab. Кнопка
  «🖌 Paint» в toolbar. Toggle on → курсор становится кистью, side
  panel показывает paint tools.

Склонен к B — меньше дублирующего viewport кода. Решим в Phase 2.

### B6. Hermetic brush kernel module

Brush + apply остаются в playa-ui (не отдельный крейт):
```rust
// crates/playa-ui/src/paint/
//   mod.rs            - re-exports
//   brush.rs          - Brush struct + kernel cache
//   apply.rs          - apply_brush(buffer, brush, x, y, color) — pure fn
//   tools.rs          - Tool enum + ToolState
//   widget.rs         - paint_overlay_widget (brush cursor + click handling)
//   panel.rs          - PaintToolboxWidget (sliders)
```

Pure functions, легко unit-test'аются (no egui in apply.rs).

### B7. Inpaint flow

SubmitDialog получает новый endpoint `Inpaint` (помимо
TextToVideo/ImageToVideo):
- Dropdown «Source layer»: list of layers in active comp
- Dropdown «Mask layer»: same list
- Prompt textarea
- На submit: read source layer's PNG bytes + mask layer's PNG bytes →
  base64 data URLs → submit к `playa-job-inpaint` provider (новый
  crate, mirror seedance) → результат auto-attach как новый node
  (existing US-15 v2 pipeline).

## Engine changes (Phase 1 — track matte foundation)

### Files

```
crates/playa-engine/src/entities/layer.rs  (or wherever LayerNode lives)
  + mask_source: Option<Uuid>
  + mask_channel: MaskChannel { Alpha, Luminance }
  + serde(default) on both for back-compat

crates/playa-engine/src/entities/comp_node.rs
  compose_internal:
    for each layer in stack:
      composite layer onto canvas (existing)
      if let Some(src_uuid) = layer.mask_source:
        find src layer's current frame
        for each pixel: composite.alpha *= mask_value(src.pixel, mask_channel)
      (if src not found / not Loaded: skip mask this frame, log warn)

crates/playa-engine/src/entities/cache_manager.rs (or wherever)
  + invalidation hook: when layer A is mask_source for layer B,
    paint stroke on A invalidates B's cached composite
```

### UI changes for Phase 1

```
crates/playa-ui/src/widgets/timeline/* (where layer outline is rendered)
  + per-layer mask source dropdown
```

## Phase breakdown — new

### Phase 1: Track matte foundation (engine + UI)

- LayerNode.mask_source + mask_channel + serde back-compat
- CompNode::compose_internal applies mask
- Mask source cache invalidation
- Layer panel mask dropdown UI
- Frozen JSON test: legacy comp save without mask_source loads
- Visual test: two layers, set B as mask of A, observe A masked by B's alpha

**Scope: ~600-800 LOC, ~1.5 days. Useful standalone — track matte is
a fundamental compositing feature, not paint-specific.**

### Phase 2: Paint UI infrastructure

- New `playa-ui/src/paint/` module (brush, apply, tools, widget, panel)
- Paint mode toggle in viewport (B above)
- Side panel: brush size / hardness / opacity sliders
- Brush apply mutates active FileNode's frame buffer
- Cache invalidation hook from paint stroke
- No persistence yet — strokes lost on app restart

**Scope: ~700 LOC, ~1 day**

### Phase 3: Copy-on-edit sidecar storage

- First stroke on a layer: copy original PNG → `<project_dir>/paint/{uuid}.png`,
  redirect FileNode.path
- Auto-save by idle (e.g. 5s after last stroke): write in-memory
  buffer to sidecar PNG on disk
- «Save All Paint» button (manual flush)
- Crash safety: on app restart, if sidecar exists and is newer than
  original — use sidecar
- Project not saved → cache_dir fallback

**Scope: ~400 LOC, ~0.5 day**

### Phase 4: New Paint Layer (W×H) button

- Layer panel button «+ Paint Layer»
- Dialog: width/height inputs, default = comp resolution
- Create transparent RGBA PNG → import → add to comp

**Scope: ~150 LOC, couple hours**

### Phase 5: Undo/redo (paint scope)

- ActionStack per FileNode being edited
- 50-stroke limit, snapshot per stroke (v1) → diff-rect per stroke (v2)
- Ctrl+Z / Ctrl+Shift+Z hotkeys

**Scope: ~250 LOC, ~0.5 day**

### Phase 6: Inpaint provider crate

- `playa-job-inpaint` mirror of playa-job-seedance
- Pick fal endpoint (flux-pro/v1.1/inpainting or similar — research
  at start of phase)
- Submit: upload source PNG + mask PNG to fal storage → run → poll →
  download result → auto-attach via US-15 v2 pipeline

**Scope: ~500 LOC, ~1 day**

### Phase 7: SubmitDialog Inpaint variant

- New endpoint radio: TextToVideo / ImageToVideo / **Inpaint**
- Inpaint mode: hide image_url field, show two layer-pickers
  (source / mask) populated from active comp's layers
- Cost estimate per-provider
- Submit dispatches kind = `"inpaint.flux_kontext"` (or whatever)

**Scope: ~250 LOC, ~0.5 day**

### Phase 8 (v1.1, optional polish)

- Fill tool (bucket flood-fill on mask alpha)
- Rect select (stamp into mask)
- Color picker (sample from layer)
- "Hide matte layer" toggle (AE compat)
- Diff-rect undo for memory efficiency on large canvases

**v1 total (Phases 1-7): ~4-5 фокусных дней.**

## Open questions deferred to phase starts

1. **Inpaint provider**: flux-pro v1.1 inpainting vs runway gen-fill
   vs seedance/seedream inpaint. Pricing + quality. Phase 6.
2. **MaskChannel default**: Alpha (default) vs Luminance (white=mask).
   AE convention. Start of Phase 1.
3. **Auto-save interval**: 5s idle? On every stroke? Configurable in
   prefs? Start of Phase 3.
4. **Mask edge softness**: hard threshold on alpha, or soft (0..255
   smooth blend)? V1 — passes through as-is, soft alpha works
   naturally. Start of Phase 1.
5. **Project save path resolution**: какой API возвращает
   `<project_dir>`? Check `Project.save_path` / `Project.path` at
   start of Phase 3.
6. **Comp resolution policy**: when first layer is added to a fresh
   comp, does comp inherit layer's W×H or stays at predefined size?
   Start of Phase 4.

## Risks / mitigations (updated)

| Risk | Mitigation |
|---|---|
| Track matte cache invalidation correctness | Phase 1 unit tests: changing mask source invalidates target's cache |
| Paint stroke on shared `Arc<PixelBuffer>` racing with frame compose | Phase 2: mutate via `Arc::make_mut` or replace buffer atomically; cache will refetch on next compose |
| Sidecar PNG bloat for long projects | Phase 3: per-layer sidecar lifecycle: deleted if user deletes layer; auto-cleanup on project close |
| Track matte misses non-loaded frames | Phase 1: skip mask + log warn when src.status != Loaded; track matte == best-effort |
| AE-style mask UX expectations vs simplified model | Phase 1: dropdown + alpha channel only. Luma/invert/etc — v1.1 |
| Brush apply perf on large canvas | Phase 2: profile; if slow → rayon worker. Single-thread first. |

## Что выйдет за scope v1 (Phases 1-7)

- Animated paint (paint different on each frame of a multi-frame layer)
- Vector layers / paths
- Multi-frame inpaint (consistent across frames — temporal video edit)
- Stylus pressure sensitivity
- Layer groups
- Adjustment layers
- Non-destructive filters

## Migration / commit strategy

Каждая Phase = 1 коммит (или 2 если engine + UI). После Phase 1 уже
есть полезная feature (track matte) — можно merge даже если paint
дальше не делаем. Phases 2-5 связаны (paint без track matte бесполезен
для inpaint, но track matte без paint полезен сам по себе).

Order matters: Phase 1 ДО Phase 2 (paint без masking — окей, но inpaint
не работает; paint + track matte — полная фича).
