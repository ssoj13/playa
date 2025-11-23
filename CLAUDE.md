# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Playa - кросс-платформенный image sequence player на Rust с OpenGL рендерингом, асинхронной загрузкой и видео-кодированием. Поддерживает EXR, PNG, JPEG, TIFF, видео через FFmpeg.

**Платформа**: Windows 11, PowerShell (НЕ bash)

## Build System

### Quick Commands

```powershell
# Базовая сборка (exrs backend, pure Rust)
.\bootstrap.ps1 build

# Полная сборка (OpenEXR C++ backend, DWAA/DWAB compression)
.\bootstrap.ps1 build --openexr

# Тесты
.\bootstrap.ps1 test

# Деплой в систему
cargo xtask deploy
```

### xtask - Build Automation

**Все команды сборки через xtask**. Это Rust workspace helper для кросс-платформенной автоматизации (замена Makefile).

Расположение: `xtask/src/main.rs`

Основные команды:
- `cargo xtask build [--release] [--openexr]` - Сборка проекта
- `cargo xtask post [--release]` - Копирование native библиотек (OpenEXR only)
- `cargo xtask verify [--release]` - Проверка зависимостей
- `cargo xtask test [--debug] [--nocapture]` - Запуск тестов
- `cargo xtask tag-dev [patch|minor|major]` - Dev tag для CI (v0.1.x-dev)
- `cargo xtask tag-rel [patch|minor|major]` - Release tag для production (v0.1.x)
- `cargo xtask pr [version]` - Создание PR: dev → main
- `cargo xtask wipe` - Очистка target/ от stale binaries

### EXR Backends

**По умолчанию**: `exrs` (pure Rust, быстрая сборка, НЕТ DWAA/DWAB)
**Опционально**: `openexr` (C++ backend, полная поддержка DWAA/DWAB, требует CMake)

Переключение через `--openexr` флаг в `cargo xtask build`.

### FFmpeg Integration

**Обязательно для видео**: vcpkg + FFmpeg с static linking

Проверка окружения:
```powershell
$env:VCPKG_ROOT        # c:\vcpkg
$env:VCPKGRS_TRIPLET   # x64-windows-static-md-release
$env:PKG_CONFIG_PATH   # %VCPKG_ROOT%\installed\%TRIPLET%\lib\pkgconfig
```

Bootstrap скрипт (`bootstrap.ps1`) автоматически настраивает окружение.

### Release Workflow

**Dev build** (тестирование на CI):
```powershell
cargo xtask tag-dev patch  # → v0.1.60-dev, запускает Build workflow
```

**Production release**:
```powershell
cargo xtask pr v0.1.60     # Создаёт PR: dev → main
# Merge PR на GitHub
git checkout main && git pull
cargo xtask tag-rel patch  # → v0.1.60, создаёт GitHub Release
```

## Architecture

### Core Components

```
PlayaApp (egui)
  ├─ Player (playback engine)
  │   └─ Project (scene container)
  │       ├─ Attrs (global settings)
  │       ├─ media: HashMap<UUID, Comp>
  │       └─ comps_order: Vec<UUID>
  │
  ├─ Comp (composition / clip)
  │   ├─ layers: Vec<Layer> (multi-layer support)
  │   ├─ cache: FrameCache (LRU + async loading)
  │   └─ compositor: CompositorType (CPU/GPU blending)
  │
  ├─ Workers (global thread pool)
  │   ├─ frame loading (75% CPU cores)
  │   └─ video encoding (background thread)
  │
  ├─ ViewportRenderer (OpenGL)
  │   └─ Shaders (GLSL shader pipeline)
  │
  └─ EventBus (application events)
      ├─ CompEvent (frame changes, layer updates)
      └─ HotkeyWindow (context routing)
```

### Key Modules

**`src/main.rs`**: Entry point, egui app loop, persistence (JSON)
**`src/player.rs`**: Playback state (JKL controls, FPS presets)
**`src/entities/project.rs`**: Top-level scene container (serialization unit)
**`src/entities/comp.rs`**: Composition (clip or multi-layer comp)
**`src/entities/compositor.rs`**: Frame blending engine (CPU/GPU)
**`src/entities/frame.rs`**: Individual frame (async loading, Arc<Mutex<FrameData>>)
**`src/entities/loader.rs`**: Format detection + loading (EXR/PNG/JPEG/TIFF via `image` crate)
**`src/entities/loader_video.rs`**: FFmpeg video decoding (via `playa-ffmpeg` crate)
**`src/widgets/viewport/`**: OpenGL rendering + viewport controls (zoom/pan)
**`src/widgets/timeline/`**: Custom timeline widget (sequence visualization, load indicator)
**`src/workers.rs`**: Global worker pool (rayon-based)
**`src/dialogs/encode/`**: Video encoding dialog (FFmpeg, hardware acceleration)

### Data Flow

1. **User loads file** → `Project::add_media()` → создаёт `Comp` (File mode)
2. **Comp инициализация** → `detect_sequences()` → создаёт `Layer` с `Sequence`
3. **Sequence scan** → `glob` pattern matching → список `Frame` (status: Header)
4. **Playback** → `Player::update()` → `Comp::current_frame()` → `cache.get(idx)`
5. **Cache miss** → Workers pool → `Frame::load()` → обновление status (Loading → Loaded)
6. **Rendering** → `ViewportRenderer::render()` → OpenGL texture upload → shader pipeline

### Caching Strategy

**FrameCache** (в каждом Comp):
- **LRU eviction**: Управление памятью (50% system RAM по умолчанию)
- **Epoch counter**: AtomicU64 для отмены stale requests при scrubbing
- **Worker pool**: 75% CPU cores для параллельной загрузки
- **Spiral preload**: Загрузка в порядке: 0, +1, -1, +2, -2...

## Development Notes

### Modern Rust Patterns Used

- **Workspace с xtask**: Build automation без внешних зависимостей
- **Arc<Mutex<T>>**: Thread-safe shared state для frames
- **crossbeam-channel**: Fast message passing для worker pool
- **egui immediate mode**: Stateless UI с persistence через serde
- **Edition 2024**: Использует последние фичи Rust

### Code Style

- **Короткие названия**: `get_tr()` вместо `extract_translation()`
- **Docstrings**: На все публичные функции, формат `//!` для module docs
- **Типизация**: Явные типы везде где неочевидно
- **Комментарии**: Concise, объясняют WHY а не WHAT
- **f-strings**: Всегда используем форматирование через `format!("{}", var)`

### Testing

```powershell
# Все тесты (unit + integration)
cargo xtask test

# Debug mode с выводом
cargo xtask test --debug --nocapture

# Один конкретный тест
cargo test test_name -- --nocapture
```

Integration tests в `tests/` проверяют:
- Sequence detection
- Frame caching
- Video encoding
- Multi-layer compositing

### Common Pitfalls

**НЕ используй bash на Windows**:
```powershell
# ПРАВИЛЬНО
pwsh.exe -Command "Get-ChildItem"

# НЕПРАВИЛЬНО (не работает на Windows)
bash -c "ls"
```

**Пути в Windows**:
```rust
// ПРАВИЛЬНО - используй PathBuf
use std::path::PathBuf;
let path = PathBuf::from(r"C:\projects\playa");

// НЕПРАВИЛЬНО - хардкодинг слешей
let path = "/c:/projects/playa"; // Не работает!
```

**OpenEXR headers патчинг** (Linux only):
```bash
cargo xtask pre  # Добавляет #include <cstdint> для GCC 11+
```

## CI/CD

### GitHub Actions Workflows

**`.github/workflows/main.yml`**: Unified Release workflow
- Триггер: git tags `v*`
- Branch detection: `check-branch` job определяет release vs dev
- Кэширование: `wait-for-cache` ждёт warm-cache workflow

**`.github/workflows/warm-cache.yml`**: Vcpkg cache warming
- Запускается перед main.yml для подготовки FFmpeg/OpenEXR
- Saves cache для ускорения build jobs

**`.github/workflows/_build-platform.yml`**: Reusable build workflow
- Windows: NSIS installer + MSI
- Linux: AppImage + DEB package
- macOS: DMG (code-signed + notarized)

### Build Artifacts

**Release builds** (tag на main):
- OpenEXR backend (DWAA/DWAB support)
- Code signing для macOS (Developer ID)
- Публикация на GitHub Releases

**Dev builds** (tag с `-dev` suffix):
- Оба backend (exrs + OpenEXR)
- Артефакты в Actions (НЕ GitHub Release)

## External Dependencies

**vcpkg packages**:
- `ffmpeg[core,avcodec,avformat,swscale,nvcodec]` - Video support
- `openexr`, `imath`, `zlib` - OpenEXR backend (optional)

**Rust crates**:
- `egui` 0.33 + `eframe` - UI framework
- `image` 0.25 - Image I/O (EXR via `exrs`, PNG/JPEG/TIFF)
- `playa-ffmpeg` 8.0.3 - FFmpeg bindings (custom fork)
- `rayon` - Data parallelism
- `crossbeam` - Lock-free concurrency

## Project Structure

```
playa/
├── src/
│   ├── main.rs              # Entry point, app state
│   ├── player.rs            # Playback engine
│   ├── entities/            # Core data types (Project, Comp, Frame)
│   ├── widgets/             # UI components (viewport, timeline, status)
│   ├── dialogs/             # Settings, encoding
│   ├── workers.rs           # Thread pool
│   └── events.rs            # Event bus
├── xtask/                   # Build automation helper
├── tests/                   # Integration tests
├── bootstrap.ps1            # Windows bootstrap script
├── Cargo.toml               # Workspace config
└── .github/workflows/       # CI/CD pipelines
```

## Persistence

**Settings**: `AppSettings` → JSON в config dir (via `dirs-next`)
**Project**: `Project` → JSON (`playa.json` по умолчанию)
**State**: egui persistence для viewport/timeline state

Config directory:
- Windows: `%APPDATA%\playa\`
- Linux: `~/.config/playa/`
- macOS: `~/Library/Application Support/playa/`
