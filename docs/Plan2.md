# План добавления Video Encoding в Playa (v2)

## Часть 1: Рабочая область (Work Area / Play Range)

### 1.1 Добавить play_range в Cache
- **Файл**: `src/cache.rs`
- **Добавить**:
  ```rust
  pub struct Cache {
      // ... existing fields
      play_range_start: AtomicUsize,  // Persistent
      play_range_end: AtomicUsize,    // Persistent
  }
  ```
- **Методы**:
  - `set_play_range(start: usize, end: usize)` - валидация (start <= end, в пределах global range)
  - `get_play_range() -> (usize, usize)`
  - `reset_play_range()` - установить на весь диапазон
- **Сериализация**: Добавить `play_range_start`, `play_range_end` в `CacheState` для persistent storage

### 1.2 Обновить Player для play_range
- **Файл**: `src/player.rs`
- **Изменить** `advance_frame()`:
  - При достижении `play_range_end` → loop к `play_range_start` (если loop_enabled)
  - Иначе stop на `play_range_end`
- **Методы**: Нет, используем напрямую `cache.set_play_range()`

### 1.3 Обновить TimeSlider UI
- **Файл**: `src/timeslider.rs`
- **Визуализация**:
  - Нарисовать серый прямоугольник посередине слайдера (50% высоты)
  - От `play_range_start` до `play_range_end`
- **Кнопки B / N**:
  - B (Begin) → `set_play_area_start(current_frame)`
  - N (eNd) → `set_play_area_end(current_frame)`
- **Возвращать** в `TimeSliderActions`:
  - `set_play_area_start: Option<usize>`
  - `set_play_area_end: Option<usize>`
- **Внутри**: При получении action вызвать `cache.set_play_range()`

### 1.4 Добавить в Help Screen
- **Файл**: `src/ui.rs` (функция `render_help_window`)
- **Добавить**:
  ```
  B - Set work area start (Begin)
  N - Set work area end (eNd)
  F7 - Open video encoder
  ```

---

## Часть 2: Video Encoding Module

### 2.1 Создать src/encode.rs
- **Модуль**: Отдельный файл с encoder логикой
- **Структуры**:
  ```rust
  #[derive(Clone, Debug, Serialize, Deserialize)]
  pub struct EncoderSettings {
      pub output_path: PathBuf,
      pub container: Container,        // MP4 или MOV
      pub codec: VideoCodec,            // H264, H265, ProRes
      pub encoder_impl: EncoderImpl,    // Auto/Hardware/Software
      pub quality_mode: QualityMode,    // CRF/Bitrate
      pub quality_value: u32,           // CRF 18-28 или bitrate
  }

  #[derive(Clone, Copy, Debug, Serialize, Deserialize)]
  pub enum Container { MP4, MOV }

  #[derive(Clone, Copy, Debug, Serialize, Deserialize)]
  pub enum VideoCodec { H264, H265, ProRes }

  #[derive(Clone, Copy, Debug, Serialize, Deserialize)]
  pub enum EncoderImpl {
      Auto,         // Попробовать HW → fallback CPU
      Hardware,     // NVENC/QSV/AMF/VideoToolbox only
      Software,     // libx264/libx265/prores_ks only
  }

  #[derive(Clone, Copy, Debug, Serialize, Deserialize)]
  pub enum QualityMode { CRF, Bitrate }

  pub struct EncodeProgress {
      pub current_frame: usize,
      pub total_frames: usize,
      pub stage: EncodeStage,
      pub error: Option<String>,
  }

  pub enum EncodeStage {
      Validating,      // Проверка размеров frames
      Opening,         // Создание encoder
      Encoding,        // Encoding frames
      Flushing,        // Flush encoder
      Complete,
      Error(String),
  }
  ```

### 2.2 Default Settings
- **Добавить**:
  ```rust
  impl Default for EncoderSettings {
      fn default() -> Self {
          Self {
              output_path: PathBuf::from("output.mp4"),
              container: Container::MP4,
              codec: VideoCodec::H264,
              encoder_impl: EncoderImpl::Auto,
              quality_mode: QualityMode::CRF,
              quality_value: 23,
          }
      }
  }
  ```

### 2.3 Frame Size Validation
- **Функция**:
  ```rust
  fn validate_frame_sizes(cache: &Cache, range: (usize, usize))
      -> Result<(u32, u32), EncodeError>
  {
      let mut width = None;
      let mut height = None;

      for i in range.0..=range.1 {
          if let Some(frame) = cache.get_frame(i) {
              let (w, h) = frame.dimensions();
              match (width, height) {
                  (None, None) => {
                      width = Some(w);
                      height = Some(h);
                  }
                  (Some(w0), Some(h0)) if w0 != w || h0 != h => {
                      return Err(EncodeError::InconsistentFrameSizes {
                          expected: (w0, h0),
                          found: (w, h),
                          frame: i,
                      });
                  }
                  _ => {}
              }
          }
      }

      width.zip(height).ok_or(EncodeError::NoFrames)
  }
  ```

### 2.4 Encoder Logic
- **Функция**:
  ```rust
  pub fn encode_sequence(
      cache: &Cache,
      settings: &EncoderSettings,
      progress_tx: Sender<EncodeProgress>,
      cancel_flag: Arc<AtomicBool>,
  ) -> Result<(), EncodeError>
  ```
- **Логика**:
  1. Получить `play_range` из cache
  2. **Validate frame sizes** (fail если разные)
  3. Определить encoder (hardware → fallback software):
     - `EncoderImpl::Auto`: Попробовать HW (h264_nvenc) → fallback SW (libx264)
     - `EncoderImpl::Hardware`: Только HW, fail если нет
     - `EncoderImpl::Software`: Только SW
  4. Создать output context (MP4/MOV)
  5. Настроить encoder с параметрами из `settings`
  6. **Loop по frames**:
     - От `play_range_start` до `play_range_end`
     - Проверить `cancel_flag` каждые N кадров
     - `cache.get_frame(i)` → convert RGBA → YUV420P
     - `encoder.send_frame()`
     - `encoder.receive_packet()` → write
     - Отправить progress: `{current: i, total: frames_count, stage: Encoding}`
  7. Flush encoder
  8. Write trailer
  9. Send `EncodeProgress { stage: Complete, ... }`

### 2.5 Hardware Encoding Auto-detect
- **Функция**:
  ```rust
  fn create_video_encoder(
      codec: VideoCodec,
      impl_type: EncoderImpl,
      width: u32,
      height: u32,
  ) -> Result<(ffmpeg::encoder::Video, String), EncodeError>
  {
      use playa_ffmpeg::encoder;

      match (codec, impl_type) {
          (VideoCodec::H264, EncoderImpl::Auto) => {
              // Try NVENC first
              if let Some(enc) = encoder::find_by_name("h264_nvenc") {
                  info!("Using h264_nvenc (NVIDIA hardware encoder)");
                  return Ok((
                      Context::new_with_codec(enc).encoder().video()?,
                      "h264_nvenc".to_string()
                  ));
              }

              // Fallback to software
              warn!("NVENC unavailable, falling back to libx264");
              let enc = encoder::find(codec::Id::H264)
                  .ok_or(EncodeError::EncoderNotFound)?;
              Ok((
                  Context::new_with_codec(enc).encoder().video()?,
                  "libx264".to_string()
              ))
          }

          (VideoCodec::H264, EncoderImpl::Hardware) => {
              let enc = encoder::find_by_name("h264_nvenc")
                  .ok_or(EncodeError::HardwareEncoderUnavailable)?;
              Ok((/* ... */, "h264_nvenc".to_string()))
          }

          (VideoCodec::H264, EncoderImpl::Software) => {
              let enc = encoder::find(codec::Id::H264)
                  .ok_or(EncodeError::EncoderNotFound)?;
              Ok((/* ... */, "libx264".to_string()))
          }

          // Similar for H265, ProRes
      }
  }
  ```

---

## Часть 3: UI для Encoding

### 3.1 Encoding Dialog (ui_encode.rs)
- **Файл**: `src/ui_encode.rs` (новый)
- **Структура**:
  ```rust
  pub struct EncodeDialog {
      pub settings: EncoderSettings,
      pub is_encoding: bool,
      pub progress: Option<EncodeProgress>,
      pub cancel_flag: Arc<AtomicBool>,
      progress_rx: Option<Receiver<EncodeProgress>>,
      encode_thread: Option<JoinHandle<Result<(), EncodeError>>>,
  }
  ```

### 3.2 Settings Persistence
- **Сохранение**: В `AppSettings` (prefs.rs)
  ```rust
  pub struct AppSettings {
      // ... existing
      pub encoder_settings: EncoderSettings,
  }
  ```
- **Когда сохранять**: При нажатии "Encode" (перед запуском encoding thread)
- **Загрузка**: При создании `EncodeDialog::new()` взять из `AppSettings`

### 3.3 Dialog Layout
- **Layout**:
  ```
  ┌─ Video Encoder ────────────────────┐
  │ Output Path: [____________] Browse│
  │                                    │
  │ Container: (•) MP4  ( ) MOV       │
  │                                    │
  │ Codec:                             │
  │   (•) H.264  ( ) H.265  ( ) ProRes│
  │                                    │
  │ Encoder:                           │
  │   (•) Auto (HW→CPU)                │
  │   ( ) Hardware only                │
  │   ( ) Software only                │
  │                                    │
  │ Quality:                           │
  │   (•) CRF  ( ) Bitrate             │
  │   Value: [23___] (18=best, 28=fast)│
  │                                    │
  │ Frame Range: 50 - 150 (101 frames)│
  │                                    │
  │ ─────── Progress ─────────         │  ← Показывать только при encoding
  │ Frame 45 / 101                     │
  │ [████████░░░░░░░░░░] 45%          │
  │ Encoder: h264_nvenc                │
  │                                    │
  │        [Close]  [Encode]           │  ← [Close] при encoding = Cancel
  └────────────────────────────────────┘
  ```

### 3.4 Progress Bar Integration
- **Использовать**: Существующий `ProgressBar` из `src/progress_bar.rs`
- **Показывать** в диалоге только когда `is_encoding == true`
- **Обновлять** из `progress_rx.try_recv()`

### 3.5 UI Blocking During Encoding
- **Когда `is_encoding == true`**:
  - Disable все controls (radio buttons, text fields, browse button)
  - Enable только кнопку "Cancel"
  - Показать progress bar
- **Cancel button**:
  - При нажатии: `cancel_flag.store(true, Ordering::Relaxed)`
  - Encoder thread проверяет flag и останавливается
  - После остановки: `is_encoding = false`, hide progress

### 3.6 Window Close Handling
- **При закрытии окна во время encoding**:
  - Вызвать cancel (как кнопка Cancel)
  - Дождаться завершения thread
  - Закрыть диалог
- **egui**: Проверить `response.close_clicked()`

### 3.7 Encoding Flow
```rust
// Render method
pub fn render(&mut self, ctx: &egui::Context, cache: &Cache) -> bool {
    let mut open = true;

    egui::Window::new("Video Encoder")
        .open(&mut open)
        .show(ctx, |ui| {
            if self.is_encoding {
                // Disable controls
                ui.disable();
            }

            // Settings controls
            // ...

            if self.is_encoding {
                ui.enable();  // Re-enable for progress area

                // Progress bar
                if let Some(progress) = &self.progress {
                    ui.separator();
                    ui.label(format!("Frame {} / {}", progress.current_frame, progress.total_frames));
                    // Use ProgressBar widget
                }

                if ui.button("Cancel").clicked() {
                    self.cancel_encoding();
                }
            } else {
                if ui.button("Encode").clicked() {
                    self.start_encoding(cache);
                }
            }
        });

    // Check window close
    if !open && self.is_encoding {
        self.cancel_encoding();
    }

    open
}

fn start_encoding(&mut self, cache: &Cache) {
    // Save settings to AppSettings
    // ...

    let (tx, rx) = mpsc::channel();
    self.progress_rx = Some(rx);
    self.cancel_flag = Arc::new(AtomicBool::new(false));
    self.is_encoding = true;

    let settings = self.settings.clone();
    let cache = cache.clone(); // Need to make Cache clonable or use Arc
    let cancel = self.cancel_flag.clone();

    self.encode_thread = Some(std::thread::spawn(move || {
        encode_sequence(&cache, &settings, tx, cancel)
    }));
}

fn cancel_encoding(&mut self) {
    self.cancel_flag.store(true, Ordering::Relaxed);
    if let Some(handle) = self.encode_thread.take() {
        let _ = handle.join();
    }
    self.is_encoding = false;
}
```

---

## Часть 4: Интеграция

### 4.1 Keyboard Handler
- **Файл**: `src/main.rs`
- **F7 key**:
  ```rust
  if ctx.input(|i| i.key_pressed(egui::Key::F7)) {
      self.show_encode_dialog = true;
  }
  ```

### 4.2 Main App State
- **Добавить**:
  ```rust
  pub struct PlayaApp {
      // ...
      show_encode_dialog: bool,
      encode_dialog: Option<EncodeDialog>,
  }
  ```

### 4.3 Render Encode Dialog
- **В `PlayaApp::update()`**:
  ```rust
  if self.show_encode_dialog {
      if self.encode_dialog.is_none() {
          self.encode_dialog = Some(EncodeDialog::new(&self.settings));
      }

      if let Some(dialog) = &mut self.encode_dialog {
          let open = dialog.render(ctx, &self.player.cache);

          // Update progress (non-blocking)
          dialog.update();

          if !open {
              self.show_encode_dialog = false;
              self.encode_dialog = None;
          }
      }
  }
  ```

### 4.4 Module Declaration
- **src/main.rs**:
  ```rust
  mod encode;
  mod ui_encode;
  ```
- **Всегда включен** (не optional feature)

### 4.5 Settings Integration
- **prefs.rs**: Добавить `encoder_settings: EncoderSettings` с `#[serde(default)]`
- **Сохранение**: При запуске encoding в `start_encoding()`
- **Загрузка**: При создании `EncodeDialog::new()`

---

## Часть 5: Error Handling

### 5.1 Error Types
```rust
#[derive(Debug)]
pub enum EncodeError {
    EncoderNotFound,
    HardwareEncoderUnavailable,
    InconsistentFrameSizes { expected: (u32, u32), found: (u32, u32), frame: usize },
    NoFrames,
    FFmpegError(ffmpeg::Error),
    Cancelled,
}
```

### 5.2 Error Display in UI
- **При ошибке**:
  - Показать в диалоге красным текстом
  - `is_encoding = false`
  - Разрешить изменить настройки и повторить

### 5.3 Fallback Feedback
- **Лог** в UI:
  - При успешном HW: "Using h264_nvenc"
  - При fallback: "NVENC unavailable, using libx264"
  - Показать в progress area под progress bar

---

## Файлы для создания/модификации

**Создать:**
1. `src/encode.rs` - encoding module (logic)
2. `src/ui_encode.rs` - encoding dialog UI

**Изменить:**
1. `src/cache.rs` - add `play_range_start`, `play_range_end`
2. `src/player.rs` - respect `play_range` in `advance_frame()`
3. `src/timeslider.rs` - visualize work area, handle B/N keys
4. `src/main.rs` - F7 handler, integrate encode dialog, mod declarations
5. `src/prefs.rs` - add `encoder_settings: EncoderSettings`
6. `src/ui.rs` - add B/N/F7 to help screen

**Проверить:**
- `src/progress_bar.rs` - re-use for encoding progress

---

## Тестирование

1. **Play Range UI**:
   - Загрузить sequence 0-200
   - Нажать B на кадре 50 → серая полоса начинается с 50
   - Нажать N на кадре 150 → серая полоса заканчивается на 150
   - Запустить playback → цикл 50-150

2. **Encode Settings Persistence**:
   - F7 → изменить на H.265, CRF 20, path "test.mp4"
   - Нажать "Encode"
   - Закрыть приложение
   - Открыть → F7 → настройки должны остаться

3. **Encoding (Hardware)**:
   - F7 → Auto encoder, H.264
   - "Encode" → проверить лог "Using h264_nvenc"
   - Progress bar должен обновляться
   - Output.mp4 должен воспроизводиться

4. **Encoding (Fallback)**:
   - Без NVIDIA GPU
   - "Encode" → лог "NVENC unavailable, using libx264"
   - Encoding должен пройти успешно

5. **Cancel**:
   - F7 → Start encoding
   - Через 2 секунды → Cancel
   - Encoding должен остановиться
   - Можно снова нажать "Encode"

6. **Frame Size Validation**:
   - Загрузить 2 sequences разных размеров (1920x1080 + 1280x720)
   - F7 → "Encode"
   - Должна появиться ошибка о несовместимых размерах

7. **Window Close During Encoding**:
   - F7 → Start encoding
   - Закрыть окно (X)
   - Encoding должен отменить
   - Окно должно закрыться

---

## Изменения от v1:

1. ✅ `set_play_area_start()` вместо `set_work_area_start()`
2. ✅ Добавлены B/N/F7 в help screen
3. ✅ UI в `ui_encode.rs` (не `ui/encode_dialog.rs`)
4. ✅ Settings persistence через `AppSettings` (save при "Encode")
5. ✅ Progress bar встроен в диалог (не отдельное окно)
6. ✅ Frame size validation обязательна (fail если разные)
7. ✅ UI блокируется при encoding, работает только Cancel/Close
8. ✅ Settings сохраняются только при нажатии "Encode"
