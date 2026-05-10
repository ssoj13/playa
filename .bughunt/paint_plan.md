# `playa-paint` — Photoshop-на-минималках для inpaint workflow

## Цель

Дать пользователю «нарисовать маску» поверх существующего слоя/кадра
и отправить результат на inpaint-провайдер (flux.kontext, seedream,
runway gen-fill, etc.). На выходе — новый слой, который автоматически
импортируется в проект (как auto-attach mp4 в US-15).

## Не-цели (v1)

- Multi-layer raster compositing (Photoshop-style). Один canvas + один
  mask layer. Никакого Layers Panel.
- Vector tools, text, filters, adjustments, channels, PSD.
- Live preview inpaint (некоторые провайдеры это умеют — это v2).
- Editing video frames in-place (только single-frame snapshots).

## Архитектурные решения (LOCKED)

### A1. `playa-paint` — hermetic egui-only crate

- Зависимости: `eframe` (для `egui` re-export), `image`, `serde`,
  `log`. **Нет** `playa-engine`, `playa-ui`, `playa-jobs`. Можно тащить
  в любое egui-приложение.
- Re-uses `playa_events` для inpaint-related events если они нужны
  hosting-стороне (опционально, через feature `events`).
- `forbid(unsafe_code)`.

### A2. Однослойный canvas + бинарная маска

- `PaintCanvas { base: RgbaImage, mask: GrayImage, … }`.
- `base` неизменяемый (это импортированный кадр).
- `mask` — single-channel `u8`, изначально 0 (no mask). User рисует
  → значения растут (255 = full inpaint). Несжатая Vec<u8>.
- Soft brush opacity делает «мягкие края маски» через blending в
  mask-buffer (не 0/255, а 0..255).

### A3. Action-stack undo/redo

- Каждое движение мыши (down→drag→up) = одна `PaintAction`.
- `Vec<PaintAction>` + cursor index. Undo откатывает cursor, не
  выбрасывая истории; redo идёт вперёд.
- Для v1 каждое action хранит ПОЛНЫЙ snapshot mask после операции
  (просто, дёшево для small canvas, можно потом сжать diff-rect'ами).
- Hardcoded limit: 50 шагов. Truncate с конца.

### A4. Tools (v1 minimum):

| Tool | Hotkey | Behavior |
|---|---|---|
| Brush (paint mask) | B | Click+drag → blend opacity into mask via brush kernel |
| Eraser (clear mask) | E | Same as brush but subtracts |
| Pan | Space+drag | Move canvas around viewport |
| Zoom | scroll wheel | Center on cursor |
| Color picker | I | Sample base RGB → not relevant for binary mask, OPTIONAL |
| Fill | G | Flood-fill mask region (bucket) |
| Rect select | M | Stamp mask rectangle |

V1 ship: **Brush, Eraser, Pan, Zoom, Rect select**. Fill + Picker = v2.

### A5. Brush params

```rust
struct Brush {
    size_px: u32,         // 1..=512, default 50
    hardness: f32,        // 0.0 (soft, full Gaussian) .. 1.0 (hard edge)
    opacity: f32,         // 0.0..=1.0, alpha applied to mask kernel
    flow: f32,            // 0.0..=1.0, multiplier per drag-step
}
```

Kernel pre-computed at `set_size`/`set_hardness` change. Cached in
`Brush::kernel: Vec<u8>` flat 2D.

### A6. Integration с playa-app

- Новый `DockTab::Paint` под `#[cfg(feature = "paint")]`.
- Открывается из:
  - Project right-click "Open in Paint" — затаскивает выбранный
    FileNode как `base`.
  - SubmitDialog "Edit in Paint…" — open + после save сабмит
    inpaint-задачи.
- Снимок текущего viewport через `PlayaApp::snapshot_current_frame`
  (уже есть) → загрузка в Paint = одна кнопка.

### A7. Inpaint provider — отдельный крейт

- `playa-job-inpaint` (или extension в playa-job-seedance, если fal
  endpoint совместим). Mirror архитектуры Seedance:
  - `InpaintProvider` impl `JobProvider`
  - kind `"inpaint.flux_kontext"` / `"inpaint.runway_gen_fill"`
  - params: `{image_url, mask_url, prompt, …}` — image+mask data
    URLs или fal-storage URLs
  - run: POST → poll → download → result `{mp4_path|png_path: …}`
- Paint → save: канонический формат = PNG (image+mask separately,
  как 2 файла в snapshots/).
- Submit flow: фронтенд upload'ит PNG → берёт URL → params → submit.

## Файлы / структура крейта

```
crates/playa-paint/
├── Cargo.toml             # eframe, image, serde, log, forbid(unsafe)
├── src/
│   ├── lib.rs             # pub use … of public surface
│   ├── canvas.rs          # PaintCanvas: base + mask + transform + selection
│   ├── brush.rs           # Brush params + kernel cache + apply
│   ├── tool.rs            # ToolMode enum + ToolState
│   ├── action.rs          # PaintAction + ActionStack (undo/redo)
│   ├── widget.rs          # egui Widget: pan/zoom + brush draw + tool overlay
│   ├── toolbox.rs         # left-side palette: size/hardness/opacity sliders
│   ├── export.rs          # save_to_png(base, path) + save_mask_to_png(mask, path)
│   └── session.rs         # PaintSession: state machine + serde for restart
└── tests/
    ├── brush_kernel.rs    # kernel math: soft vs hard, size scaling
    ├── action_stack.rs    # undo/redo cursor
    ├── apply_brush.rs     # canvas mutation: blend opacity correctly
    └── export_roundtrip.rs # save → load round-trip preserves pixels
```

## Phase breakdown — pragmatic execution order

### Phase 1: Skeleton + canvas display (smallest verifiable slice)

- Crate scaffold. Cargo.toml. lib.rs export.
- `PaintCanvas::new(rgba: Vec<u8>, w, h)` — just store base. Empty mask.
- `PaintCanvasWidget` egui widget: render base as Image, no tools.
- Pan + zoom (mouse drag + scroll).
- Integration: new `DockTab::Paint` in playa-app, opens with current
  snapshot.
- Acceptance: cargo run, open tab, see image, pan/zoom works.
- **Scope: ~300 LOC, ~half-day**

### Phase 2: Brush + eraser + mask blending

- Brush struct + kernel pre-compute.
- Apply brush to mask on mouse drag.
- Render mask as a red 50% overlay over base.
- Sliders: size, hardness, opacity in `ToolboxWidget`.
- Hotkey B/E to switch tool.
- Acceptance: paint over the image, see red mask appear, parameters
  control thickness/softness.
- **Scope: ~400 LOC, ~day**

### Phase 3: Undo/redo + action stack

- `PaintAction::Brush { stroke_points, brush_snapshot, mask_before }`
- Action stack with cursor.
- Ctrl+Z / Ctrl+Shift+Z hotkeys.
- Acceptance: paint → undo → mask reverts. Redo restores.
- **Scope: ~200 LOC, ~half-day**

### Phase 4: Export + integration shim

- `export::save_base_and_mask(canvas, dir) -> (PathBuf, PathBuf)` —
  writes `inpaint_{ts}_base.png` + `inpaint_{ts}_mask.png` to
  cache-dir/playa/inpaint-staging/.
- New button in PaintWidget: "Submit for Inpaint…" → calls
  `host_callback(base_path, mask_path)` provided by playa-app.
- playa-app callback: opens a new SubmitDialog variant (Inpaint mode,
  not Seedance) with both paths pre-filled.
- Acceptance: open Paint, draw mask, click submit → SubmitDialog
  appears with paths shown.
- **Scope: ~300 LOC, ~half-day**

### Phase 5: Inpaint provider crate

- `playa-job-inpaint` skeleton (mirror playa-job-seedance).
- Pick a fal endpoint (`fal-ai/flux-pro/v1.1/inpainting` is one;
  research current best at start of phase).
- Submit flow: upload base PNG + mask PNG to fal storage → run →
  poll → download result.
- Register provider in playa-app's `build_default_job_queue`.
- Cost estimate per request (varies per provider — check rates).
- Acceptance: real submit through Paint → real result → auto-attach
  as new project node (uses existing US-15 v2 pipeline).
- **Scope: ~500 LOC, ~day**

### Phase 6 (v1.1, optional):

- Fill tool (bucket flood-fill on mask).
- Rect select (stamp mask rectangle).
- Color picker (not useful for mask but consistent UX).
- Better keyboard shortcuts (Maya-style).

**Total v1: Phases 1-5 = ~3-4 days end-to-end of focused work.**

## Open questions / decisions to make at start of Phase 1

1. **Inpaint provider choice**: flux.kontext vs flux-pro inpaint vs
   runway gen-fill vs seedream-3 inpaint. Pricing + quality varies.
   Pick after a quick comparison при старте Phase 5; не блокирует
   Phases 1-4.

2. **Mask format wire-side**: most fal inpaint endpoints want
   white=inpaint / black=keep. Verify against chosen provider; flip
   if needed.

3. **Canvas resolution**: should we downscale to e.g. 1024×1024 для
   inpaint (большинство моделей всё равно режут)? Или сохранять
   native? Решить в Phase 4.

4. **PaintSession serde persistence**: сохраняется ли paint state
   между запусками приложения (как unfinished work)? Or one-shot?
   В Phase 3 default = ephemeral; persistence как Phase 6 feature.

5. **Threading**: brush apply runs на UI thread. Канвас maybe 2048²
   = 4M пикселей mask, brush kernel 50² = 2.5K samples per move.
   На 60 fps это 150K samples/frame — должно идти на UI thread без
   проблем. Если будут лаги на больших canvas — Phase 6 переносим
   на rayon worker.

## Cargo features (как и в playa-jobs)

```toml
[features]
default = []
events = ["dep:playa-events"]  # publish PaintActionEvent etc — opt-in
serde = ["dep:serde"]           # persist canvas to disk
```

Hosting приложение включает только нужное. Дефолт — голый рисовалка.

## API surface (drafted; refine при кодинге)

```rust
// crates/playa-paint/src/lib.rs
pub use canvas::{PaintCanvas, PaintCanvasConfig};
pub use widget::PaintCanvasWidget;
pub use toolbox::ToolboxWidget;
pub use tool::ToolMode;
pub use brush::Brush;
pub use action::PaintAction;
pub use session::PaintSession;
pub use export::{save_base_and_mask, ExportPaths};

// One-call host integration:
pub struct PaintHostBindings {
    pub on_submit_inpaint: Box<dyn Fn(ExportPaths) + Send + Sync>,
}

impl PaintSession {
    pub fn new(base_rgba: Vec<u8>, width: u32, height: u32) -> Self;
    pub fn ui(&mut self, ui: &mut egui::Ui, bindings: &PaintHostBindings);
    pub fn current_base(&self) -> &[u8];
    pub fn current_mask(&self) -> &[u8];
    pub fn export(&self, dir: &Path) -> std::io::Result<ExportPaths>;
}
```

## Risks / mitigations

| Risk | Mitigation |
|---|---|
| Drawing perf on large canvas | Phase 6: rayon brush apply. Phase 1-5 ship single-thread, profile under load |
| Hardness/opacity blending math wrong | Phase 2 has dedicated kernel/blend unit tests |
| egui Image widget zoom flickering | Use Mesh approach (UV-mapped quad) not Image widget — controlled redraw |
| Undo state size explosion | v1: 50 actions × full mask snapshot. Большой canvas (4096²) = 16MB × 50 = 800MB. Phase 6: diff-rect snapshots — 10×-100× компрессия |
| Inpaint provider API changes | Provider crate isolated; swap-out is contained |
| User saves PaintSession mid-stroke | Phase 4: serialize only completed strokes (after mouse-up). Discard in-flight strokes on reload |

## Что выйдет за scope этого плана

- Multi-frame editing (edit a range of frames consistently)
- Onion-skin / reference image overlay
- Animated brush strokes (record + replay)
- Vector layers / paths

Эти приходят постфактум когда (если) база заработает и UX
устоится. v1 — фокус на одно: «нарисовать маску → отправить →
получить результат как слой».
