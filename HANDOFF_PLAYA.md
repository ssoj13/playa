# HANDOFF — playa cache & display pipeline issues

**Investigated**: 2026-05-11 by vfx-rs agent.
**Symptoms reported**: (1) "куски сканлайнов перепутаны блоками, как помехи старого ТВ"; (2) cache не загружает весь сиквенс; (3) серый экран при autoplay.

## TL;DR

Три независимых бага в playa, **vfx-rs read path чист**:

1. **Decode bottleneck — vfx-io fix coming**: vfx-io.exr.read_layers сейчас декодит ВСЕ каналы (43 для V-Ray AOV → 158MB uncompressed/frame → 254ms средний). vfx-rs готовит API с `layers: Option<Vec<String>>` фильтром. После landing в vfx-rs main и bump зависимости, изменить вызов `ExrReader::new()` на `ExrReader::with_layers(["default"])` (или каков нужный AOV) для viewport — уберёт ~70-80% per-frame decode time.

2. **Cache key bifurcation**: `FileNode.compute()` пишет под `file_node.uuid()` (file_node.rs:202), `CompNode.compute()` пишет под `self.uuid()` (comp_node.rs:1624). Один и тот же кадр живёт под двумя ключами. 17 entries в логе = ~8 кадров с обеими копиями. Это удваивает память и не помогает.

3. **Hit rate 0.1%**: `comp_node.compute()` (line 1597-1605) вычисляет `is_dirty()` на каждом тике и при dirty=true считает `needs_recompute = true` → cache miss recorded → 1792 misses за 9 секунд при 17 entries. Подозрение: `is_dirty()` returns true чаще чем должно.

## Repro

```powershell
playa.exe -vv -l playa.log --autoplay <path-to-multi-AOV-EXR-sequence>
```

Note: `-l` без значения **сломан** в текущем CLI — clap quirk. `Option<Option<PathBuf>>` без `num_args = 0..=1` + `default_missing_value` → `-l` требует value. Quick fix in `crates/playa-app/src/cli.rs:65-66`:

```rust
#[arg(
    short = 'l',
    long = "log",
    value_name = "LOG_FILE",
    num_args = 0..=1,
    default_missing_value = "playa.log",
)]
pub log_file: Option<Option<PathBuf>>,
```

## Diagnostic evidence (от vfx-rs agent)

### Что vfx-rs read path ВЫДАЁТ (тестовый файл `D:\_demo\Srcs\Robat\robat.0001.exr`)

```
$ cargo run -p vfx-exr --example probe_scanline_parity --release -- robat.0001.exr
... все 43 channel hashes match between parallel and serial decode (bit-exact)
```

Probe прогнан на 7 кадрах сиквенса (0001, 0010, 0020, 0030, 0040, 0050, 0063) — все clean. Гипотеза "rayon race condition" опровергнута.

### Per-frame timing (vfx-io.exr.read_layers)

```
$ cargo run -p vfx-io --example bench_read --release --features exr -- robat.0001.exr 10
 390ms  robat.0001.exr  1280x720  43 channels  1 layers
 312ms  robat.0002.exr  1280x720  43 channels  1 layers
 ...
 avg=254ms  min=171ms  max=390ms  total=2548ms over 10 frames
```

С 254ms/frame пути нет к 24fps playback (нужно <42ms). Серый экран = playa requests faster than vfx-io can produce.

### Cache stats (из playa.log)

```
[INFO  playa_engine::core::cache_man] CacheManager init: limit=27717 MB (75%)
[INFO  playa_app::app::run] Cache stats: 17 entries | hits: 1 | misses: 1792 | hit rate: 0.1%
```

Лимит огромный, но cache почти пустой → не нагрузка памяти лимитирует, а decode bottleneck (#1) и логика recompute (#3).

### "Блоки сканлайнов перепутаны как помехи ТВ"

Гипотеза: при таком темпе декода viewport показывает кадр частично загруженный (верх свежий, низ от предыдущего frame в той же GPU текстуре). Это **partial texture upload bug** или **race condition между ImageLayer.pixels и GPU upload**. После фикса bottleneck #1 симптом скорее всего исчезнет (frames будут готовы целиком до upload). Если остаётся — копать в `playa-engine/src/render_gpu/` где идёт `ImageLayer → texture_2d` upload — убедиться, что `queue.write_texture` не вызывается с torn buffer.

## Action items для playa-агента

В порядке приоритета:

### P0 — Decode speed (waiting on vfx-rs)

После того как vfx-rs смерджит `layers` filter в main + bump зависимости:

```rust
// crates/playa-io/src/source_image/native.rs:15
let reader = vfx_io::exr::ExrReader::with_layers(["default"]);
//   или
//   let reader = vfx_io::exr::ExrReader::with_options(ExrReaderOptions {
//       layers: Some(vec!["default".into()]),
//       ..Default::default()
//   });
```

Для multi-AOV display (когда user явно открыл AOV layer) — передавать имя выбранного слоя. По умолчанию для viewport — только default RGB+A.

### P1 — `-l` CLI fix

`crates/playa-app/src/cli.rs:65-66` — добавить `num_args = 0..=1` + `default_missing_value`. См. сниппет выше.

### P2 — Cache key consistency

`crates/playa-engine/src/core/global_cache.rs` + `entities/file_node.rs` + `entities/comp_node.rs`. Решить: либо single cache key per (comp_uuid, frame_idx) с file данные хранятся как side-effect через FileNode-internal cache, либо single key per (source_uuid, frame_idx) с comp как тонкая обёртка. Сейчас гибрид удваивает память без выигрыша.

### P3 — Recompute loop

`comp_node.rs:1597-1605`. `is_dirty()` возвращает true чаще чем должно — по логу хит-рейт 0.1% при 17 cached entries. Логика: `needs_recompute = is_dirty || cached_frame.is_none() || cache_is_loading`. Подозрение что `is_dirty()` для preview comp возвращает true каждый тик из-за того что `attrs.is_dirty()` где-то не сбрасывается. Pinpoint через `trace!("compute() dirty: ...")` уже в коде — включи `RUST_LOG=playa_engine::entities::comp_node=trace`.

### P4 — Investigate "blocks shuffled" if persists after P0

Если после P0 (decode 5x faster) серый экран ушёл, но "помехи ТВ" остались — копай GPU upload path: `crates/playa-engine/src/render_gpu/`. Проверь что `queue.write_texture(...)` для каждого frame идёт с **полностью валидным buffer**, а не partial-decoded. Возможный bug: shared `Frame.pixels: Arc<...>` модифицируется во время `write_texture` — race с decoder thread.

## Что vfx-rs готовит для playa-side

- **Now**: `vfx_io::exr::ExrReader::with_layers(...)` + `ExrReaderOptions { layers: Option<Vec<String>>, .. }`. Layer name semantics: `""` или `"default"` = channels без `.` (R/G/B/A); `"diffuse"` = `diffuse.*`; etc. Empty filter / None = read all (backward-compat).
- **Bench tool**: `cargo run -p vfx-io --example bench_read --release --features exr -- <first.exr> <count>` — для regression timing.
- **Parity probe**: `cargo run -p vfx-exr --example probe_scanline_parity --release -- <file.exr>` — verify decode bit-exact на любом EXR.

## Не баг

- 18 V-Ray AOV каналов (lighting/masks/materialID/multimatte/specular) с одинаковым hash — это нормально, эти AOV в этом кадре заполнены нулями. Не путать с decode bug.
- vfx-exr parallel decode race condition — опровергнуто, parallel == serial bit-exact.

---

End of handoff.
