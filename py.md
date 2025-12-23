# Python API for Playa: PyO3+Maturin vs RustPython

## Executive Summary

| Criteria | PyO3 + Maturin | RustPython |
|----------|----------------|------------|
| **Use Case** | Python extension module (.pyd/.so) | Embedded Python interpreter |
| **Direction** | Rust -> Python (export) | Python -> Rust (embed) |
| **Performance** | Native speed, zero-copy possible | Interpreter overhead |
| **Ecosystem** | Full CPython ecosystem (numpy, etc) | Limited stdlib, no C extensions |
| **Complexity** | Medium | High |
| **Maturity** | Production-ready | Alpha/Beta |
| **Recommendation** | **Winner** | Not suitable |

---

## Option 1: PyO3 + Maturin (RECOMMENDED)

### What It Is
- **PyO3**: Rust bindings for Python C API
- **Maturin**: Build tool for Rust Python extensions

### Architecture
```
playa-py/                    # New crate
  Cargo.toml                 # [lib] crate-type = ["cdylib"]
  pyproject.toml             # maturin config
  src/
    lib.rs                   # #[pymodule] entry point
    project.rs               # PyProject wrapper
    comp.rs                  # PyCompNode wrapper
    frame.rs                 # PyFrame with numpy integration
    ...
```

### How It Works
```rust
use pyo3::prelude::*;

#[pyclass]
struct PyProject {
    inner: Arc<RwLock<Project>>,
}

#[pymethods]
impl PyProject {
    #[new]
    fn new() -> Self {
        PyProject { inner: Arc::new(RwLock::new(Project::new())) }
    }

    fn add_comp(&mut self, name: &str) -> PyResult<PyCompNode> {
        let mut proj = self.inner.write().unwrap();
        let uuid = proj.add_comp(name);
        Ok(PyCompNode { project: self.inner.clone(), uuid })
    }

    fn to_json(&self) -> PyResult<String> {
        let proj = self.inner.read().unwrap();
        proj.to_json_string().map_err(|e| PyErr::new::<pyo3::exceptions::PyIOError, _>(e))
    }
}

#[pymodule]
fn playa_py(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyProject>()?;
    m.add_class::<PyCompNode>()?;
    m.add_class::<PyFrame>()?;
    Ok(())
}
```

### Python Usage
```python
import playa_py as playa
import numpy as np

# Create project
proj = playa.Project()
comp = proj.add_comp("Main")

# Add layers
layer = comp.add_layer("/path/to/sequence.####.exr")
layer.set_attr("opacity", 0.8)
layer.set_attr("position", [100.0, 50.0, 0.0])

# Get frame as numpy array
frame = comp.render(frame=24)
pixels = frame.to_numpy()  # np.ndarray (H, W, 4), dtype=float32

# Save project
proj.save("project.playa")
```

### Pros
- Full CPython compatibility (numpy, scipy, PIL, OpenCV)
- Zero-copy pixel buffers via numpy
- Native performance (no interpreter overhead)
- Mature ecosystem, production-ready
- Easy pip install: `pip install playa-py`
- Async support via pyo3-asyncio
- GIL release for parallel Rust code

### Cons
- Requires Python installation
- Build complexity (needs Rust + Python toolchain)
- Wrapper boilerplate for each class
- Memory management between Rust/Python

### Build & Distribution
```bash
# Development
cd playa-py
maturin develop

# Release wheel
maturin build --release

# Upload to PyPI
maturin publish
```

### Effort Estimate
- Initial setup: 1-2 days
- Core classes (Project, Comp, Layer, Frame): 3-5 days
- Numpy integration: 1-2 days
- Event system bindings: 2-3 days
- Testing & docs: 2-3 days
- **Total: 2-3 weeks**

---

## Option 2: RustPython (NOT RECOMMENDED)

### What It Is
- Python interpreter written in Rust
- Embeds Python runtime inside Rust application

### Architecture
```rust
use rustpython_vm as vm;

fn main() {
    let interp = vm::Interpreter::with_init(Default::default(), |vm| {
        // Register native modules
        vm.add_native_module("playa", Box::new(playa_module));
    });

    interp.enter(|vm| {
        let scope = vm.new_scope_with_builtins();
        let code = vm.compile("import playa; p = playa.Project()", ...);
        vm.run_code_obj(code, scope);
    });
}
```

### Pros
- No external Python dependency
- Single binary distribution
- Deep Rust integration possible
- Sandboxed execution

### Cons
- **No numpy/scipy** (C extensions don't work)
- **Incomplete stdlib** (many modules missing)
- **Performance**: Interpreter is slower than CPython
- **Immature**: Many edge cases, bugs
- **Limited async**: No full asyncio support
- **Memory overhead**: Duplicates Python runtime

### Why It Doesn't Fit Playa
1. **No numpy** = Can't efficiently pass pixel buffers
2. **No OpenEXR/PIL** = Can't use Python imaging libraries
3. **Incomplete stdlib** = Many user scripts will break
4. Video/CG workflows expect full Python ecosystem

### When RustPython Makes Sense
- Scripting in games (sandboxed, no C deps)
- Config files in Python syntax
- Simple automation without external libs

---

## Detailed Comparison

### Performance

| Operation | PyO3 | RustPython |
|-----------|------|------------|
| Function call overhead | ~50ns | ~500ns |
| Pixel buffer (4K frame) | Zero-copy | Must copy |
| Parallel Rust code | GIL release | Limited |
| Startup time | ~100ms (Python init) | ~50ms |

### Ecosystem Compatibility

| Library | PyO3 | RustPython |
|---------|------|------------|
| numpy | Yes | No |
| OpenCV | Yes | No |
| PIL/Pillow | Yes | No |
| scipy | Yes | No |
| OpenEXR | Yes | No |
| PySide/PyQt | Yes | No |
| asyncio | Yes | Partial |

### Code Complexity

**PyO3 wrapper example:**
```rust
#[pyclass]
struct PyFrame {
    inner: Arc<Frame>,
}

#[pymethods]
impl PyFrame {
    #[getter]
    fn width(&self) -> u32 { self.inner.width() }

    #[getter]
    fn height(&self) -> u32 { self.inner.height() }

    fn to_numpy<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyArray4<f32>>> {
        let pixels = self.inner.pixels_f32();
        let arr = PyArray4::from_slice(py, &pixels);
        Ok(arr.reshape([self.height(), self.width(), 4])?)
    }
}
```

**RustPython equivalent:**
```rust
fn frame_to_list(vm: &VirtualMachine, frame: &Frame) -> PyResult {
    // No numpy - must return Python list (slow, memory-heavy)
    let pixels = frame.pixels_f32();
    let list: Vec<PyObjectRef> = pixels
        .chunks(4)
        .map(|px| {
            let rgba: Vec<PyObjectRef> = px.iter()
                .map(|&v| vm.ctx.new_float(v as f64).into())
                .collect();
            vm.ctx.new_list(rgba).into()
        })
        .collect();
    Ok(vm.ctx.new_list(list).into())
}
// Result: 100x slower, 10x more memory
```

---

## Implementation Plan (PyO3 + Maturin)

### Phase 1: Foundation (Week 1)
```
playa-py/
  Cargo.toml
  pyproject.toml
  src/
    lib.rs          # Module entry, error handling
    types.rs        # Uuid, Vec3, AttrValue conversions
    project.rs      # PyProject
    attrs.rs        # PyAttrs (generic attribute access)
```

**Milestone**: `import playa_py; p = playa_py.Project()`

### Phase 2: Core Entities (Week 2)
```
  src/
    comp.rs         # PyCompNode
    layer.rs        # PyLayer
    file_node.rs    # PyFileNode
    frame.rs        # PyFrame + numpy
    effects.rs      # PyEffect
```

**Milestone**: Create comp, add layers, render frame to numpy

### Phase 3: Playback & Events (Week 3)
```
  src/
    player.rs       # PyPlayer
    events.rs       # Event subscription from Python
    cache.rs        # Cache status queries
```

**Milestone**: Play/pause, subscribe to events, query cache

### Phase 4: Polish (Week 4)
- Documentation (sphinx/mkdocs)
- Type stubs (.pyi files)
- PyPI publishing
- Examples & tutorials

---

## Cargo.toml (playa-py)

```toml
[package]
name = "playa-py"
version = "0.1.0"
edition = "2021"

[lib]
name = "playa_py"
crate-type = ["cdylib"]

[dependencies]
pyo3 = { version = "0.22", features = ["extension-module"] }
numpy = "0.22"                    # numpy integration
playa = { path = ".." }           # main playa crate

[build-dependencies]
pyo3-build-config = "0.22"
```

## pyproject.toml

```toml
[build-system]
requires = ["maturin>=1.4,<2.0"]
build-backend = "maturin"

[project]
name = "playa-py"
version = "0.1.0"
description = "Python bindings for Playa video compositor"
requires-python = ">=3.9"
classifiers = [
    "Programming Language :: Rust",
    "Programming Language :: Python :: Implementation :: CPython",
]

[tool.maturin]
features = ["pyo3/extension-module"]
python-source = "python"
module-name = "playa_py"
```

---

## Classes to Expose

### Priority 1 (Essential)
| Rust | Python | Notes |
|------|--------|-------|
| `Project` | `playa.Project` | Load/save, node management |
| `CompNode` | `playa.Comp` | Layer ops, render |
| `Layer` | `playa.Layer` | Attrs, effects |
| `FileNode` | `playa.FileNode` | Sequences |
| `Frame` | `playa.Frame` | Pixels as numpy |
| `Attrs` | `playa.Attrs` | Dict-like access |

### Priority 2 (Important)
| Rust | Python | Notes |
|------|--------|-------|
| `Player` | `playa.Player` | Playback control |
| `Effect` | `playa.Effect` | Blur, HSV, etc |
| `EffectType` | `playa.EffectType` | Enum |
| `CacheManager` | `playa.Cache` | Stats, clear |

### Priority 3 (Nice to Have)
| Rust | Python | Notes |
|------|--------|-------|
| `EventBus` | `playa.EventBus` | Subscribe/emit |
| `CameraNode` | `playa.Camera` | Camera control |
| `TextNode` | `playa.Text` | Text rendering |
| `TonemapMode` | `playa.Tonemap` | HDR conversion |

---

## Example Python Scripts

### Batch Render
```python
import playa_py as playa
from pathlib import Path

proj = playa.Project.load("project.playa")
comp = proj.get_comp("Main")

for frame in range(comp.in_frame, comp.out_frame + 1):
    img = comp.render(frame)
    pixels = img.to_numpy()  # (H, W, 4) float32
    
    # Use OpenEXR, PIL, etc
    import imageio
    imageio.imwrite(f"output/frame_{frame:04d}.exr", pixels)
```

### Layer Automation
```python
import playa_py as playa

proj = playa.Project()
comp = proj.add_comp("Composite", width=1920, height=1080, fps=24)

# Add background
bg = comp.add_layer("/footage/bg.####.exr")
bg.set_attr("in", 1)
bg.set_attr("out", 100)

# Add foreground with transform
fg = comp.add_layer("/footage/fg.####.exr")
fg.set_attr("position", [100.0, 50.0, 0.0])
fg.set_attr("opacity", 0.9)
fg.add_effect(playa.EffectType.GaussianBlur, radius=5.0)

proj.save("automated.playa")
```

### Event Listener
```python
import playa_py as playa

def on_frame_change(event):
    print(f"Frame changed to {event.frame}")

player = playa.Player()
player.subscribe("frame_changed", on_frame_change)
player.play()
```

---

## Conclusion

**PyO3 + Maturin is the clear winner:**

1. Full Python ecosystem (numpy, OpenCV, PIL)
2. Production-ready, battle-tested
3. Zero-copy pixel buffers
4. Easy distribution via PyPI
5. Industry standard for Rust/Python interop

**RustPython is unsuitable** for Playa because:
1. No numpy = no efficient pixel handling
2. No C extensions = no imaging libraries
3. Immature = production risk

**Next Steps:**
1. Create `playa-py/` crate structure
2. Implement PyProject, PyCompNode, PyFrame
3. Add numpy integration for pixel buffers
4. Build wheels with maturin
5. Publish to PyPI
