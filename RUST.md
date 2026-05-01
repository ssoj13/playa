# LLM Requirements for Professional Rust Development
## Aerospace / High-Tech / Manufacturing Grade

**Version**: 2026-04 | **Toolchain**: 1.95.0 stable | **Primary OS**: Windows 11 (cross-platform: macOS, Linux) | **CI/CD**: GitHub Actions | **GUI**: QtBridges + egui/egui-dock | **Docs**: rustdoc + mdbook + README.md

---

# Alex:

- Avoid using functions that panic like `unwrap()`, instead use mechanisms like `?` to propagate errors.
- Be careful with operations like indexing which may panic if the indexes are out of bounds.
- Never silently discard errors with `let _ =` on fallible operations. Always handle errors appropriately:
  - Propagate errors with `?` when the calling function should handle them
  - Use `.log_err()` or similar when you need to ignore errors but want visibility
  - Use explicit error handling with `match` or `if let Err(...)` when you need custom logic
  - Example: avoid `let _ = client.request(...).await?;` - use `client.request(...).await?;` instead
- When implementing async operations that may fail, ensure errors propagate to the UI layer so users get meaningful feedback.
- When creating new crates, prefer specifying the library root path in `Cargo.toml` using `[lib] path = "...rs"` instead of the default `lib.rs`, to maintain consistent and descriptive naming (e.g., `gpui.rs` or `main.rs`).
- Avoid creative additions unless explicitly requested
- Use shortened words for language names: variables, functions, classes, structs, everything: no dull full names like "get_main_host_parent_window()", use "get_win()" instead or "get_parent()". Make names intuitive.
- Use variable shadowing to scope clones in async contexts for clarity, minimizing the lifetime of borrowed references, example:
```rust
executor.spawn({
    let task_ran = task_ran.clone();
    async move {
        *task_ran.borrow_mut() = true;
    }
});



## 1. CORE PRINCIPLES

### 1.1 Safety First
- **Memory safety is non-negotiable.** Every `unsafe` block requires a `// SAFETY:` comment with invariants and preconditions.
- **No panics on user input.** Use `Result<T, E>` everywhere. Panic only for internal invariant violations.
- **`unwrap()` is forbidden.** Use `?`, `.context()`, or explicit match. Exception: tests, where `unwrap()` is acceptable.
- **Never silently discard errors.** No `let _ = fallible_op();`. Propagate with `?`, log with `.log_err()`, or handle explicitly.
- **Safety-critical context:** DO-178C / DO-254 awareness for aerospace. Ferrocene compiler for certification.

### 1.2 Correctness Before Speed
- Optimize for correctness, readability, maintainability first. Performance after correctness is proven.
- `cargo clippy` with `#![deny(clippy::all)]` -- warnings are errors.
- Enable pedantic lints: `clippy::pedantic`, `clippy::correctness`, `clippy::suspicious`.
- Measure before optimizing. Use `criterion` benchmarks, `cargo flamegraph`, `tokio-console`.

### 1.3 Strict Type Safety
- Leverage type system as design tool: phantom types, newtypes, GATs over runtime checks.
- `#[non_exhaustive]` for public enums/structs (forwards compatibility).
- Builder pattern for complex APIs with optional fields.
- Make illegal states unrepresentable at compile time (Type-State pattern).

### 1.4 Documentation is Code
- **Every public API needs rustdoc** with `# Examples`, `# Errors`, `# Panics`, `# Safety`.
- **mdbook** for architecture decisions (ADR), design rationale, conceptual docs. Max 3 levels deep.
- **README.md**: what, why, how, what uses it, where it's used, quick start, building from source.
- Code comments only for "why" (non-obvious context), never for "what" (code is self-documenting).
- All code examples in docs must be **runnable** (`cargo test --doc`).

### 1.5 Production Infrastructure
- **CI/CD on GitHub**: lint, test, clippy, `cargo audit`, `cargo deny`, cross-platform matrix builds.
- **MSRV explicit**: `rust-version = "1.XX"` in `Cargo.toml`.
- **Testing pyramid**: unit tests in modules, integration in `/tests/`, property tests for critical paths, doc tests for API.
- **No unsafe clippy suppression** without detailed justification comment.

---

## 2. NAMING CONVENTIONS (RFC 430)

### 2.1 Type Names
```
Types/Traits/Enums:    UpperCamelCase    (MeshData, SceneGraph)
Functions/Methods:     snake_case        (get_xform, calc_bounds)
Constants/Statics:     UPPER_SNAKE_CASE  (MAX_VERTICES, DEFAULT_DPI)
Modules:               snake_case        (scene_graph, mesh_ops)
Lifetimes:             'short or 'desc   ('src, 'dst, 'a)
Type Parameters:       T, U, N simple;   Item, Error descriptive
```

### 2.2 Method Naming Conventions
```rust
// to_* = owned/expensive conversion (allocates)
fn to_string(&self) -> String
fn to_vec(&self) -> Vec<T>

// as_* = zero-cost reference/reinterpretation (free)
fn as_str(&self) -> &str
fn as_slice(&self) -> &[T]

// into_* = consuming conversion (moves ownership)
fn into_inner(self) -> T

// from_* = constructor (associated function)
fn from_bytes(bytes: &[u8]) -> Result<Self, E>

// is_* / has_* = boolean predicates
fn is_empty(&self) -> bool
fn has_errors(&self) -> bool

// iter / iter_mut = iterator access
fn iter(&self) -> Iter<'_, T>
```

### 2.3 Naming Quality
```rust
// GOOD: concise, clear, domain-aware
fn get_tr(xform: &Mat4) -> Vec3       // "transform" is universally understood in CG
fn calc_bbox(mesh: &Mesh) -> BBox     // bounding box
fn lerp(a: f32, b: f32, t: f32) -> f32  // standard math term

// BAD: too cryptic
fn gtx(x: &X) -> V                    // no domain meaning

// BAD: overly verbose
fn extract_translation_component_from_transformation_matrix(m: &Mat4) -> Vec3
```

### 2.4 Style Rules
- Implicit returns (no semicolon on last expression).
- Variable shadowing for resource management in async contexts.
- Early returns to reduce nesting.
- Pattern matching over if-let chains when >2 arms.
- `clippy::pedantic` by default, opt-out case-by-case with justification.

---

## 3. ERROR HANDLING & PROPAGATION

### 3.1 Error Hierarchy
```rust
// Libraries: custom error types with thiserror
#[derive(Error, Debug)]
pub enum MeshError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid topology: {msg}")]
    InvalidTopology { msg: String },
}
pub type Result<T> = std::result::Result<T, MeshError>;

// Applications: anyhow for rich context chains
use anyhow::{Result, Context};
let data = fs::read(path).context("Failed to read mesh file")?;
```

### 3.2 Error Rules
- **Never `unwrap()`** on Result from user input, I/O, or network.
- **Use `?`** to propagate errors up the call stack.
- **Use `.context()` / `.with_context()`** to add contextual information.
- **Return `Result<T, E>`** from fallible functions; never silently swallow.
- **Indexing**: prefer `.get(i)` over `vec[i]`; use iterators when possible.
- **Arithmetic**: use `checked_*`, `saturating_*`, `wrapping_*` for safety.

---

## 4. ASYNC & MULTITHREADING

### 4.1 Tokio Standard
```rust
// Production async runtime
#[tokio::main(flavor = "multi_thread")]
async fn main() -> anyhow::Result<()> { ... }

// Light-weight green threads for I/O
tokio::spawn(async { ... })

// CPU-bound / blocking I/O -- offload from reactor
tokio::task::spawn_blocking(|| heavy_computation())

// NEVER block async context
// BAD:  std::thread::sleep(Duration::from_secs(1));
// GOOD: tokio::time::sleep(Duration::from_secs(1)).await;
```

### 4.2 Concurrency Safety
- **Send + Sync**: understand marker traits; document when types are NOT Send.
- **Shared state**: `Arc<Mutex<T>>` for exclusive access, `Arc<RwLock<T>>` for many-readers.
- **Channels**: `tokio::sync::mpsc` / `broadcast` / `watch` for async, not `std::sync::mpsc`.
- **Never hold locks across `.await` points** unless scoped with braces.
- **`parking_lot::Mutex`** over `std::sync::Mutex` for non-async lock performance.

### 4.3 Cancellation Safety
```rust
// Timeout pattern
tokio::select! {
    result = operation() => handle(result),
    _ = tokio::time::sleep(Duration::from_secs(30)) => return Err("timeout".into()),
}

// Spawned task error propagation
let handle = tokio::spawn(async { operation().await });
let result = handle.await??; // Both ? needed: JoinError + operation error
```

### 4.4 Variable Shadowing for Clones in Async
```rust
// Scope clones to minimize lifetime of borrowed references
executor.spawn({
    let state = state.clone();
    let sender = sender.clone();
    async move {
        let data = state.lock().await;
        sender.send(data.clone()).await?;
        Ok(())
    }
});
```

---

## 5. UNSAFE CODE & FFI

### 5.1 Five Unsafe Superpowers
Only use `unsafe` for operations impossible in safe Rust:
1. Dereference raw pointers (`*const T`, `*mut T`)
2. Call unsafe functions/methods
3. Mutate static/global variables
4. Implement unsafe traits (`Send`, `Sync`, `Unpin`)
5. Union field access

### 5.2 Mandatory Rules
```rust
// Every unsafe block MUST have SAFETY comment
unsafe {
    // SAFETY: ptr obtained from Vec::as_mut_ptr(), index verified < len,
    // no aliasing references exist. Allocation owned by this scope.
    *ptr.add(index) = value;
}

// Prefer safe abstractions wrapping unsafe internals
pub fn safe_api(input: &[u8]) -> Result<Output> {
    // validate input...
    unsafe {
        // SAFETY: input validated above, alignment checked
        internal_unsafe_op(input.as_ptr(), input.len())
    }
}
```

### 5.3 FFI / C Interop
- `#![forbid(unsafe_code)]` at crate level; selectively `#![allow(unsafe_code)]` in FFI modules.
- Wrap C bindings in safe Rust types **immediately** at the boundary.
- Use `cty` crate for C type mappings (not hardcoded `i32` for C `int`).
- Validate with `cargo +nightly miri test` for undefined behavior detection.
- Use `cbindgen` / `cxx` for generating bindings.

### 5.4 Global State
```rust
// NEVER: mutable statics
static mut GLOBAL: Option<T> = None; // DATA RACE

// ALWAYS: safe alternatives
static GLOBAL: OnceLock<Config> = OnceLock::new();
static GLOBAL: LazyLock<Config> = LazyLock::new(|| Config::default());
```

---

## 6. CLIPPY & LINTING CONFIGURATION

### 6.1 Cargo.toml Lints
```toml
[lints.rust]
unsafe_code = "deny"       # Require explicit allow per-module
missing_docs = "warn"

[lints.clippy]
all = "deny"
correctness = "deny"
suspicious = "deny"
complexity = "warn"
perf = "warn"
style = "warn"
pedantic = "warn"
nursery = "warn"
unwrap_used = "deny"
expect_used = "deny"

# Justified exceptions (document WHY in code)
module_inception = "allow"
```

### 6.2 Never Suppress Without Justification
- `correctness`: undefined behavior, logic errors
- `suspicious`: likely bugs (regex in loop, inefficient patterns)
- `unwrap_used` / `expect_used`: panic sources

---

## 7. MEMORY & PERFORMANCE OPTIMIZATION

### 7.1 Allocation Strategy
```rust
// Pre-allocate known sizes
let mut vertices = Vec::with_capacity(mesh.vertex_count());

// Stack allocation for small collections
use smallvec::{SmallVec, smallvec};
let mut indices: SmallVec<[u32; 8]> = smallvec![];

// Fixed-size buffers
let mut buf: [u8; 4096] = [0; 4096];
```

### 7.2 Zero-Copy Patterns
```rust
// Borrow instead of clone
fn process(data: &MeshData) {}           // not fn process(data: MeshData)

// Cow for conditional ownership
fn normalize(name: &str) -> Cow<'_, str> {
    if name.contains(' ') {
        Cow::Owned(name.replace(' ', "_"))
    } else {
        Cow::Borrowed(name)
    }
}

// Use &str / &[T] in function signatures, not String / Vec<T>
fn parse_header(input: &[u8]) -> Result<Header> {}

// bytes::Bytes for zero-copy byte buffers in async
use bytes::Bytes;
```

### 7.3 CPU Performance
```toml
[profile.release]
opt-level = 3
lto = "fat"           # Link-time optimization
codegen-units = 1     # Better optimization (slower build)
strip = false         # Keep symbols for production debugging
panic = "abort"       # Smaller binary, no unwinding overhead
```

### 7.4 Profiling Workflow
1. **Baseline**: `cargo build --release --timings` (identify slow crates)
2. **CPU hotspots**: `cargo flamegraph --bin my_app` (Linux/macOS) or ETW/VTune (Windows)
3. **Memory**: `heaptrack` / DHAT / Windows Performance Analyzer
4. **Async**: `tokio-console` for runtime insights
5. **Benchmarks**: `criterion` for key functions with `black_box()`
6. **Algorithm first**, allocations second, locks third, cache fourth, parallelism fifth

### 7.5 Optimization Order
```
1. Algorithm complexity   O(n log n) > O(n^2)
2. Allocations           minimize in hot paths, pre-allocate
3. Lock contention       RwLock > Mutex, lock-free when possible
4. Cache locality        SoA vs AoS, contiguous memory
5. Parallelism           rayon for data, tokio for I/O
6. SIMD / intrinsics     only after profiling proves benefit
```

---

## 8. GUI: egui + egui-dock + QtBridges

### 8.1 egui -- Immediate-Mode UI
Use for: real-time visualization, dynamic layouts, rapid iteration, cross-platform (wasm/desktop).
```rust
egui::CentralPanel::default().show(ctx, |ui| {
    if ui.button("Compute").clicked() {
        // Non-blocking: send command to async backend
        tx.send(Command::Compute).ok();
    }
    ui.add(egui::Slider::new(&mut value, 0.0..=100.0));
});

// NEVER block UI thread -- use channels + request_repaint
tokio::spawn({
    let ctx = ctx.clone();
    let tx = tx.clone();
    async move {
        let result = heavy_compute().await;
        tx.send(Event::Done(result)).ok();
        ctx.request_repaint();
    }
});
```

### 8.2 egui-dock -- Dockable Tabbed Layout
```rust
let mut dock_state = DockState::new(vec!["Viewport", "Properties", "Console"]);
DockArea::new(&mut dock_state)
    .style(Style::from_egui(ctx.style().as_ref()))
    .show(ctx, &mut tab_viewer);

// Features: drag-and-drop tabs, resizable panels, layout serialization, floating windows
```

### 8.3 QtBridges -- Qt/QML Integration
For integrating Rust backend with Qt Quick UI in professional applications:
```
Architecture:
  Rust Backend (business logic, async, data) <--channel--> Qt/QML Frontend (UI rendering)

CXX-Qt:  Direct C++ Qt API access from Rust (for existing C++ Qt codebases)
QtBridges: Higher-level abstraction, Rust backend decoupled from Qt (for new projects)
```
```rust
// Backend exposes clean data interface
#[derive(Serialize, Deserialize)]
pub struct SceneModel {
    pub objects: Vec<SceneObject>,
    pub camera: Camera,
}

// Communication via typed channels
pub fn backend_loop(rx: Receiver<Command>, tx: Sender<Event>) {
    while let Ok(cmd) = rx.recv() {
        match cmd {
            Command::LoadScene(path) => {
                let scene = load_scene(&path)?;
                tx.send(Event::SceneLoaded(scene))?;
            }
        }
    }
}
```

### 8.4 GUI Best Practices
- **Separation of concerns**: logic in library crates, UI in binary crate.
- **Reactive pattern**: Event -> State mutation -> UI update.
- **Never block UI thread**: async/channels for I/O and compute.
- **Accessibility**: keyboard navigation, screen reader support (AccessKit in egui).
- **DPI awareness**: handle high-DPI displays; test at multiple scale factors.
- **Dark/Light themes**: support both; follow OS preference.

---

## 9. BUILD SYSTEM & CROSS-COMPILATION

### 9.1 Cargo.toml Structure
```toml
[package]
name = "my-tool"
version = "0.1.0"
edition = "2024"
rust-version = "1.85"       # Explicit MSRV
license = "MIT OR Apache-2.0"
repository = "https://github.com/org/project"

[lib]
path = "src/my_tool.rs"     # Descriptive name, not default lib.rs

[[bin]]
name = "cli"
path = "src/bin/cli.rs"

[dependencies]
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
anyhow = "1"
thiserror = "2"
tracing = "0.1"

[dev-dependencies]
proptest = "1"
criterion = { version = "0.5", features = ["html_reports"] }

[features]
default = ["gui"]
gui = ["egui", "egui-dock"]
qt = ["qtbridges"]
```

### 9.2 Platform-Specific Dependencies
```toml
[target.'cfg(windows)'.dependencies]
windows = { version = "0.58", features = ["Win32_Foundation", "Win32_System_Threading"] }

[target.'cfg(unix)'.dependencies]
nix = { version = "0.29", features = ["fs", "process"] }
```

### 9.3 Conditional Compilation
```rust
#[cfg(target_os = "windows")]
mod win_impl { /* Windows-specific */ }

#[cfg(target_family = "unix")]
mod unix_impl { /* Unix-specific */ }

// Always use Path/PathBuf (handles separators automatically)
let config = Path::new("config").join("app.toml"); // correct on all platforms
// NEVER: "config\\app.toml" or "config/app.toml" hardcoded
```

### 9.4 Feature Flags
```toml
[features]
default = ["tokio-runtime"]
tokio-runtime = ["tokio"]
gui = ["egui", "egui-dock"]
qt-gui = ["qtbridges"]
unsafe-ffi = []             # Opt-in for FFI modules
nightly = []                # Opt-in nightly features
```

---

## 10. NIGHTLY RUST FEATURES

### 10.1 When to Use Nightly
- Only with documented stabilization timeline / tracking RFC.
- Only in feature-gated code with stable fallback.
- Never in published library crates without major version warning.
- CI must test both stable and nightly.

### 10.2 Safe Unstable Usage Pattern
```rust
// Cargo.toml
[features]
nightly = []

// lib.rs
#![cfg_attr(feature = "nightly", feature(type_alias_impl_trait))]
#![cfg_attr(feature = "nightly", feature(generic_const_exprs))]

pub fn stable_api() { /* always available */ }

#[cfg(feature = "nightly")]
pub fn experimental_api() { /* uses unstable features */ }
```

### 10.3 Useful Nightly Features (2026)
- `async_fn_in_trait` -- async methods in traits
- `type_alias_impl_trait` -- `type Foo = impl Trait`
- `generic_const_exprs` -- const generics with expressions
- `allocator_api` -- custom allocators
- Track stabilization at: https://github.com/rust-lang/rust/milestones

---

## 11. TESTING & VALIDATION

### 11.1 Testing Pyramid
```
              Integration Tests  (few, broad, real I/O)
             /                 \
        Unit Tests           E2E Tests
           /                    \
     Doc Tests              Property Tests (proptest)
         /                      \
    Miri (UB)              Benchmarks (criterion)
```

### 11.2 Unit Tests
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path() {
        let result = process_mesh(&sample_mesh());
        assert_eq!(result.vertex_count(), 42);
    }

    #[test]
    fn error_on_invalid_input() {
        let result = process_mesh(&empty_mesh());
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn async_operation() {
        let data = fetch_data("test.obj").await.unwrap();
        assert!(!data.is_empty());
    }
}
```

### 11.3 Property-Based Testing
```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn roundtrip_serialization(value in any::<MeshData>()) {
        let bytes = bincode::serialize(&value)?;
        let decoded: MeshData = bincode::deserialize(&bytes)?;
        prop_assert_eq!(value, decoded);
    }
}
```

### 11.4 Static Analysis & Validation Pipeline
```bash
cargo fmt --check                    # formatting
cargo clippy -- -D warnings          # linting
cargo test --all-features            # unit + integration
cargo test --doc                     # documentation examples
cargo +nightly miri test             # undefined behavior
cargo audit                          # known vulnerabilities
cargo deny check advisories          # supply chain
cargo tarpaulin --out Html           # code coverage
```

---

## 12. CI/CD GITHUB ACTIONS

### 12.1 Main CI Workflow
```yaml
name: CI
on: [push, pull_request]

jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --check
      - run: cargo clippy --all-targets --all-features -- -D warnings
      - run: cargo doc --no-deps --all-features
        env:
          RUSTDOCFLAGS: "-D warnings"

  test:
    needs: check
    strategy:
      matrix:
        os: [ubuntu-latest, windows-latest, macos-latest]
        rust: [stable, nightly]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@master
        with:
          toolchain: ${{ matrix.rust }}
      - uses: Swatinem/rust-cache@v2
      - run: cargo test --all-features --verbose

  miri:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@nightly
        with:
          components: miri
      - run: cargo +nightly miri test

  audit:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: cargo install cargo-audit cargo-deny
      - run: cargo audit
      - run: cargo deny check
```

### 12.2 Release Workflow
```yaml
name: Release
on:
  push:
    tags: ['v*']
jobs:
  build:
    strategy:
      matrix:
        include:
          - os: windows-latest
            target: x86_64-pc-windows-msvc
          - os: ubuntu-latest
            target: x86_64-unknown-linux-gnu
          - os: macos-latest
            target: aarch64-apple-darwin
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}
      - run: cargo build --release --target ${{ matrix.target }}
      - uses: actions/upload-artifact@v4
        with:
          name: binary-${{ matrix.target }}
          path: target/${{ matrix.target }}/release/
```

---

## 13. DOCUMENTATION STANDARDS

### 13.1 Rustdoc Template
```rust
/// Brief one-liner description.
///
/// Longer explanation: behavior, invariants, design rationale.
///
/// # Examples
/// ```
/// let mesh = Mesh::from_obj("cube.obj")?;
/// assert_eq!(mesh.face_count(), 6);
/// ```
///
/// # Errors
/// Returns [`MeshError::InvalidTopology`] if faces reference non-existent vertices.
///
/// # Panics
/// Panics if internal vertex buffer is corrupted (invariant violation).
///
/// # Safety (if unsafe)
/// Caller must ensure `ptr` is valid and aligned to `T`.
pub fn load_mesh(path: &Path) -> Result<Mesh> { }
```

### 13.2 mdbook Structure
```
book/
  SUMMARY.md          # Table of contents (max 3 levels deep)
  intro/
    what-is-this.md    # Purpose and audience
    quick-start.md     # Minimal example
  architecture/
    overview.md        # System design, key decisions
    adr-001.md         # Architecture Decision Record
  api/
    mesh-ops.md        # API design and examples
  deployment/
    windows.md         # Platform-specific notes
    ci-cd.md           # Build pipeline docs
```

Build and test:
```bash
mdbook build              # generate HTML
mdbook serve              # local preview at localhost:3000
mdbook test               # verify code examples compile
```

### 13.3 README.md Structure
```markdown
# Project Name
## What is this?
[Purpose and audience in one paragraph.]
## Key features
## Quick start
## How it works
[Architecture, design decisions, why built this way.]
## Building from source
[Prerequisites, cargo commands, optional features.]
## Documentation
[Links to mdbook, docs.rs, architecture docs.]
## License
```

---

## 14. SECURITY & SUPPLY CHAIN

### 14.1 Dependency Auditing
```bash
cargo audit                        # known CVEs
cargo deny check advisories        # unmaintained, yanked
cargo deny check licenses          # license compliance
cargo tree --duplicates            # duplicate dependency versions
```

### 14.2 Rules
- Pin versions in `Cargo.lock` for reproducible builds (always commit lock for binaries).
- Minimize features: `features = []` to reduce transitive dependencies.
- Review new dependencies before adding (maintenance status, unsafe usage, license).
- Regular updates: `cargo upgrade` + full test suite.
- Use `#![forbid(unsafe_code)]` at crate level; selectively allow in FFI modules.
- Digital signatures for release binaries.
- SSL/TLS for all network communication.

---

## 15. AEROSPACE / SAFETY-CRITICAL REQUIREMENTS

### 15.1 DO-178C Compliance
- **Traceability**: requirements -> code -> tests (traceability matrix).
- **Structural coverage**: 100% statement, 100% branch for DAL-A/B. MC/DC for critical code.
- **Configuration management**: version everything; no uncommitted code in builds.
- **Code review**: peer review mandatory for all safety-critical changes.
- **Ferrocene compiler**: for certified Rust toolchain (Ferrous Systems).

### 15.2 Forbidden in Safety-Critical Code
```rust
// NEVER in safety-critical:
unwrap() / expect()                    // panic in flight = catastrophic
Vec without pre-allocated capacity     // unbounded allocation
recursion without proven depth bounds  // stack overflow
dyn Trait                              // unpredictable dispatch
unsafe without formal verification     // unverified memory ops
floating-point in safety logic         // use fixed-point
```

### 15.3 Required Patterns
```rust
#![forbid(unsafe_code)]
#![deny(clippy::all, clippy::pedantic)]

// Fixed-size collections
use heapless::Vec as FixedVec;
let mut buf: FixedVec<u8, 256> = FixedVec::new();

// Explicit error handling everywhere
fn critical_op(input: &SensorData) -> Result<Output, CriticalError> {
    let validated = validate(input)?;
    compute(validated)
}

// Document safety case
/// # Safety Case
/// - Preconditions: sensor data within calibrated range
/// - Postconditions: output bounded to [MIN_OUTPUT, MAX_OUTPUT]
/// - Evidence: unit tests + property tests + formal analysis
```

---

## 16. WINDOWS-FIRST, CROSS-PLATFORM

### 16.1 Windows Development
- MSVC C++ Build Tools required (via Visual Studio or standalone).
- `windows` crate for Win32/COM APIs with strongly-typed bindings.
- Use `winapi` / `windows-sys` for low-level, `windows` crate for high-level.
- DPI awareness: handle high-DPI monitors, test at 100%/150%/200% scaling.
- MSIX packaging recommended for production Windows applications.

### 16.2 Cross-Platform Portability
```rust
// Always use Path (not string manipulation)
let config = dirs::config_dir().context("no config dir")?;
let app_config = config.join("myapp").join("settings.toml");

// Platform-specific behavior behind cfg
#[cfg(target_os = "windows")]
fn open_explorer(path: &Path) -> Result<()> {
    Command::new("explorer").arg(path).spawn()?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn open_explorer(path: &Path) -> Result<()> {
    Command::new("open").arg(path).spawn()?;
    Ok(())
}
```

### 16.3 Build Targets
```bash
# Primary
cargo build --target x86_64-pc-windows-msvc

# Cross-platform
cargo build --target x86_64-unknown-linux-gnu
cargo build --target aarch64-apple-darwin

# WebAssembly (for egui web deployment)
cargo build --target wasm32-unknown-unknown
```

---

## 17. PRODUCTION CONFIGURATION

### 17.1 Logging & Tracing (from day one)

Logging must be integrated from the very first line of code, not added later.

**Crates:**
- `tracing` -- structured, span-based instrumentation (replaces `log`)
- `tracing-subscriber` -- formatting, filtering, layered subscribers
- `tracing-chrome` -- Chrome Trace Format output (`.json`) for **Perfetto UI** / `chrome://tracing`
- `tracing-flame` -- flamegraph-compatible output for `inferno`

**Log Levels (use all of them from the start):**
```rust
error!("Fatal: mesh file corrupted");        // ERROR -- broken state, user must act
warn!("Texture not found, using fallback");   // WARN  -- degraded but recoverable
info!("Scene loaded: {} objects", count);     // INFO  -- key lifecycle events
debug!("Vertex buffer allocated: {} bytes", n); // DEBUG -- developer context
trace!("Entering transform loop, iter={}", i); // TRACE -- hot-path granularity
```

**Subscriber Setup (multi-layer):**
```rust
use tracing_subscriber::{self, EnvFilter, fmt, prelude::*};
use tracing_chrome::ChromeLayerBuilder;

fn init_tracing() {
    // Layer 1: stderr with color (dev) or JSON (prod)
    let fmt_layer = fmt::layer()
        .with_env_filter(EnvFilter::from_default_env()) // RUST_LOG=debug
        .with_writer(std::io::stderr);

    // Layer 2: Chrome Trace Format for performance analysis
    let (chrome_layer, guard) = ChromeLayerBuilder::new()
        .file("trace.json")
        .include_args(true)
        .build();
    // IMPORTANT: keep `guard` alive until program exit (flush on drop)

    tracing_subscriber::registry()
        .with(fmt_layer)
        .with(chrome_layer)
        .init();
}
```

**Instrumenting Functions:**
```rust
#[instrument(skip(mesh_data), fields(bytes = mesh_data.len()))]
fn process_mesh(path: &Path, mesh_data: &[u8]) -> Result<Mesh> {
    info!(?path, "Loading mesh");
    // spans automatically create nested trace events
    let _span = tracing::debug_span!("parse_vertices").entered();
    // ...
    Ok(mesh)
}
```

### 17.2 Performance Profiling & Visualization

**Flamegraphs:**
```bash
# CPU flamegraph (Linux/macOS)
cargo flamegraph --bin my_app -- --args

# Windows: use ETW + Windows Performance Analyzer, or:
cargo install inferno
cargo build --release
# collect perf data, then:
inferno-flamegraph < perf.folded > flamegraph.svg
```

**Chrome Trace Format (Perfetto UI):**
```
1. Add tracing-chrome to dependencies
2. Run application -> generates trace.json
3. Open https://ui.perfetto.dev (or chrome://tracing)
4. Load trace.json -> interactive timeline with spans, durations, nesting
```
This shows exact function call hierarchy, async task scheduling, lock contention, and I/O waits in a visual timeline. Essential for async performance debugging.

**tokio-console (async runtime inspector):**
```bash
# Install
cargo install tokio-console

# Enable in app (Cargo.toml)
tokio = { version = "1", features = ["full", "tracing"] }
console-subscriber = "0.4"

# In code
console_subscriber::init();  // instead of or alongside tracing_subscriber

# Run inspector in separate terminal
tokio-console
```
Shows: active tasks, task poll times, waker counts, resource utilization in real-time.

**DHAT (heap profiling):**
```rust
// In main.rs (debug builds only)
#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();
    // ... generates dhat-heap.json -> view at https://nnethercote.github.io/dh_view/dh_view.html
}
```

### 17.3 Process Hygiene & Orphan Prevention

LLM MUST track and clean up all spawned processes. Dangling build/debug/dev-server processes waste resources, hold file locks, and block ports.

**Rules:**
1. **Before spawning a build/test/dev-server** -- check if a previous instance is still running. Kill stale ones first.
2. **After finishing work** -- verify no orphaned `cargo`, `rustc`, `rust-analyzer`, `miri`, `mdbook serve` processes remain.
3. **On error/crash** -- always clean up child processes before retrying.
4. **Track PIDs** -- when spawning background processes, record PID and verify termination.
5. **Port conflicts** -- before starting a dev server, check if the port is already in use (`netstat` / `ss` / `Test-NetConnection`).

**NEVER kill `bun` processes.** Bun manages its own lifecycle; force-killing it causes cascading self-destruction (zombie subprocesses, corrupted state, broken IPC). Let bun terminate naturally or send a graceful shutdown signal.

**Process checklist (run periodically during long sessions):**
```bash
# Windows (pwsh)
Get-Process -Name cargo,rustc,rust-analyzer,mdbook -ErrorAction SilentlyContinue | Format-Table Id,ProcessName,StartTime

# Linux/macOS
ps aux | grep -E '(cargo|rustc|rust-analyzer|mdbook)' | grep -v grep
```

**Cleanup pattern:**
```bash
# Windows (pwsh) -- kill stale cargo/rustc (NOT bun!)
Get-Process -Name cargo,rustc -ErrorAction SilentlyContinue | Where-Object { $_.StartTime -lt (Get-Date).AddMinutes(-30) } | Stop-Process -Force

# Linux/macOS
pkill -f 'cargo (build|test|run)' --older-than 30m
```

**Forbidden:**
- `Stop-Process -Name bun` / `pkill bun` / `kill -9 <bun_pid>` -- NEVER
- Leaving `cargo watch` / `mdbook serve` running after session ends
- Starting a second `cargo build` while another is running (lock contention on `target/`)
- Ignoring "address already in use" errors (find and stop the previous process first)

### 17.4 Release Profiles
```toml
[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
strip = false          # keep debug symbols for crash analysis
panic = "abort"

[profile.dev]
opt-level = 1          # faster dev iteration with some optimization

[profile.bench]
inherits = "release"
debug = true           # flamegraphs need debug info
```

---

## 18. LLM COGNITIVE INFRASTRUCTURE

LLM MUST use external tools for thinking, memory, and progress tracking. LLM context window is finite and compacts -- anything not persisted externally WILL be lost.

### 18.1 Sequential Thinking (seq_think)

Use `mcp__filesystem__seq_think` for **all non-trivial reasoning** before writing code:
- Analyzing error messages and choosing fix strategy
- Planning multi-step refactors
- Evaluating trade-offs between approaches
- Debugging complex issues (form hypothesis -> test -> conclude)
- Architectural decisions

```
// WRONG: jump straight to code after seeing an error
// RIGHT: seq_think -> analyze error -> form hypothesis -> plan fix -> implement
```

Thinking is cheap, broken code is expensive. Think first, code second.

### 18.2 Memory Tools (mem_put / mem_get / mem_search / mem_link)

Use `mcp__filesystem__mem_*` tools as persistent cross-session memory:
- **mem_put**: store key decisions, discovered constraints, architecture notes
- **mem_get**: recall previous decisions before making new ones
- **mem_search**: find relevant context when starting new task
- **mem_link**: create relations between memory entries (e.g., "crate X depends on decision Y")
- **mem_update**: keep entries current as project evolves

**What to store:**
- Build quirks discovered (e.g., "winapi requires feature X on ARM")
- Dependency decisions and rationale ("chose sqlx over diesel because async-first")
- Platform-specific gotchas found during development
- Performance baselines and optimization results
- Error patterns and their solutions
- Cross-crate API contracts and invariants

**What NOT to store:** things derivable from code, git history, or docs.

### 18.3 Intermediate .md Files for State & Progress

LLM MUST create and maintain `.md` files as working memory that survives context compaction:

**TODO.md -- task tracking (MANDATORY for multi-step work):**
```markdown
# TODO

**Rules for TODO.md:**
1. **Create before starting** any task with >3 steps.
2. **Mark items `[x]` immediately** when completed, not in batches.
3. **Add discovered issues** as you find them -- this IS the memory.
4. **Record decisions with rationale** -- future you (or next session) needs the "why".
5. **Never delete completed items** -- they are evidence of progress and context for next session, just mark them done.
6. **Read TODO.md first** when resuming work or after context compaction.

**PROGRESS.md -- session handoff (for long/multi-session tasks):**
```markdown
# Progress Log

**SCRATCH.md -- ephemeral working notes:**
- Error messages being investigated
- Hypotheses being tested
- Intermediate results from profiling
- Deleted after issue is resolved

### 18.4 Workflow Integration

```
Start task:
  1. seq_think: analyze requirements, plan approach
  2. mem_search: check for relevant prior decisions
  3. Create/update TODO.md with detailed checklist
  4. Begin implementation

During work:
  5. Mark TODO items [x] as completed
  6. seq_think before any non-obvious decision
  7. mem_put for discoveries that affect future work
  8. Add issues/blockers to TODO.md immediately

End of session / before context compacts:
  9. Update PROGRESS.md with current state
  10. mem_put key decisions and unresolved issues
  11. Verify TODO.md reflects actual state

Resume / new session:
  12. Read TODO.md + PROGRESS.md FIRST
  13. mem_search for project context
  14. seq_think: re-orient, plan next steps
  15. Continue from where left off
```

**MANDATORY:** these files are NOT optional documentation. They are the LLM's external brain. Without them, context compaction = amnesia = repeated mistakes = wasted tokens.

---

## 19. COMMON PATTERNS

### 19.1 Builder Pattern
```rust
pub struct PipelineConfig { /* fields */ }

pub struct PipelineBuilder {
    threads: usize,
    buffer_size: usize,
}

impl PipelineBuilder {
    pub fn new() -> Self { Self { threads: 4, buffer_size: 8192 } }
    pub fn threads(mut self, n: usize) -> Self { self.threads = n; self }
    pub fn buffer_size(mut self, size: usize) -> Self { self.buffer_size = size; self }
    pub fn build(self) -> Result<PipelineConfig> { /* validate + construct */ }
}
```

### 19.2 Type-State Pattern
```rust
pub struct Uninitialized;
pub struct Ready;
pub struct Running;

pub struct Pipeline<S> { _state: PhantomData<S>, /* ... */ }

impl Pipeline<Uninitialized> {
    pub fn new() -> Self { /* ... */ }
    pub fn configure(self, config: Config) -> Result<Pipeline<Ready>> { /* ... */ }
}

impl Pipeline<Ready> {
    pub fn start(self) -> Result<Pipeline<Running>> { /* ... */ }
}

impl Pipeline<Running> {
    pub fn process(&self, data: &[u8]) -> Result<Output> { /* ... */ }
    pub fn stop(self) -> Pipeline<Uninitialized> { /* ... */ }
}
// Compile-time: Pipeline<Uninitialized>.process() is impossible
```

### 19.3 RAII / Drop Guard
```rust
pub struct GpuBuffer { handle: u64 }

impl Drop for GpuBuffer {
    fn drop(&mut self) {
        // Automatic cleanup when scope exits
        unsafe { gpu_free(self.handle); }
    }
}
```

### 19.4 Newtype Pattern
```rust
pub struct Meters(f64);
pub struct Radians(f64);

// Compile-time prevents mixing units
fn set_distance(d: Meters) { }
fn set_angle(a: Radians) { }
// set_distance(Radians(1.0)); // COMPILE ERROR
```

---

## 20. ECOSYSTEM STACK (2026)

| Category | Primary | Alternative | Notes |
|----------|---------|-------------|-------|
| Async Runtime | tokio | smol, embassy | tokio for general; embassy for embedded |
| HTTP | reqwest | hyper, ureq | reqwest high-level; hyper low-level |
| Serialization | serde + serde_json | bincode, rmp | serde universal; bincode for binary |
| Errors | anyhow + thiserror | eyre, miette | anyhow for apps; thiserror for libs |
| Logging | tracing | log | tracing is structured, replaces log |
| Testing | criterion + proptest | divan | criterion benchmarks; proptest fuzzing |
| GUI (immediate) | egui + egui-dock | slint | egui for real-time, dynamic UIs |
| GUI (retained) | iced, slint | QtBridges for professional Qt apps |
| CLI | clap | argh | clap is standard |
| Database | sqlx | diesel, sea-orm | sqlx async-first, compile-time checked |
| Math | glam, nalgebra | ultraviolet | glam for CG; nalgebra for science |
| Proc Macros | syn + quote | darling | syn for parsing; darling for derives |
| Allocator | mimalloc | jemalloc | mimalloc fast on Windows |

---

## 21. CRITICAL LLM BEHAVIORAL RULES

### NEVER Do This:
```rust
value.unwrap()                          // panic source
let _ = fallible_op();                  // silent error discard
static mut GLOBAL: T = ...;            // data race
unsafe { *ptr = val; }                  // no SAFETY comment
panic!("bad input: {}", user_data);     // panic on user input
let path = "C:\\Users\\data.txt";       // hardcoded non-portable path
std::thread::sleep(dur);               // blocking async reactor
for _ in 0..user_input { vec.push(x); } // unbounded allocation (DoS)
vec[user_index]                         // unchecked index (panic)
```

### ALWAYS Do This:
```rust
value.context("loading mesh")?          // propagate with context
match op() { Ok(v) => v, Err(e) => return Err(e.into()) }
static GLOBAL: OnceLock<T> = OnceLock::new();
unsafe { /* SAFETY: ptr from Vec::as_mut_ptr(), len verified */ *ptr = val; }
if bad_input { return Err(InputError::new(details)); }
let path = Path::new("data").join("config.toml");
tokio::time::sleep(dur).await;
let bounded = user_input.min(MAX_ITEMS);
vec.get(index).ok_or(IndexError)?
```

### LLM Output Verification Checklist:
- [ ] No `unwrap()` / `expect()` without test-only justification
- [ ] All errors handled with `Result` + `?` operator
- [ ] No `unsafe` without `// SAFETY:` comment and invariants
- [ ] Lifetimes explicit or correctly inferred
- [ ] `cargo clippy` passes clean
- [ ] `cargo fmt` applied
- [ ] All tests pass (`cargo test`)
- [ ] Documentation complete (rustdoc on public items)
- [ ] No deprecated crates (check RUSTSEC advisories)
- [ ] Cross-platform paths use `Path` / `PathBuf`
- [ ] Async code uses tokio; no blocking in async context
- [ ] No unbounded allocations
- [ ] Error types are semantic (not `String`)
- [ ] Naming follows RFC 430 conventions

---

## 22. LLM PROMPT TEMPLATE

When requesting Rust code from LLM, use this structure:
```markdown
# Task: [description]

## Requirements
- Error handling: `?` operator, no `unwrap()`
- Async: tokio / sync
- Dependencies: [approved crates + versions]
- Target: nightly Rust, MSRV 1.85

## Constraints
- No allocations in hot paths
- Cross-platform (Windows primary, macOS/Linux compatible)
- Production-ready, clippy-clean

## Example input/output
[concrete example]

## Testing
Include unit tests for success and failure cases.
```

---

## REFERENCES

| Resource | URL |
|----------|-----|
| Rust Book | https://doc.rust-lang.org/book/ |
| Rust API Guidelines | https://rust-lang.github.io/api-guidelines/ |
| Rust Performance Book | https://nnethercote.github.io/perf-book/ |
| Tokio | https://tokio.rs/ |
| Safety-Critical Rust | https://coding-guidelines.arewesafetycriticalyet.org/ |
| egui | https://github.com/emilk/egui |
| QtBridges | https://www.qt.io/qt-bridges |
| Microsoft Rust Training | https://github.com/microsoft/RustTraining |
| Ferrocene (safety Rust) | https://ferrocene.dev/ |
| Cargo Book | https://doc.rust-lang.org/cargo/ |
| Clippy Lints | https://rust-lang.github.io/rust-clippy/ |

---
