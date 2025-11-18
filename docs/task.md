# План: Многослойный Композитинг

## Ключевые архитектурные решения
Берём за основу After Effects:

### Attrs: атрибуты для Frame, Clip, Layer, Comp, Project: attrs.rs
  - Hashmap of <str:value>: String, u32/i32, f32, Vec3(also RGB), Vec4(also RGBA), Mat3, Mat4
  - Нужна сериализация атрибутов в JSON, можно использовать serde-json.
  - Можно сделать Attrs трейтом и просто добавить его везде где надо?


### Frame: frame.rs
  - Frame owns the Frame memory storage and hashmap of attributes;
  - .Attr:
    - .file String
    - .res(resx, resy, depth)
    - .status(Unloaded|Header|Loading|Loaded|Error)
  - .Data<bytes>
  - ::new_empty(resx, resy, depth, color=Vec3(black)) -> in-place
  - ::new_from_file(filename, status) (calls from_exr or from_image) -> in-place. Can be used to load just Header or full frame with Loaded
  - ::crop(new_left_bottom_x, new_left_bottom_y, new_right_top_x, new_right_top_y, color=black) -> Frame(): correctly increase of decrease size of Frame.
  - ::to_file(filename) -> writes file out
  - ::to_u8() -> Frame()
  - ::to_f16 -> Frame()
  - ::to_f32 -> Frame()
  - ::to_yuv -> Frame()
  - ::get() -> return frame data
  - ::set() -> set frame data
  - ::set_status(Unloaded|Header|Loading|Loaded|Error)
  - Serializable into JSON - that's ::to_json() and ::from_json() functions.
  - текущий вариант Frame можно слегка поменять если надо.


### Clip (бывший Sequence): Clip.rs
  - .data = Vec<Frames>
  - Clip allow access to Frames with indexing: Clip[0] -> &Frame
  - Clip can be initialized either from Clip::new(resx, resy, frames)
  - or with Clip::from_file() that will do Clip::detect_seqs(file) if needed and create required Frames().
  - Clip::get_frame(num) is a centralized interface to get frames from Clips.
  - by default has Attrs with:
    - filename - маска файла типа "c:/temp/seq01/aaa.####.exr" или "aaa.*.exr"
    - .uuid - hash из .filename
    - resolution(x, y, depth)
    - .start/end frames - задетектированные начальный конечный номера файлов. Это позволит реконструировать полные имена файлов и создать Frames.
  - Serializable into JSON - that's ::to_json() and ::from_json() functions. Сохраняет имя файла с маской (одно, не все) и номера начального-конечного кадра. Если конечный не установлен - на загрузке десериализатор должен сделать detect_seqs и обнаружить новые start/end frames.


### Layer: Layer.rs
  - Holds a ref to a single &Clip
  - ::new(&Clip) -> set(&Clip)
  - ::set() - sets &Clip ref
  - ::get() - gets &Clip ref
  - ::clear() - clears Clip ref (sets to Error?)
  - Layer Attrs:
    - Name
    - Resolution, Vec3(x,y,depth) - by default takes that from first Frame of the sequence.
    - Clip_start/clip_end: by default &Clip's.start and end
    - Trim_start/trim_end: offsets from start and end into the inner range. Can be negative, meaning Layer is longer than referred Clip and just displays first and last frames of the Clip outside of start/end.
    - start/end: calculated values from clip_start+trim_start and clip_end+trim_end
    - Layer::get_frame(num) is a centralized interface to get frames from Layers. It num is out of bounds - returns Error()
    - clip_uuid - через uuid запоминается и восстанавливается связь с клипом из MediaPool.
  - Serializable into JSON - that's ::to_json() and ::from_json() functions: хранит свои атрибуты.


### Comp (composition): Comp.rs
  - Attrs:
    - .name
    - .start/end
    - .layers: Vec<Layers>
    - .props: Vec<Attrs>, который хранит атрибуты каждого Layer в векторе Attrs:
      - Visibility, Mute, Solo
      - Transparency: 0.0..1.0
      - Mode: normal, screen, add, substract, mult, divide.
      - Transforms: TRS, pivot. All having XYZ components. Or maybe one 4x4 matrix, suggest here.
      - Start/end: Start and end in global Comp time: Start is offset from Comp start, End can be bigger or smaller than total Layer length, need to think here on trim system
  - Comp allow access to Frames with indexing: Comp[0] -> &Layer
  - ::cur_frame_to_layer_frame(cur_frame) -> (Layer, layer_local_frame_num) - time conversion function and we need the opposite function as well.
  - ::get_cur_frame() / ::set_cur_frame(u32) - задаёт текущий кадр для всех подструктур: Layer, Clip, etc
  - ::get_layers_at(frame) - вычисляет все слои и их абсолютный кадр относительно глобального времени композиции -> hashMap<&Layer, layer_local_framenum)>
  - ::compose() берёт результат get_layers_at_frame и композит слои используя режим layer::mode. Нужен быстрый процессинг на GPU, но шейдера необязательны.
  - ::snap_edges(frame): calculates all layer edges in this comp sorted by distance to given frame -> Vec<u32> - list of frames. Used by UI to snap to layer edges.
  - Финальный результат compose() кэшируется в Composition::Cache (кэш переезжает сюда целиком).
  - Composition::Cache включает кеширующие стратегии (отдельные классы? продумать. Сейчас у нас есть две стратегии: linear(forward/backward) и spirale)
  - Serializable into JSON - that's ::to_json() and ::from_json() functions: хранит свои атрибуты и список сериализованных Layers.



### MediaPool - владеет всеми Clip в проекте, "багажник" куда загружены клипы: media_pool.rs
  - .clips: Vec<Clip>
  - ::add(Clip) / add(filename)
  - ::get(uuid) -> &Clip
  - ::del_name(name): removes Clip from clips
  - When functions delete clips, they also invalidate set status "MissingClip" in all linked Layers
  - Could probably use weakref if Rust have that.
  - Serializable into JSON - that's ::to_json() and ::from_json() functions: сериализует и десериализцет все Clips и Comps


### Project: hold everything together: project.rs
  * MediaPool
    - Clips
  * Compositions
    - Layers
  * Serializable into JSON - that's ::to_json() and ::from_json() functions, хранит буквально JSON с ключами: 'media_pool', comps' и сохраняет и восстанавливает их.

### Viewport: viewport.rs
  - Показывает кадры.
  - Загружает на инициализации список и устанавливает текущий шейдер.
  - Текущий вьюпорт очень хорош
  - On-screen timescrubber

### Encoder: encode.rs
  - максимально отделен от всего остального, получает Comp и кодирует их используя его интерфейс
  - Кодирует Comp в видеоформаты.

### App: app.rs
  * App::Workers: a ThreadPool of workers (3/4 of CPU cores) that do everything, all tasks.
    - Этот Thread pool либо глобален либо синглтон (если такое есть в русте) либо пробрасывается во все многозадачные функции и включает в себя очередь.
    - Если очередь не пуста - воркеры разбирают её и выполняют. Можно класть в очередь лямдбы с референсами, избегаем копирования большой памяти опять же.
    - Manager: a thread watching the queue and assigning workers to tasks in it, manager.
    - Queue: simple queue of lambas to execute
      - Должна напрямую принимать задачи, типа Queue::add(lambda)
      - Либо через шину сообщений (приёмник просто истользует add тоже.
    - Workers: different thread, accepts a given lambda and executes it
    - Избегаем копирования памяти по возможности.
  * .prefs: инстанс Attrs в котором хранятся настройки приложения и всех его компонентов и окон. Это обычный JSON c ключами в которых хранится нужная информация.
  * .load_project, .save_project - запись и загрузка проекта. Функции вызывают JSON-сериализацию всех компонентов и затем записывают результирующий файл.



## UI:
  ### Windows:
    - Project: ui_project.rs
      - Project attrs: resolution, fps, etc
    - MediaPool: ui_media_pool.rs
      - list of Clips and Comps
    - Viewport: ui_viewport.rs
    - Timeline: ui_timeline.rs
      - Timeslider at the top of the window, height: 20 pixels with arros and numbers showing current frame.
      - Scale at the top under the timeslider, showing small vertical ticks for frames and longer ticks for every `FPS` frames
      - Handles simple left clicks. we can keep all the functionality of the current timeslider:
        - colored clips
        - play_range
        - left click handling
      - Layers area, where we have a stack of LayerTracks:
        - Layer name as a string, then
        - Layer options as tight checkboxes at the left: visibility, mute, solo
        - Layers as bars that user can drag around left and right, resize clicking near the left and right edge and move up and down between layers in composition, like in After Effects. We need some robust drag'n'drop mechanism here.
      - We should be able to change the scale of timeline with couple of buttons and we need a mechanism of "zoom". Need suggestions here, нужен какой-то горизонтальный коэффициент масштабирования со слайдером от 0.01 До 100.
      - При измененийи текущего кадра дергается механизм preload который ставит кадры рядом в глобальную очередь воркеров.

  ### Bus: bus.rs
    - Все элементы UI посылают неблокирующие сигналы через системную шину или каналы, нужные функции слушают эти сообщения
    - диалоги
    - кнопки
    - таймслайдер
    - экранный таймслайдер (скруббер)
    - окно MediaPool, вообще все окна - всё основано на сообщениях.
    - Оцени насколько это возможно? Возможно надо сделать центральную шину сообщений в приложении?


Максимально переиспользуем существующую логику, она работает. Но не стесняемся всё ломать и делать заново. Совместимость не нужна.
Старый код и файлы надо будет удалить, у нас есть копия.
Посмотри как устроено сейчас и изучи что можно сделать: нужны детальные мнения, критика, отчёт и план модернизации.
Use sub-agents и mcp, work in parallel.
