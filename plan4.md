# План восстановления Memory Management + Auto-caching

## Проблема

У нас нет memory management - можем переполнить память если небрежно играть.

Нужно:
1. LRU в Comp.cache
2. Timeline indicator (синий/желтый/зеленый статус кадров)
3. Алгоритм автокеширования с двумя стратегиями (spiral/forward)
4. Session/epoch mechanism для отмены устаревших команд
5. Настройка "Reserve N GB for system"

## Архитектура из .orig

### Memory Tracking (`.orig/src/cache.rs`)
```rust
memory_usage: Arc<AtomicUsize>,      // Текущее использование
max_memory_bytes: usize,             // Лимит памяти
```

- При загрузке: `memory_usage.fetch_add(frame_size, Ordering::Relaxed)` (строка 728)
- Метод `mem()` возвращает `(usage, max)` (строки 770-773)

### Session/Epoch Mechanism (`.orig/src/cache.rs`)
```rust
current_epoch: Arc<AtomicU64>,       // Счётчик сессии
```

- При изменении времени: `epoch += 1` (строки 297-298)
- Каждый `LoadRequest` содержит `epoch: u64` (строка 44)
- Воркеры проверяют: `if req.epoch != current_epoch { continue; }` (строка 217)
- **Результат:** старые команды автоматически игнорируются

### Preload Strategies (`.orig/src/cache.rs:309-329`)

**Spiral** (для image sequences):
```rust
// 0, +1, -1, +2, -2, +3, -3, ...
for offset in 0..=max_offset {
    send_load(center - offset);
    send_load(center + offset);
}
```

**Forward-only** (для video):
```rust
// center, center+1, center+2, ...
for idx in center..=end {
    send_load(idx);
}
```

Определяется автоматически: `media::is_video(path)` (строки 310-317)

### Timeline Status Indicator (`.orig/src/timeslider.rs:143-149`)

```rust
fn draw_load_indicator(painter, rect, statuses: &[FrameStatus], height: f32) {
    for (idx, status) in statuses.iter().enumerate() {
        let color = match status {
            Placeholder | Header => BLUE,    // Незагружен
            Loading => YELLOW,               // Загрузка
            Loaded => GREEN,                 // Готов
            Error => RED,                    // Ошибка
        };
        painter.rect_filled(x, y, width, height, color);
    }
}
```

Кешируется с тремя ключами:
- `cached_count` (количество загруженных)
- `loaded_events` (монотонный счётчик событий)
- `sequences_version` (версия плейлиста)

---

## План реализации

### ✅ 1. Настройки памяти (`src/dialogs/prefs/prefs.rs`)

**Добавить в `AppSettings`:**
```rust
pub cache_memory_percent: f32,     // 25.0-95.0% (default 75%)
pub reserve_system_memory_gb: f32, // Минимум для системы (default 2.0)
```

**UI:**
```rust
ui.heading("Cache & Memory");
ui.add_space(8.0);

ui.label("Cache Memory Limit (% of available):");
ui.add(
    egui::Slider::new(&mut settings.cache_memory_percent, 25.0..=95.0)
        .suffix("%")
        .step_by(5.0)
);

ui.label("Reserve for System (GB):");
ui.add(
    egui::Slider::new(&mut settings.reserve_system_memory_gb, 0.5..=8.0)
        .suffix(" GB")
        .step_by(0.5)
);
```

**Расчёт лимита:**
```rust
let available = sysinfo::System::available_memory() as usize;
let reserve = (reserve_system_memory_gb * 1024.0 * 1024.0 * 1024.0) as usize;
let usable = available.saturating_sub(reserve);
let max_memory_bytes = (usable as f64 * cache_memory_percent / 100.0) as usize;
```

---

### ✅ 2. Cache Manager (`src/cache_manager.rs` - новый модуль)

```rust
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PreloadStrategy {
    Spiral,   // Для image sequences: 0, ±1, ±2, ...
    Forward,  // Для video: center → end
}

pub struct CacheManager {
    memory_usage: Arc<AtomicUsize>,      // Текущее использование
    max_memory_bytes: usize,             // Лимит
    current_epoch: Arc<AtomicU64>,       // Счётчик сессии
    preload_strategy: PreloadStrategy,    // Spiral или Forward
}

impl CacheManager {
    pub fn new(mem_fraction: f64, reserve_gb: f64) -> Self {
        let mut sys = sysinfo::System::new_all();
        sys.refresh_memory();

        let available = sys.available_memory() as usize;
        let reserve = (reserve_gb * 1024.0 * 1024.0 * 1024.0) as usize;
        let usable = available.saturating_sub(reserve);
        let max_memory_bytes = (usable as f64 * mem_fraction) as usize;

        log::info!(
            "CacheManager: available={} MB, reserve={} MB, limit={} MB",
            available / 1024 / 1024,
            reserve / 1024 / 1024,
            max_memory_bytes / 1024 / 1024
        );

        Self {
            memory_usage: Arc::new(AtomicUsize::new(0)),
            max_memory_bytes,
            current_epoch: Arc::new(AtomicU64::new(0)),
            preload_strategy: PreloadStrategy::Spiral,
        }
    }

    /// Инкрементирует epoch и возвращает новое значение
    pub fn increment_epoch(&self) -> u64 {
        self.current_epoch.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Получить текущий epoch
    pub fn current_epoch(&self) -> u64 {
        self.current_epoch.load(Ordering::Relaxed)
    }

    /// Проверка превышения лимита памяти
    pub fn check_memory_limit(&self) -> bool {
        self.memory_usage.load(Ordering::Relaxed) > self.max_memory_bytes
    }

    /// Получить статистику памяти
    pub fn mem(&self) -> (usize, usize) {
        let usage = self.memory_usage.load(Ordering::Relaxed);
        (usage, self.max_memory_bytes)
    }

    /// Добавить использованную память
    pub fn add_memory(&self, bytes: usize) {
        self.memory_usage.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Освободить память
    pub fn free_memory(&self, bytes: usize) {
        self.memory_usage.fetch_sub(bytes, Ordering::Relaxed);
    }

    /// Установить стратегию preload (на основе типа файла)
    pub fn set_strategy(&mut self, is_video: bool) {
        self.preload_strategy = if is_video {
            PreloadStrategy::Forward
        } else {
            PreloadStrategy::Spiral
        };
    }

    /// Получить текущую стратегию
    pub fn strategy(&self) -> PreloadStrategy {
        self.preload_strategy
    }
}
```

**Добавить в `src/lib.rs`:**
```rust
pub mod cache_manager;
```

---

### ✅ 3. Workers + Epoch (`src/workers.rs`)

**Обновить структуры:**
```rust
pub struct LoadRequest {
    pub frame: Frame,
    pub comp_uuid: String,
    pub frame_idx: usize,
    pub epoch: u64,  // ← Новое!
}

pub struct Workers {
    sender: Sender<Job>,
    _handles: Vec<thread::JoinHandle<()>>,
    current_epoch: Arc<AtomicU64>,  // ← Новое!
}

impl Workers {
    pub fn new(num_threads: usize, epoch: Arc<AtomicU64>) -> Self {
        // ... создание воркеров

        Self {
            sender: tx,
            _handles: handles,
            current_epoch: epoch,
        }
    }

    /// Отправить запрос на загрузку (с проверкой epoch)
    pub fn load_frame(&self, req: LoadRequest) {
        let current = self.current_epoch.load(Ordering::Relaxed);

        if req.epoch != current {
            log::debug!(
                "Skipping stale load request: epoch {} != current {}",
                req.epoch,
                current
            );
            return;
        }

        self.execute(move || {
            // Загрузка фрейма
            if let Err(e) = req.frame.set_status(FrameStatus::Loaded) {
                log::warn!("Failed to load frame: {}", e);
            }
        });
    }
}
```

**Воркеры проверяют epoch при обработке:**
```rust
// В worker loop
loop {
    let req = rx.recv()?;

    // Проверка epoch
    if req.epoch != current_epoch.load(Ordering::Relaxed) {
        log::debug!("Worker: skipping stale request (epoch mismatch)");
        continue;
    }

    // Загрузка...
}
```

---

### ✅ 4. Preload логика в Comp (`src/entities/comp.rs`)

**Добавить поля:**
```rust
pub struct Comp {
    // ... существующие поля
    cache_manager: Arc<CacheManager>,  // Ссылка на глобальный manager
}
```

**Метод `signal_preload()`:**
```rust
impl Comp {
    /// Триггерит загрузку кадров вокруг курсора
    pub fn signal_preload(&mut self, workers: &Workers) {
        let center = self.current_frame as usize;
        let start = self.play_start() as usize;
        let end = self.play_end() as usize;

        // Инкрементируем epoch (отменяет старые команды)
        let new_epoch = self.cache_manager.increment_epoch();

        log::debug!(
            "Preload: epoch={}, center={}, range={}..{}",
            new_epoch,
            center,
            start,
            end
        );

        // Определяем стратегию (spiral для images, forward для video)
        let is_video = self.detect_video_at_frame(center);
        self.cache_manager.set_strategy(is_video);

        match self.cache_manager.strategy() {
            PreloadStrategy::Spiral => {
                self.preload_spiral(workers, center, start, end, new_epoch);
            }
            PreloadStrategy::Forward => {
                self.preload_forward(workers, center, start, end, new_epoch);
            }
        }
    }

    /// Spiral preload: 0, +1, -1, +2, -2, ...
    fn preload_spiral(&self, workers: &Workers, center: usize, start: usize, end: usize, epoch: u64) {
        let max_offset = (end - start) / 2;

        for offset in 0..=max_offset {
            // Backward
            if center >= offset {
                let idx = center - offset;
                if idx >= start && idx <= end {
                    self.request_frame_load(workers, idx, epoch);
                }
            }

            // Forward (skip offset=0 as already loaded)
            if offset > 0 {
                let idx = center + offset;
                if idx <= end {
                    self.request_frame_load(workers, idx, epoch);
                }
            }
        }
    }

    /// Forward-only preload: center, center+1, center+2, ...
    fn preload_forward(&self, workers: &Workers, center: usize, start: usize, end: usize, epoch: u64) {
        for idx in center..=end {
            self.request_frame_load(workers, idx, epoch);
        }
    }

    /// Отправка запроса на загрузку (если не загружен)
    fn request_frame_load(&self, workers: &Workers, frame_idx: usize, epoch: u64) {
        if let Some(frame) = self.get_frame(frame_idx) {
            let status = frame.status();

            if status == FrameStatus::Placeholder || status == FrameStatus::Header {
                workers.load_frame(LoadRequest {
                    frame: frame.clone(),
                    comp_uuid: self.uuid.clone(),
                    frame_idx,
                    epoch,
                });
            }
        }
    }

    /// Определяет является ли текущий кадр видео
    fn detect_video_at_frame(&self, frame_idx: usize) -> bool {
        if let Some(frame) = self.get_frame(frame_idx) {
            if let Some(path) = frame.file() {
                return crate::utils::media::is_video(path);
            }
        }
        false
    }
}
```

---

### ✅ 5. LRU в Comp.cache (`src/entities/comp.rs`)

**Добавить зависимость в `Cargo.toml`:**
```toml
lru = "0.12"
```

**Заменить HashMap на LRU:**
```rust
use lru::LruCache;
use std::num::NonZeroUsize;

pub struct Comp {
    // ... существующие поля

    // Заменить:
    // cache: RefCell<HashMap<(u64, usize), Frame>>,

    // На:
    cache: RefCell<LruCache<(u64, usize), Frame>>,
    cache_manager: Arc<CacheManager>,
}

impl Comp {
    pub fn new(..., cache_manager: Arc<CacheManager>) -> Self {
        Self {
            // ...
            cache: RefCell::new(LruCache::new(NonZeroUsize::new(10000).unwrap())),
            cache_manager,
        }
    }

    /// Получить кадр с проверкой memory limit
    pub fn get_frame(&self, frame_idx: usize) -> Result<Frame, String> {
        let comp_hash = self.compute_comp_hash();
        let key = (comp_hash, frame_idx);

        // Проверяем кеш
        if let Some(frame) = self.cache.borrow_mut().get(&key) {
            return Ok(frame.clone());
        }

        // Загружаем новый кадр
        let frame = match self.mode {
            CompMode::File => self.get_file_frame(frame_idx)?,
            CompMode::Layer => self.get_layer_frame(frame_idx)?,
        };

        let frame_size = frame.mem();

        // LRU eviction при превышении лимита
        while self.cache_manager.check_memory_limit() {
            if let Some((_, evicted_frame)) = self.cache.borrow_mut().pop_lru() {
                let evicted_size = evicted_frame.mem();
                self.cache_manager.free_memory(evicted_size);

                // Опционально: выгрузить frame data
                let _ = evicted_frame.set_status(FrameStatus::Header);

                log::debug!("LRU evicted frame: freed {} MB", evicted_size / 1024 / 1024);
            } else {
                break;
            }
        }

        // Добавляем в кеш
        self.cache.borrow_mut().put(key, frame.clone());
        self.cache_manager.add_memory(frame_size);

        Ok(frame)
    }

    /// Очистка кеша с освобождением памяти
    pub fn clear_cache(&mut self) {
        let mut cache = self.cache.borrow_mut();

        // Освобождаем память
        for (_, frame) in cache.iter() {
            let size = frame.mem();
            self.cache_manager.free_memory(size);
        }

        cache.clear();
    }
}
```

---

### ✅ 6. Timeline Status Indicator (`src/widgets/timeline/timeline_ui.rs`)

**Добавить метод в Comp:**
```rust
impl Comp {
    /// Получить статусы всех кадров для timeline indicator
    pub fn get_frame_statuses(&self) -> Vec<FrameStatus> {
        let start = self.start() as usize;
        let end = self.end() as usize;
        let mut statuses = Vec::with_capacity(end - start + 1);

        for idx in start..=end {
            if let Some(frame) = self.get_frame(idx) {
                statuses.push(frame.status());
            } else {
                statuses.push(FrameStatus::Placeholder);
            }
        }

        statuses
    }
}
```

**Добавить функцию отрисовки:**
```rust
/// Рисует индикатор статуса загрузки кадров (тонкая полоска)
fn draw_load_indicator(
    painter: &egui::Painter,
    rect: Rect,
    statuses: &[FrameStatus],
    height: f32,
) {
    if statuses.is_empty() {
        return;
    }

    let bar_rect = Rect::from_min_size(
        Pos2::new(rect.min.x, rect.max.y - height),
        Vec2::new(rect.width(), height),
    );

    let pixel_per_frame = rect.width() / statuses.len() as f32;

    for (idx, status) in statuses.iter().enumerate() {
        let color = match status {
            FrameStatus::Placeholder | FrameStatus::Header => Color32::from_rgb(50, 100, 200),  // Blue
            FrameStatus::Loading => Color32::from_rgb(200, 200, 50),                           // Yellow
            FrameStatus::Loaded => Color32::from_rgb(50, 200, 100),                            // Green
            FrameStatus::Error => Color32::from_rgb(200, 50, 50),                              // Red
        };

        let x = rect.min.x + idx as f32 * pixel_per_frame;
        let frame_rect = Rect::from_min_size(
            Pos2::new(x, bar_rect.min.y),
            Vec2::new(pixel_per_frame.max(1.0), height),
        );

        painter.rect_filled(frame_rect, 0.0, color);
    }
}
```

**Кеширование статусов:**
```rust
#[derive(Clone, Debug)]
struct LoadIndicatorCache {
    statuses: Vec<FrameStatus>,
    loaded_events: usize,     // Monotonic counter
    cache_version: u64,       // Comp cache version
}

// В render_timeline():
let cache_id = ui.id().with("load_indicator_cache");
let current_loaded_events = comp.loaded_events_counter();
let current_cache_version = comp.cache_version();

let cached_statuses = ui.ctx().memory_mut(|mem| {
    let stored: Option<LoadIndicatorCache> = mem.data.get_temp(cache_id);

    match stored {
        Some(cached)
            if cached.loaded_events == current_loaded_events
                && cached.cache_version == current_cache_version =>
        {
            cached.statuses
        }
        _ => {
            let statuses = comp.get_frame_statuses();
            mem.data.insert_temp(
                cache_id,
                LoadIndicatorCache {
                    statuses: statuses.clone(),
                    loaded_events: current_loaded_events,
                    cache_version: current_cache_version,
                },
            );
            statuses
        }
    }
});

// Рисуем полоску
draw_load_indicator(&painter, ruler_rect, &cached_statuses, 4.0);
```

---

### ✅ 7. Интеграция в main.rs

**Обновить `PlayaApp`:**
```rust
struct PlayaApp {
    // ... существующие поля
    cache_manager: Arc<CacheManager>,
}

impl PlayaApp {
    fn new(cc: &CreationContext) -> Self {
        // Создаём глобальный cache manager
        let cache_manager = Arc::new(CacheManager::new(0.75, 2.0));

        // Создаём workers с shared epoch
        let workers = Arc::new(Workers::new(
            num_threads,
            Arc::clone(&cache_manager.current_epoch),
        ));

        // ...

        Self {
            cache_manager,
            workers,
            // ...
        }
    }
}
```

**При изменении текущего времени:**
```rust
impl PlayaApp {
    fn set_current_time(&mut self, time: i32) {
        // Устанавливаем время
        self.comp.set_current_frame(time);

        // Триггерим preload с новым epoch
        self.comp.signal_preload(&self.workers);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Обновляем memory limit из настроек
        let settings_mem_percent = self.settings.cache_memory_percent;
        let settings_reserve_gb = self.settings.reserve_system_memory_gb;

        if self.applied_mem_fraction != settings_mem_percent as f64 / 100.0 {
            let new_manager = Arc::new(CacheManager::new(
                settings_mem_percent as f64 / 100.0,
                settings_reserve_gb as f64,
            ));
            self.cache_manager = new_manager;
            self.applied_mem_fraction = settings_mem_percent as f64 / 100.0;
        }

        // ... остальная логика
    }
}
```

---

## Файлы для изменения

### Новые файлы
- [ ] `src/cache_manager.rs` - глобальный memory/epoch manager

### Изменения
- [ ] `src/dialogs/prefs/prefs.rs` - добавить настройки памяти
- [ ] `src/workers.rs` - добавить epoch в LoadRequest и Workers
- [ ] `src/entities/comp.rs` - LRU cache + signal_preload()
- [ ] `src/widgets/timeline/timeline_ui.rs` - draw_load_indicator()
- [ ] `src/main.rs` - создание CacheManager и интеграция
- [ ] `src/lib.rs` - экспорт cache_manager модуля
- [ ] `Cargo.toml` - добавить `lru = "0.12"`

---

## Приоритеты

### Phase 1: Foundation (критично)
1. ✅ CacheManager создание
2. ✅ Настройки памяти (prefs.rs)
3. ✅ Workers + epoch mechanism

### Phase 2: Core Features
4. ✅ LRU в Comp.cache
5. ✅ Preload logic (spiral/forward)
6. ✅ signal_preload() интеграция

### Phase 3: UI Polish
7. ✅ Timeline status indicator
8. ✅ Memory stats в status bar

---

## Тестирование

### Проверки
- [ ] Memory limit соблюдается (не превышает лимит)
- [ ] LRU eviction работает корректно
- [ ] Epoch отменяет устаревшие загрузки при быстром scrubbing
- [ ] Spiral strategy для image sequences
- [ ] Forward strategy для video
- [ ] Timeline indicator обновляется корректно
- [ ] Настройки памяти применяются без перезапуска

### Тест-кейсы
1. Загрузить 4K EXR sequence (большие фреймы) → должен автоматом выгружать LRU
2. Быстрый scrub по timeline → старые запросы должны отменяться
3. Изменить memory limit в настройках → должен пересчитать лимит
4. Смешанный проект (video + images) → правильная стратегия для каждого

---

## Референсы из .orig

### Ключевые строки
- **Memory tracking:** `.orig/src/cache.rs:91-92, 728, 771-772`
- **Epoch mechanism:** `.orig/src/cache.rs:104, 191, 297-298, 217`
- **Spiral strategy:** `.orig/src/cache.rs:374-411`
- **Forward strategy:** `.orig/src/cache.rs:354-373`
- **Timeline indicator:** `.orig/src/timeslider.rs:143-149`
- **Unload outside range:** `.orig/src/cache.rs:574-597`

### Код для копирования
- `CacheManager::new()` → адаптировать из `.orig/src/cache.rs:146-456`
- Spiral/Forward loops → копировать напрямую
- `draw_load_indicator()` → адаптировать из `.orig/src/timeslider.rs`
