# playa-app — notebook

> Записывай сюда всю значимую инфу об этом крейте: что это, как устроено,
> ловушки, и TODO с пометками `[ ]`/`[x]`. Обновляй по мере работы.

## Что это
Top-level egui/eframe приложение playa: `PlayaApp` (serde-persisted, NOT Clone),
док-лейаут (egui_dock), вкладки (viewport / timeline / node-editor / project /
attribute-editor), event-bus диспетч, runner (eframe creation closure + wgpu),
встроенный REST API сервер.

## Ключевые места
- `src/app/mod.rs` — `PlayaApp` (поля + serde). `viewport_renderer: Arc<Mutex<HdrView>>`
  (egui-hdr-view), `node_editor_state` (nodes-rs), `attributes_state`, и т.д.
- `src/runner.rs` — eframe creation closure: phosphor-шрифт, `HdrView::configure_wgpu_render_state`,
  `node_editor_state.configure_wgpu_render_state`, GPU blend init, REST `start_api_server`.
- `src/app/run.rs` — per-frame update; `set_output_format` для viewport; teardown (`destroy`).
- `src/app/tabs.rs` / `events.rs` / `layout.rs` / `main_events.rs` — рендер вкладок,
  обработка событий, лейаут.

## Виджеты — потребляются из egui-widgets-rs / nodes-rs (git-ref)
playa мигрирована на общие крейты (Track M): progressbar, help-overlay, statusbar,
prefs, jobs-table, attr-grid, hdr-view, asset-browser, encode-dialog, track-timeline,
timeline, gizmo + node-editor на nodes-rs. См. корневой `crates/playa-ui` и
`egui-widgets-rs`. Все pin'ы egui 0.34 / wgpu 29 / glam 0.33 общие.

## TODO
- [ ] **Заменить `rouille` (REST API сервер)** — тянет `multipart 0.18` → `buf_redux 0.8.4`,
      обе помечены `future-incompatibilities` (будут отклонены будущим rustc). `rouille 3.6.2`
      (последняя) держит `multipart` в ядре без opt-out feature. Кандидаты на замену:
      `axum` (+ tokio) или `tiny_http` (легче, sync, ближе к текущей модели rouille).
      Проверить: `cargo tree -i multipart`. Найти REST-эндпоинты (`start_api_server`) и
      перенести роутинг. Отдельный PR.
- [ ] **Runtime-смоки после Track M миграции** (интерактив/GPU, compile-green не покрывает):
      node-editor (граф рисуется), viewport (картинка + exposure/tonemap), timeline
      (move/trim/slide/drop/bookmarks/dive/ctrl-select), gizmo (move/rotate/scale + snap).

## Ловушки
- `PlayaApp` — `Serialize`/`Deserialize`, НО НЕ `Clone`. wgpu-ресурсы (`HdrView`,
  node-runtime) держать за `#[serde(skip)]`.
- Сборка только через `python bootstrap.py b --debug` (ставит vcpkg/MSVC env для
  ffmpeg-sys). Bare `cargo build/test` падает на `ffmpeg-sys-next` build-скрипте.
  `cargo xtask test` гоняет `--workspace` и сейчас упирается в pre-existing gap
  примера `playa-ffmpeg` (нужна gated `filter` feature).
