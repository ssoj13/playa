# GPU Compositing Bridge Audit

## Scope

Files actually read:
- `crates/playa-engine/src/entities/gpu_blend_bridge.rs` (full)
- `crates/playa-engine/src/entities/compositor.rs` (full)
- `crates/playa-engine/src/entities/comp_node.rs` (lines 1-200, 500-700, 1300-1840)
- `crates/playa-engine/src/entities/node.rs` (full — `ComputeContext`)
- `crates/playa-app/src/app/mod.rs` (full — `PlayaApp`, `ensure_gpu_blend_initialized`, `gpu_blend_bridge_ref_for_preload`, `drain_gpu_blend_queue`)
- `crates/playa-app/src/app/run.rs` (full — `update`, `update_compositor_backend`)
- `crates/playa-app/src/app/project_io.rs` (full — `enqueue_frame_loads_around_playhead`, `load_project`)
- `crates/playa-app/src/runner.rs` (full — boot path, deserialize → `ensure_gpu_blend_initialized`)
- Targeted greps: `WgpuCompositor`, `compose_internal`, `set_compositor`, all bridge usages

## Findings

### F1 — BLOCKER: `load_project` replaces `self.project` без перевыбора компоновщика; UI несинхронно теряет GPU backend на следующий кадр
- file: `crates/playa-app/src/app/project_io.rs:186-225`
- Class: state-desync / API misuse on persistence boundary
- Evidence:
  ```
  self.project = project;                // 203 — replaces project (compositor is #[serde(skip)] inside Project)
  ```
  В `run.rs:340-369` `update_compositor_backend` создаёт `Wgpu(WgpuCompositor::new(...))` ТОЛЬКО когда `current_is_cpu != desired_is_cpu`. После `load_project` новая `Project` приходит со своим `compositor` (вероятно `CompositorType::default() == Cpu` через rebuild), `desired_is_cpu` остаётся прежним из `settings.compositor_backend`. Если оба `Cpu` — `update_compositor_backend` **никогда** не переинициализирует backend, хотя на самом деле live state для GPU потерян (или наоборот сохранён в загруженном файле как Wgpu невозможно — `#[serde(skip)]`).
  Дополнительно `gpu_blend_rx` поле в `PlayaApp` уцелело между projects, но `project.compositor` теперь свежий объект → между моментом `self.project = project` и следующим вызовом `update_compositor_backend` поле бриджа жило, а компоновщик уже Cpu. `gpu_blend_bridge_ref_for_preload` возвращает `None` (правильно), но `drain_gpu_blend_queue` тем не менее держит lock на `project.compositor` и вызывает `drain_into_compositor` → если в очереди остались pending запросы (worker уже отослал, до ответа не дошло) — drain выполнит `Cpu.blend_with_dim` для GPU-собранного стека. Семантически correct (Cpu blend ok), но это нарушает инвариант "GPU stack blended GPU-side", причём незаметно. Хуже: реплай `Some(Frame)` будет 8-bit/CPU-промотированным, что создаст одиночный визуальный glitch.
- Why it matters: после Open Project пользователь увидит мерцание/смену backend на 1 кадр; запрос worker'а приходит на новый Cpu compositor, ответ может не совпадать с ожидаемым (precision/transform).
- Class-of-bug check: `runner.rs:163-187` имеет тот же паттерн при boot — но там есть явный `app.ensure_gpu_blend_initialized()` (line 187) ПОСЛЕ rebuild. В `load_project` этого вызова **НЕТ**. Sister site mismatch.
- Proposed fix: после `self.project = project` (project_io.rs:203) добавить `self.ensure_gpu_blend_initialized();` и явный `self.update_compositor_backend(...)` если `wgpu_render_state` доступен (или дренировать очередь и сбросить её до подмены: `*self.gpu_blend_rx.lock()... = None;` + потом recreate). Идеально: метод `swap_project(&mut self, new_project)` который дренирует pending → сбрасывает receiver → подменяет project → recreate pair.

### F2 — HIGH: drain в `update()` идёт ПОСЛЕ `update_compositor_backend`, но `update_compositor_backend` ОТСУТСТВУЕТ в первом кадре до wgpu init
- file: `crates/playa-app/src/app/run.rs:38-47`
- Class: race / boot-order
- Evidence:
  ```
  if let Some(rs) = frame.wgpu_render_state() {           // 38
      ...
      self.update_compositor_backend(&rs.device, &rs.queue);   // 43
  }
  ...
  self.drain_gpu_blend_queue(ctx);                         // 47 — ВСЕГДА выполняется
  ```
  `drain_gpu_blend_queue` (mod.rs:336-353) дренирует независимо от того, был ли вызов `update_compositor_backend`. На первом кадре до wgpu init `frame.wgpu_render_state()` может вернуть `None` (нечасто на eframe Wgpu, но возможно на retry/headless). Если settings = Gpu, но backend ещё `Cpu(CpuCompositor)` (default), worker мог уже enqueue запрос (see F3) → drain выполнит CPU blend.
- Why it matters: «GPU compositing включён в prefs» + boot-time race → первые кадры идут через CPU. Не блокер сам по себе, но в сочетании с F4 (TLS forking из NotQueued never fires т.к. send успешен) даёт скрытое CPU fallback без warning.
- Class-of-bug check: `update_compositor_backend` сам по себе хрупок — он сравнивает только `current_is_cpu != desired_is_cpu`, не учитывает factor «GPU device пересоздался». Если eframe пересоздал wgpu::Device (resize on macOS sometimes does it), флаги не меняются → старый `WgpuCompositor` с мёртвым device остаётся.
- Proposed fix: 1) дренировать ТОЛЬКО когда `compositor` действительно `Wgpu(_)` (already implicit, но явный early-return сэкономит mutex); 2) в `update_compositor_backend` хранить версию device (Arc::as_ptr или handle id) и пересоздавать `WgpuCompositor` при смене device; 3) на первом кадре блокировать drain до подтверждения backend wiring.

### F3 — HIGH: workers могут enqueue в bridge ДО того как Wgpu backend смонтирован
- file: `crates/playa-engine/src/entities/comp_node.rs:1660-1725` (`signal_preload`) + `crates/playa-app/src/app/project_io.rs:140-143`
- Class: ordering / API misuse
- Evidence:
  ```
  let bridge = self.gpu_blend_bridge_ref_for_preload();      // project_io.rs:140
  self.project.with_comp(comp_uuid, |comp| {
      comp.signal_preload(&self.workers, &self.project, bridge, effective_radius);
  });
  ```
  `gpu_blend_bridge_ref_for_preload` возвращает `Some(bridge)` ТОЛЬКО если compositor уже `Wgpu(_)`. ОК для preload-вызова. Но в `signal_preload` (comp_node.rs:1702-1709) есть второй фильтр через lock на compositor — duplicated check. Между этими двумя проверками никто не держит lock непрерывно: `gpu_blend_bridge_ref_for_preload` lock'ает → drop guard → потом signal_preload лочит снова. Race window: если в этот момент `update_compositor_backend` свапнул compositor `Wgpu→Cpu` (toggle prefs), то `gpu_blend_bridge_ref_for_preload` мог вернуть `Some` (был Wgpu), а signal_preload увидит `Cpu` и нулифицирует `bridge_for_ctx`. Это **сейчас безопасно** (downgrade в `None` верен), но обратный случай Cpu→Wgpu на той же кадровой логике даст `bridge=None` несмотря на Wgpu backend → workers пойдут TLS Cpu path → визуальный mismatch до следующего frame'а.
  Worse: если CompNode::compute() вызывается из preloaded job уже после смены backend, ctx.gpu_blend_bridge захвачен в момент enqueue, а compositor уже сменился. Выполнение пойдёт по NotQueued? Нет — bridge есть, send ok, дрейн выполнится Cpu compositor'ом (потому что mod.rs:344 берёт project.compositor который стал Cpu). Семантически OK, но тонкая связь.
- Why it matters: смена backend в runtime обычно работает, но окно несинхронизировано. На быстром toggle prefs и одновременном scrub'е получим один-два кадра «не той» точности.
- Class-of-bug check: дублирование checks в двух местах — sister-site issue. `set_compositor` в `project.rs:596` не дренирует pending перед сменой.
- Proposed fix: централизовать выбор bridge в одном месте — внутри signal_preload (там уже есть lock) — убрать `gpu_blend_bridge_ref_for_preload` из вызова или сделать его дешёвым `Option<Arc<...>>` cache, который инвалидируется через `set_compositor`. Альтернатива: drain + reset `gpu_blend_rx` внутри `update_compositor_backend` при смене Wgpu↔Cpu.

### F4 — HIGH: `delegate_blend_blocking` использует `recv()` без timeout — бесконечный deadlock при паузе UI
- file: `crates/playa-engine/src/entities/gpu_blend_bridge.rs:108-118`
- Class: deadlock / cancellation
- Evidence:
  ```
  Ok(()) => match reply_rx.recv() {                      // 110 — UNBOUNDED BLOCK
      Ok(frame) => GpuBlendReport::Completed(frame),
      Err(_) => { ... GpuBlendReport::ReplyDisconnected }
  }
  ```
  Если UI thread suspended (window minimize → eframe пропускает `update`, vsync stall на discrete GPU, modal native dialog rfd::FileDialog blocks `update()` callback in `show_open_project_dialog`), то `drain_into_compositor` не вызывается → worker заблокирован НАВСЕГДА на reply_rx.
  rfd::FileDialog (project_io.rs:177) — синхронный blocking call в `update`. Пока он показан, никаких frame'ов не идёт. Если перед его показом был pending GPU blend (drag preload), worker thread намертво стоит. Не критично если только один worker (main thread жив), но если все N workers на этом — preload на других comp заглохнет, потом при закрытии диалога драйн всех их размотает.
- Why it matters: window minimize + paused playback = workers замораживаются и не могут реагировать на cancel/epoch. Preload не отменится. На графе deps это удерживает Arc<Workers> в живом состоянии.
- Class-of-bug check: `cache_manager.increment_epoch()` в `on_exit` (run.rs:298) и в обычной отмене НЕ разблокирует worker'а на recv. Workers::execute_with_epoch (comp_node.rs:1570) проверяет epoch ДО `compute`, не во время.
- Proposed fix: использовать `recv_timeout(Duration::from_millis(N))` с проверкой epoch/abort флага в петле; ИЛИ ввести AbortHandle/`Arc<AtomicBool>` cancel token в `GpuBlendRequest`, который UI ставит при teardown и компарящий worker увидит на следующем чек-поинте; ИЛИ заменить mpsc на crossbeam с select для cancel канала.

### F5 — HIGH: `gpu_blend_rx_default()` создаёт `None`, но если serde-deserialize **замещает** уже сконструированный default, fresh pair из `Default` теряется
- file: `crates/playa-app/src/app/mod.rs:39-46, 198-282`
- Class: state-desync / serde quirk
- Evidence:
  ```
  #[serde(default)]                  // 66 — `default` на struct level: serde uses Default::default() then overrides individual fields
  pub struct PlayaApp { ... }
  ```
  ```
  #[serde(skip)]
  pub gpu_blend_bridge: Option<Arc<GpuBlendBridge>>,                   // 189-190
  #[serde(skip, default = "gpu_blend_rx_default")]
  pub gpu_blend_rx: Mutex<Option<Receiver<GpuBlendRequest>>>,           // 194-195
  ```
  Поведение serde с `#[serde(default)]` на struct + `#[serde(skip)]` на поле: serde вызывает `Default::default()` для all-struct snapshot, потом patch'ит non-skip поля из JSON. Поля `skip` остаются от Default → у них **есть** свежий bridge/rx из `Default` (mod.rs:215, 279-280). Но `gpu_blend_rx` имеет `default = "gpu_blend_rx_default"` *в дополнение* к `skip` — это override. Результат: после deserialize `gpu_blend_bridge = Some(<from Default>)` (свежий sender), `gpu_blend_rx = None` (через override default function).
  То есть **sender и receiver рассинхронизированы**: bridge содержит sender, который никогда не был спарен с этим rx (rx == None). `ensure_gpu_blend_initialized` (293-309) проверяет `bridge.is_some() && rx.is_some()` → false (rx=None) → создаёт **новую пару**, заменяет bridge свежим, отбрасывает старый из Default. Старый sender тогда осиротеет, но он не передан никому → drop OK.
  Однако: workers, спавнутые до `ensure_gpu_blend_initialized` (но workers создаются в `Default`, они уже живут к моменту deserialize) — они НЕ держат ссылок на bridge до первого `signal_preload`, так что race нет. **Это работает по случайности.** Хрупко: добавь кто-нибудь `pub workers: ... = "default_with_bridge"` — порядок поломается.
- Why it matters: семантически опасный паттерн с двумя источниками `default` для скоррелированных полей. Любой рефакторинг (например удалить `gpu_blend_rx_default` потому что "skip уже подразумевает Default") приведёт к runtime разсинхронизации.
- Class-of-bug check: то же самое для других пар `*_bridge`/`*_rx` если их добавят. Sister-site не нашёл (только одна пара пока).
- Proposed fix: вытащить пару в один Mutex<Option<(Bridge, Receiver)>> или один struct `GpuBlendChannels { bridge, rx }` с общим Default. Либо удалить `default = "gpu_blend_rx_default"` (skip+Default достаточно — Default корректно создаёт пару) — упростить инвариант.

### F6 — MEDIUM: `drain_into_compositor` ловит `Disconnected` как `break` — но в нашем graph никто sender не дропнет до teardown PlayaApp
- file: `crates/playa-engine/src/entities/gpu_blend_bridge.rs:148-150`
- Class: dead-code branch / API surface
- Evidence:
  ```
  Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
  ```
  Sender = `GpuBlendBridge { tx }`. tx — `std::sync::mpsc::Sender<GpuBlendRequest>`, не Sync, но Clone (через `Sender::clone`). `GpuBlendBridge` обёрнут в `Arc` в `PlayaApp` (`Arc<GpuBlendBridge>`) и клонируется в `ComputeContext` через ссылку (node.rs:71 `&'a GpuBlendBridge`). Worker НЕ клонирует Sender в свой стек — он юзает ссылку.
  Это значит: пока `Arc<GpuBlendBridge>` жив, sender жив. Disconnected в drain невозможен пока PlayaApp::gpu_blend_bridge = Some(...). Если `ensure_gpu_blend_initialized` пересоздаёт пару — старый Arc<GpuBlendBridge> дропается (если refcount=1). Старый sender уходит, старый rx уходит. Новый rx ставится в `gpu_blend_rx` — он спарен с новым bridge. drain читает уже **новый** rx. Disconnected branch недостижим в production пути.
- Why it matters: dead branch скрывает потенциальную проблему: если когда-нибудь добавят `worker_sender = Sender::clone()` для прямого использования воркером (мимо Arc), Disconnected перестанет быть unreachable — и текущая обработка `break` (без логирования) тихо дроп'нет очередь.
- Class-of-bug check: симметричный с F4 — там `Err` от reply_rx.recv() логируется. Здесь — silent break.
- Proposed fix: добавить `log::warn!("gpu blend bridge sender dropped — treating as teardown")` перед break + unreachable!() в debug.

### F7 — MEDIUM: `drain_gpu_blend_queue` держит **два** mutex'а (rx + compositor) одновременно; `delegate_blend_blocking` вызывается под lock'ом compositor → потенциальный приоритет инверсия
- file: `crates/playa-app/src/app/mod.rs:336-353`
- Class: lock ordering
- Evidence:
  ```
  let rx_guard = self.gpu_blend_rx.lock()...;      // 337
  ...
  let mut comp = self.project.compositor.lock()...; // 344
  let n = GpuBlendBridge::drain_into_compositor(rx, &mut comp);  // 349 — keeps both locks for duration of all blends
  ```
  Внутри `drain_into_compositor` каждый `compositor.blend_with_dim` может занять 16-50ms на Wgpu (GPU submit + readback). Если в очереди 5 запросов — оба mutex'а удерживаются ~250ms.
  В этот момент `update_compositor_backend` хочет lock на compositor (для свапа Cpu↔Wgpu) — встанет. `gpu_blend_bridge_ref_for_preload` хочет lock на compositor — встанет. `signal_preload` дальше по цепочке — встанет. Worker enqueue не блокируется (sender unbounded), но если worker сам зашёл в delegate_blend_blocking и UI thread долго делает drain — worker ждёт на reply_rx (это OK, ответы прилетают по мере выполнения каждого blend в drain).
  Дальше intereting: если `signal_preload` вызывается ИЗ обработчика события на UI thread (event_bus), он может попытаться lock'нуть compositor РЕКУРСИВНО — `std::sync::Mutex` reentrant'ом не является → deadlock UI thread'а на самом себе. Сейчас в коде signal_preload вызывается из `enqueue_frame_loads_around_playhead` (project_io.rs) → из `tick()` debounced preloader (run.rs:144) → из update. Проверил chain: `drain_gpu_blend_queue` ВЫШЕ в update (line 47), `enqueue_frame_loads_around_playhead` НИЖЕ (line 145) → последовательно, не вложенно. ОК, deadlock'а сейчас нет, но ordering хрупок.
- Why it matters: одно изменение (например drain после tick'а или внутри ивент-хэндлера) поломает порядок и даст self-deadlock.
- Class-of-bug check: `signal_preload` lock'ает compositor (comp_node.rs:1702-1709) — если кто-то его вызовет из контекста, где compositor уже залочен (например, из обработчика внутри drain), мгновенный deadlock.
- Proposed fix: drain должен снимать compositor lock МЕЖДУ запросами или хотя бы периодически уступать; альтернативно — `parking_lot::ReentrantMutex` для compositor; или вытащить `&mut CompositorType` через `RwLock` с явным upgrade.

### F8 — MEDIUM: `CompositorType::Clone` молчаливо downgrade'ит Wgpu → Cpu c warning
- file: `crates/playa-engine/src/entities/compositor.rs:57-70`
- Class: API smell / silent semantic change
- Evidence:
  ```
  impl Clone for CompositorType {
      fn clone(&self) -> Self {
          if matches!(self, CompositorType::Wgpu(_)) {
              log::warn!("CompositorType::clone() called on Wgpu variant - downgrading to CPU. ...");
          }
          CompositorType::Cpu(CpuCompositor)
      }
  }
  ```
  Если кто-то клонирует `Project` (он содержит compositor в Mutex; sequential clone маловероятен, но возможен через тесты или serde paths), результат — тихий downgrade. Сейчас `compositor` — `#[serde(skip)]` (вероятно, проверил по runner.rs:157 `rebuild_with_manager` не упоминает compositor). 
- Why it matters: потенциальный visual glitch если clone случайно вызовется в hot path.
- Class-of-bug check: NEEDS-VERIFY — не нашёл прямых вызовов `.clone()` на compositor; project_io.rs:188 `Project::from_json` создаёт новый Project с default compositor, не clone существующего.
- Proposed fix: `impl Clone` снять; если действительно нужна, panic в debug, log::error в release.

### F9 — LOW: `Frame::set_status(FrameStatus::Composing)` вызывается на возвращённом frame ПОСЛЕ того как тот мог уже быть закэширован (`ctx.cache.insert` line 1469) — потенциальный race
- file: `crates/playa-engine/src/entities/comp_node.rs:1399-1403, 1466-1469`
- Class: caching ordering
- Evidence:
  ```
  let composed = self.compose_internal(frame_idx, ctx)?;  // 1466
  ctx.cache.insert(self.uuid(), frame_idx, composed.clone());  // 1469 — clone before status mutate
  ```
  Внутри compose_internal (1399-1403):
  ```
  result.inspect(|frame| {
      if !all_loaded {
          let _ = frame.set_status(FrameStatus::Composing);
      }
  })
  ```
  `set_status` мутирует через интерьерную мутабельность? NEEDS-VERIFY (не читал frame.rs). Если `Frame` содержит Arc<Mutex<Status>> — то set_status на возвращённом frame ВИДЕН в clone (cache hit увидит "Composing"). Если же Frame — value type, status у клона остаётся unchanged. В первом случае race с readers, во втором — bug (cache хранит "Loaded"-or-whatever, а UI получает "Composing"). 
- Why it matters: статусы могут сбиваться → UI показывает "loaded" placeholder вместо progress; не GPU-bridge specific, но в зоне обзора.
- Class-of-bug check: NEEDS-VERIFY; рекомендую гранулярно посмотреть Frame::set_status.
- Proposed fix: устанавливать статус ДО `cache.insert` или передавать `FrameStatus::Composing` как параметр в compose_internal.

### F10 — LOW/NIT: drop order — `PlayaApp` дроп уронит `gpu_blend_rx` (Mutex<Option<Receiver>>) и `gpu_blend_bridge` (Option<Arc<GpuBlendBridge>>)
- file: `crates/playa-app/src/app/mod.rs:67-196`
- Class: shutdown ordering
- Evidence: Rust drop'ит поля struct в порядке declaration. `workers: Arc<Workers>` (line 126) declared **раньше**, чем `gpu_blend_bridge` (190) и `gpu_blend_rx` (195). Значит при дропе PlayaApp: workers дропается ПЕРВЫМ → `Arc<Workers>::drop`, если refcount=1, остановит worker pool. Но workers могут быть заняты в `delegate_blend_blocking` на recv() → workers::drop ждёт join всех threads → threads ждут UI на reply → UI thread это `on_exit` который уже ушёл → DEADLOCK at shutdown.
  `on_exit` (run.rs:295-305) вызывает `cache_manager.increment_epoch()` и `debounced_preloader.cancel()`, но **НЕ дренирует pending GPU blend** и не разблокирует workers, заклиненных в delegate_blend_blocking::recv.
- Why it matters: shutdown hang при выходе с активным GPU compositor + pending preload.
- Class-of-bug check: симметрично F4 — root cause тот же (нет cancel-канала в bridge).
- Proposed fix: в `on_exit`: 1) сбросить `gpu_blend_rx` в None — это закроет `Receiver<GpuBlendRequest>`, новые `delegate_blend_blocking` получат `NotQueued`, активные не разблокируются; 2) дренировать pending: пока `gpu_blend_rx.is_some()`, выполнять `drain_into_compositor` last-time. 3) ИЛИ внутри `delegate_blend_blocking` использовать `recv_timeout` (см. F4).

### F11 — LOW: `PlayaApp::Default` создаёт `gpu_blend_bridge` всегда, даже если settings выберет Cpu
- file: `crates/playa-app/src/app/mod.rs:215, 279-280`
- Class: minor resource waste
- Evidence:
  ```
  let (gpu_blend_bridge, gpu_blend_rx) = gpu_blend_arc_pair();  // 215
  ...
  gpu_blend_bridge: Some(gpu_blend_bridge),
  gpu_blend_rx: Mutex::new(Some(gpu_blend_rx)),
  ```
  `gpu_blend_bridge_ref_for_preload` всё равно отдаст None при Cpu, так что bridge не используется. Память — Arc + mpsc channel — копейки. Но логически вводит в заблуждение.
- Why it matters: cosmetic.
- Proposed fix: `None` по умолчанию, lazy-init при первом обнаружении Wgpu compositor.

### F12 — NIT: `bridge_for_ctx` дублирует логику `gpu_blend_bridge_ref_for_preload`
- file: `comp_node.rs:1702-1709` vs `mod.rs:315-329`
- Class: duplication
- Evidence: оба читают `project.compositor` под Mutex и матчат `Wgpu(_)`. Если завтра введут третий backend (Vulkan? Metal?) — нужно править в двух местах.
- Proposed fix: оставить только в `signal_preload` (она ближе к use-site), вызвать `gpu_blend_bridge_ref_for_preload` упростить до `self.gpu_blend_bridge.as_deref()`.

## Class-of-bug findings

1. **Lifecycle skip-fields с двойным Default**: F5 — паттерн где `#[serde(skip, default = "fn")]` сосуществует с `#[serde(skip)]` для парного поля → невидимая обязанность синхронизации. Проверять в любых других bridge-типах (api_command_rx, comp_event_emitter и пр.). NEEDS-VERIFY: api_command_rx — `Option<Receiver<ApiCommand>>` (mod.rs:167), default `None`. Похожий паттерн.

2. **`recv()` без timeout/cancel в worker→UI handoff**: F4 + F10 — single источник риска (deadlock при паузе UI / shutdown). Паттерн повторится при любом следующем worker→UI sync вызове, если не зафиксировать общий "blocking handoff" примитив с cancel.

3. **Lock на compositor берётся в трёх местах** (`drain_gpu_blend_queue`, `gpu_blend_bridge_ref_for_preload`, `signal_preload`) — риск deadlock'а растёт с каждым новым call site. Централизовать через единый accessor `with_compositor(|comp| ...)`.

4. **Дублированные backend-checks**: F3 + F12 — выбор bridge через `matches!(*compositor, Wgpu(_))` в двух местах, разделённых разрешённым lock'ом. Race window небольшой, но реален.

## Dead code candidates

- `GpuBlendBridge::drain_into_compositor` ветка `TryRecvError::Disconnected` (gpu_blend_bridge.rs:149) — недостижима в текущей архитектуре (sender в `Arc<GpuBlendBridge>` живёт всю PlayaApp). Не удалять — защита на будущее, но залогировать.
- `CompositorType::Clone` (compositor.rs:57-70) — каллеры не найдены прямым grep'ом, но `Project::Clone` через derive автоматически клонирует поля. NEEDS-VERIFY: есть ли `#[derive(Clone)]` на Project. Если есть, `compositor: Mutex<CompositorType>` потребует Mutex::clone (через owned guard.clone()) → вызовет downgrade. Не удалять без проверки.

## NEEDS-VERIFY

1. `Frame::set_status` thread-safety / value-vs-ref semantics (F9) — не читал `frame.rs`.
2. `Project` has `#[derive(Clone)]`? — для класс-проверки F8/dead-code candidate.
3. Поведение `#[serde(default)]` на struct + `#[serde(skip)]` + `#[serde(skip, default = "fn")]`: подтвердить через тест что `Default` действительно вызывается ДО override field default — F5.
4. `wgpu_render_state()` гарантированно `Some` начиная с какого кадра eframe? Влияет на F2.
5. Есть ли вторая точка входа где `set_compositor` вызывается ПОМИМО `update_compositor_backend`? Если да — F3 race расширяется.
6. `rfd::FileDialog::pick_file()` точно блокирует update() callback на macOS/Linux? На Windows блокирует, на других может быть async — F4 влияние варьируется.
7. Workers thread pool teardown semantics: ждёт ли join на Drop? Если threads detached — F10 не deadlock, а лик. Прочитать `core/workers.rs`.
