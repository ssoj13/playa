# Playa Architecture Diagrams

## 1. High-Level Module Dependency

```mermaid
graph TD
    main[main.rs] --> cli[cli.rs]
    main --> config[config.rs]
    main --> runner[runner.rs]
    runner --> app[app/]

    app --> ui[ui.rs]
    app --> main_events[main_events.rs]
    app --> shell[shell.rs]

    subgraph "App State"
        app_mod[app/mod.rs]
        app_run[app/run.rs]
        app_events[app/events.rs]
        app_api[app/api.rs]
        app_tabs[app/tabs.rs]
        app_layout[app/layout.rs]
        app_pio[app/project_io.rs]
    end

    subgraph "Core Engine"
        event_bus[event_bus.rs]
        player[player.rs]
        workers[workers.rs]
        cache_man[cache_man.rs]
        global_cache[global_cache.rs]
        preloader[debounced_preloader.rs]
    end

    subgraph "Entities"
        project[project.rs]
        node_kind[node_kind.rs]
        comp_node[comp_node.rs]
        file_node[file_node.rs]
        camera_node[camera_node.rs]
        text_node[text_node.rs]
        frame[frame.rs]
        compositor[compositor.rs]
        gpu_comp[gpu_compositor.rs]
        loader[loader.rs]
        loader_video[loader_video.rs]
        transform[transform.rs]
        effects[effects/]
    end

    subgraph "Widgets"
        viewport[viewport/]
        timeline[timeline/]
        project_panel[project/]
        node_editor[node_editor/]
        ae[ae/]
        status[status/]
    end

    app_run --> event_bus
    app_run --> player
    app_events --> main_events
    main_events --> project
    main_events --> player
    main_events --> global_cache
    main_events --> workers

    project --> node_kind
    node_kind --> comp_node
    node_kind --> file_node
    node_kind --> camera_node
    node_kind --> text_node

    comp_node --> compositor
    comp_node --> gpu_comp
    comp_node --> frame
    comp_node --> transform
    comp_node --> effects

    file_node --> loader
    file_node --> loader_video
    loader --> frame

    workers --> global_cache
    global_cache --> cache_man
    global_cache --> frame

    ui --> viewport
    ui --> timeline
    ui --> project_panel
    ui --> node_editor
    ui --> ae
    ui --> status
```

## 2. Frame Loading Pipeline

```mermaid
sequenceDiagram
    participant UI as UI Thread
    participant EB as EventBus
    participant ME as main_events
    participant PL as Preloader
    participant WK as Workers
    participant GC as GlobalCache
    participant LD as Loader
    participant CM as CacheManager

    UI->>EB: SetFrameEvent(42)
    EB->>ME: handle_app_event()
    ME->>ME: modify_comp() → set_frame(42)
    ME-->>EB: CurrentFrameChangedEvent (deferred)

    Note over ME: BUG-09: also sets<br/>enqueue_frames=true<br/>(double preload)

    EB->>PL: enqueue_frame_loads_around_playhead()
    PL->>GC: get(comp, 42)

    alt Cache HIT
        GC-->>UI: Frame data
        Note over GC: PERF-04: O(n) LRU<br/>shift_remove on hit
    else Cache MISS
        GC->>GC: insert placeholder (Composing)
        PL->>WK: execute_with_epoch(epoch, job)
        WK->>WK: Check epoch (stale? skip)
        WK->>LD: load_frame(path)

        alt EXR
            LD->>LD: detect half/float
            Note over LD: PERF-13: opens<br/>file twice
            LD-->>WK: Frame(F16/F32)
        else Video
            LD->>LD: FFmpeg decode
            Note over LD: BUG-04: no denom<br/>zero check
            LD-->>WK: Frame(U8)
        else PNG/TIFF/JPEG
            LD-->>WK: Frame(U8)
        end

        WK->>GC: insert(comp, 42, frame)
        GC->>CM: track_memory(frame.size)

        alt Memory over limit
            GC->>GC: enforce_limits()
            Note over GC: PERF-04: O(n)<br/>eviction loop
            GC->>CM: release_memory(evicted.size)
        end
    end
```

## 3. Composition Pipeline

```mermaid
flowchart TD
    A[compose_internal] --> B{is_dirty?}
    B -->|recursive check| B
    B -->|dirty| C[collect visible layers]
    B -->|clean + cached| Z[return cached]

    C --> D[for each layer]
    D --> E{source type?}
    E -->|FileNode| F[load from loader]
    E -->|CompNode| G[recursive compose]
    E -->|TextNode| H[render_text]
    E -->|CameraNode| I[skip non-renderable]

    F --> J[apply effects]
    G --> J
    H --> J

    J --> K{has effects?}
    K -->|blur| L[to_f32 + convolve H + convolve V]
    K -->|brightness| M[adjust per pixel]
    K -->|hsv| N[rgb→hsv→adjust→rgb]
    K -->|none| O[passthrough]

    L --> P[apply transform]
    M --> P
    N --> P
    O --> P

    P --> Q[collect source_frames vec]
    Q --> R[promote_frame - format unify]

    R --> S{GPU available?}
    S -->|yes| T[gpu_compositor.blend]
    S -->|no| U[cpu compositor.blend]

    T --> V{success?}
    V -->|yes| W[download texture]
    V -->|no| U

    U --> X[blend_with_dim]

    Note over X: BUG-01: NaN on<br/>transparent pixels
    Note over X: PERF-01: clones<br/>buffer per layer
    Note over X: PERF-03: match<br/>per pixel

    X --> Y[insert into GlobalCache]
    W --> Y
    Y --> Z[return Frame]
```

## 4. Event System Flow

```mermaid
flowchart LR
    subgraph Sources
        KB[Keyboard]
        MS[Mouse]
        API[REST API]
        TMR[Timer/Player]
    end

    subgraph EventBus
        IM[Immediate Subscribers]
        DQ[Deferred Queue<br/>max 1000 events]
    end

    subgraph MainLoop["Main Loop (60Hz)"]
        POLL[poll events]
        HAE[handle_app_event<br/>1232 lines, 16 params]
        DER[Derived Events Loop<br/>max 10 iterations]
    end

    KB --> EventBus
    MS --> EventBus
    API --> EventBus
    TMR --> EventBus

    EventBus --> IM
    EventBus --> DQ

    DQ --> POLL
    POLL --> HAE
    HAE --> DER
    DER -->|re-emit| DQ

    Note over DQ: ARCH-08: silently<br/>drops 500 events<br/>on overflow

    Note over HAE: ARCH-01: god function<br/>needs AppEventContext

    Note over DER: ARCH: ViewportRefresh<br/>re-entrance wastes<br/>iteration budget
```

## 5. Node Type Hierarchy

```mermaid
classDiagram
    class Node {
        <<trait>>
        +uuid() Uuid
        +name() String
        +attrs() Attrs
        +fps() f32
        +_in() i32
        +_out() i32
        +frame() i32
        +dim() (usize, usize)
        +compute(ctx) Result~Frame~
        +inputs() Vec~Uuid~
    }

    class NodeKind {
        <<enum_dispatch>>
        File(FileNode)
        Comp(CompNode)
        Camera(CameraNode)
        Text(TextNode)
        ---
        is_file() bool
        is_comp() bool
        as_file() Option~FileNode~
        as_comp() Option~CompNode~
        fps() ⚠️ SHADOWS TRAIT
        _in() ⚠️ SHADOWS TRAIT
        _out() ⚠️ SHADOWS TRAIT
        frame() ⚠️ SHADOWS TRAIT
    }

    class FileNode {
        +file_mask: String
        +padding: u32
        +compute() Frame
    }

    class CompNode {
        +layers: Vec~Layer~
        +layer_selection: Vec~Uuid~
        +compose_internal() Frame
    }

    class CameraNode {
        +fov: f32
        +near_clip: f32
        +far_clip: f32
    }

    class TextNode {
        +text: String
        +font: String
        +render_text() Frame
    }

    Node <|.. FileNode
    Node <|.. CompNode
    Node <|.. CameraNode
    Node <|.. TextNode
    NodeKind *-- FileNode
    NodeKind *-- CompNode
    NodeKind *-- CameraNode
    NodeKind *-- TextNode
```

## 6. Cache Architecture

```mermaid
flowchart TD
    subgraph GlobalFrameCache
        CACHE["cache: HashMap&lt;Uuid, HashMap&lt;i32, Frame&gt;&gt;"]
        LRU["lru_order: IndexSet&lt;CacheKey&gt;<br/>⚠️ O(n) shift_remove"]
        STATS["CacheStats: hits/misses AtomicU64"]
    end

    subgraph CacheManager
        MEM["memory_usage: AtomicUsize"]
        MAX["max_memory: AtomicUsize (75% RAM)"]
        EPOCH["current_epoch: AtomicU64"]
        DIRTY["dirty_repaint: AtomicBool"]
    end

    subgraph "Frame States"
        HDR[Header] --> |try_claim| LDG[Loading]
        LDG --> |success| LOD[Loaded]
        LDG --> |failure| ERR[Error]
        LOD --> |dehydrate| EXP[Expired]
        EXP --> |reload| LDG
        ERR --> |retry| HDR

        Note over LDG,EXP: BUG-10: dehydrate can<br/>race with Loading
    end

    GlobalFrameCache --> CacheManager
    CACHE --> |get/insert| LRU
    LRU --> |evict| CacheManager
```
