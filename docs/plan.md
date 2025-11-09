# План добавления поддержки видео в Playa

## Этап 1: Добавление зависимости playa-ffmpeg

```bash
cargo add playa-ffmpeg
```

**Инициализация в main.rs:**
```rust
use playa_ffmpeg as ffmpeg;

fn main() {
    ffmpeg::init().unwrap();
    // ... остальной код
}
```

---

## Этап 2: Создание модуля src/video.rs

```rust
use std::path::Path;
use playa_ffmpeg as ffmpeg;

pub struct VideoMetadata {
    pub frame_count: usize,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
}

impl VideoMetadata {
    pub fn from_file(path: &Path) -> Result<Self, Error> {
        let ictx = ffmpeg::format::input(path)?;
        let stream = ictx.streams().best(ffmpeg::media::Type::Video)?;

        // Получить frame count из duration * fps
        let duration = stream.duration();
        let fps = stream.avg_frame_rate();
        let tb = stream.time_base();

        let duration_secs = duration as f64 * tb.numerator() as f64 / tb.denominator() as f64;
        let frame_rate = fps.numerator() as f64 / fps.denominator() as f64;
        let frame_count = (duration_secs * frame_rate) as usize;

        // Получить resolution
        let codec_params = stream.parameters();
        let decoder = ffmpeg::codec::context::Context::from_parameters(codec_params)?
            .decoder().video()?;

        Ok(VideoMetadata {
            frame_count,
            width: decoder.width(),
            height: decoder.height(),
            fps: frame_rate,
        })
    }
}

pub fn decode_frame(path: &Path, frame_num: usize) -> Result<PixelBuffer, Error> {
    // 1. Открыть input context
    let mut ictx = ffmpeg::format::input(path)?;
    let stream = ictx.streams().best(ffmpeg::media::Type::Video)?;
    let stream_idx = stream.index();

    // 2. Создать decoder
    let codec_params = stream.parameters();
    let mut decoder = ffmpeg::codec::context::Context::from_parameters(codec_params)?
        .decoder().video()?;

    let width = decoder.width();
    let height = decoder.height();

    // 3. Создать scaler для конвертации в RGB24
    let mut scaler = ffmpeg::software::scaling::Context::get(
        decoder.format(),
        width,
        height,
        ffmpeg::format::Pixel::RGB24,
        width,
        height,
        ffmpeg::software::scaling::Flags::BILINEAR,
    )?;

    // 4. Итерировать пакеты до нужного кадра
    let mut current_frame = 0;
    for (stream, packet) in ictx.packets() {
        if stream.index() == stream_idx {
            decoder.send_packet(&packet)?;

            let mut decoded = ffmpeg::util::frame::video::Video::empty();
            while decoder.receive_frame(&mut decoded).is_ok() {
                if current_frame == frame_num {
                    // 5. Конвертировать в RGB24
                    let mut rgb_frame = ffmpeg::util::frame::video::Video::empty();
                    scaler.run(&decoded, &mut rgb_frame)?;

                    // 6. Конвертировать RGB24 -> RGBA (PixelBuffer::U8)
                    let rgb_data = rgb_frame.data(0);
                    let stride = rgb_frame.stride(0);

                    let mut rgba_data = vec![0u8; (width * height * 4) as usize];
                    for y in 0..height {
                        for x in 0..width {
                            let src_idx = (y * stride as u32 + x * 3) as usize;
                            let dst_idx = (y * width + x) as usize * 4;

                            rgba_data[dst_idx] = rgb_data[src_idx];         // R
                            rgba_data[dst_idx + 1] = rgb_data[src_idx + 1]; // G
                            rgba_data[dst_idx + 2] = rgb_data[src_idx + 2]; // B
                            rgba_data[dst_idx + 3] = 255;                   // A
                        }
                    }

                    return Ok(PixelBuffer::U8(rgba_data));
                }
                current_frame += 1;
            }
        }
    }

    Err(Error::FrameNotFound)
}
```

---

## Этап 3: Обновление src/frame.rs

**Добавить функцию парсинга видео пути:**

```rust
fn parse_video_path(path: &Path) -> (PathBuf, Option<usize>) {
    let path_str = path.to_string_lossy();

    if let Some(at_pos) = path_str.rfind('@') {
        let base = &path_str[..at_pos];
        let frame_num = &path_str[at_pos + 1..];

        if let Ok(num) = frame_num.parse::<usize>() {
            return (PathBuf::from(base), Some(num));
        }
    }

    (path.to_path_buf(), None)
}
```

**Обновить метод load():**

```rust
pub fn load(&self) -> Result<(), FrameError> {
    let (actual_path, frame_num) = parse_video_path(&self.path);

    match actual_path.extension().and_then(|s| s.to_str()) {
        Some("mp4" | "mov" | "avi" | "mkv") => {
            let frame_num = frame_num.unwrap_or(0);
            let pixel_buffer = video::decode_frame(&actual_path, frame_num)?;
            // установить self.data, self.width, self.height
        }
        #[cfg(feature = "openexr")]
        Some("exr") => self.load_exr(&actual_path)?,
        _ => self.load_image(&actual_path)?,
    }

    Ok(())
}
```

---

## Этап 4: Обновление src/sequence.rs

**В методе detect() добавить проверку видео:**

```rust
pub fn detect(paths: Vec<PathBuf>) -> Result<Vec<Sequence>, DetectError> {
    for path in paths {
        if is_video_extension(&path) {
            // Получить метаданные
            let meta = video::VideoMetadata::from_file(&path)?;

            // Создать обычный Sequence (БЕЗ изменения SequenceType!)
            let mut frames = Vec::new();
            for i in 0..meta.frame_count {
                let frame_path = format!("{}@{}", path.display(), i);
                frames.push(Frame::new_placeholder(
                    PathBuf::from(frame_path),
                    meta.width,
                    meta.height,
                ));
            }

            sequences.push(Sequence {
                name: path.file_name().unwrap().to_string_lossy().to_string(),
                frames,
                // ... остальные поля
            });

            continue;
        }

        // Существующая логика для image sequences
        // ...
    }
}

fn is_video_extension(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|s| s.to_str()),
        Some("mp4" | "mov" | "avi" | "mkv")
    )
}
```

---

## Этап 5: UI изменения

**src/ui.rs - обновить FILE_FILTERS:**

```rust
pub const FILE_FILTERS: &[&str] = &[
    "exr", "png", "jpg", "jpeg", "tif", "tiff", "tga", "hdr",
    "mp4", "mov", "avi", "mkv",  // NEW
];
```

**src/main.rs** - drag-and-drop уже универсален, изменений не требуется.

---

## Этап 6: Cache и Workers (БЕЗ ИЗМЕНЕНИЙ!)

Существующий код уже поддерживает:
- Worker pool вызывает `frame.load()` который сам диспатчит по расширению
- Epoch механизм работает для любых источников
- Spiral preload применим к видео
- LRU cache работает с любыми Frame объектами

---

## Порядок реализации:

1. ✅ `cargo add playa-ffmpeg`
2. ✅ `ffmpeg::init()` в main.rs
3. ✅ Создать `src/video.rs` (VideoMetadata + decode_frame)
4. ✅ Обновить `src/frame.rs` (parse_video_path + load)
5. ✅ Обновить `src/sequence.rs` (detect видео файлов)
6. ✅ Обновить `src/ui.rs` (FILE_FILTERS)
7. ✅ Тест: открыть .mp4, scrub по кадрам

---

## Ключевые решения:

✅ **FFmpeg**: `playa-ffmpeg` с crates.io
✅ **Frame path**: `"video.mp4@17"` = кадр 17
✅ **SequenceType**: НЕ меняется, используем существующую структуру
✅ **Decoder**: Один на файл с итерацией до нужного кадра
✅ **Pixel format**: RGB24 → RGBA при загрузке
✅ **Cache/Workers**: Без изменений, работают универсально

---

## Оптимизация (Phase 2, опционально):

Если итерация до нужного кадра медленная:
- Добавить LRU кеш декодеров в video.rs
- Держать декодер открытым между запросами
- Использовать seek для random access к keyframes
