# Repository Guidelines

## Project Structure & Modules
- `src/` holds the app: `main.rs` entry/UI wiring, `entities/` (frames, comps, loaders), `dialogs/` (encode/open UI flows), `widgets/` (custom egui controls), `utils/` + `utils.rs` (helpers), `player.rs` (playback/threads), `events.rs` (app events).
- Build helpers live in `xtask/` (crate-driven automation) and `build.rs`. Assets/screenshots sit in `.github/` and `docs/` contains working notes.
- Binaries land in `target/{debug,release}/`; shaders and copied native libs are placed alongside the built `playa` binary by the build steps.

## Build, Test, and Development Commands
```bash
# Fast local build (exrs backend)
.\bootstrap.ps1 build           # Windows
./bootstrap.sh build            # Linux/macOS

# Full OpenEXR backend
cargo xtask build --openexr [--release]

# Run tests (defaults to exrs backend)
cargo test
cargo test --features openexr   # if touching OpenEXR paths

# Lint/format
cargo clippy -- -D warnings
cargo fmt --check
```
- `cargo xtask deploy` installs to the user bin dir; `cargo xtask post` copies native libs for OpenEXR builds.

## Coding Style & Naming Conventions
- Rust defaults via `cargo fmt`; 4-space indentation, `snake_case` for modules/functions, `CamelCase` for types.
- Keep public APIs minimal; prefer `anyhow::Result`/`Context` for error paths (matches existing code).
- GUI code uses egui/eframe; keep widget logic in `widgets/`, dialogs in `dialogs/`, and avoid UI work in core `entities/`.

## Testing Guidelines
- Unit tests live inline (`mod tests` blocks) across modules; favor small, deterministic cases.
- Run `cargo test` before PRs; add `--features openexr` when modifying OpenEXR-backed paths to ensure DWAA/DWAB handling compiles.
- For performance-sensitive changes, add perf checks or benchmarks locally (not currently in CI).

## Commit & Pull Request Guidelines
- Follow Conventional Commits (`feat:`, `fix:`, `refactor:`, `test:`, `chore:`); changelog automation depends on it.
- PRs should complete the template in `.github/pull_request_template.md`: clear description, checkbox for tests (`cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check`), note breaking changes, and link issues (`Fixes #123`).
- Prefer small, focused PRs with screenshots/gifs for UI changes; mention platform tested (Windows/macOS/Linux) when relevant.

## Security & Configuration Tips
- Windows builds auto-discover FFmpeg via vcpkg; ensure `%VCPKG_ROOT%` is set when using system packages.
- OpenEXR backend needs C++ toolchain/CMake; run `cargo xtask pre` on Linux if headers need patching. Keep API keys or private footage paths out of commits.
# Repository Guidelines

## Project Structure & Module Organization
- `src/` holds the runtime: `entities` (comp, frame, loader, attrs), `widgets` (timeline, viewport, project tree), `dialogs` (prefs/encode), and `utils`. UI helpers live next to their widgets to keep context close.
- `docs/` contains flow notes and specs; `xtask/` wraps build helpers; `start.cmd` bootstraps a local run on Windows.
- Assets, caches, and build outputs stay under `target/`; do not check them in. Legacy references live in `.orig`/`.bak` for archaeology only.

## Build, Test, and Development Commands
- `./start.cmd` — fast path to run the Windows dev stack; wraps `cargo run` with the correct env.
- `cargo run --bin playa` — launch the app from the current sources.
- `cargo test` — run unit tests; prefer targeted filters (e.g., `cargo test frame::`).
- `cargo fmt && cargo clippy --all-targets --all-features` — required before sending patches; fix or annotate clippy warnings.

## Coding Style & Naming Conventions
- Rust 2021, 4‑space indent, snake_case for funcs/fields, UpperCamelCase for types. Keep line lengths readable (~120 cols).
- Favor small helpers in the owning module (e.g., timeline helpers) instead of new files unless the type is reused broadly.
- Log via `log` macros (`debug!`/`warn!`) instead of `println!`; keep messages actionable and frame-addressed.
- Attr keys remain snake_case (`start`, `play_start`, `width`), and timeline math is absolute; negative frames are allowed when clips are shifted left.

## Testing Guidelines
- Add unit tests for math-heavy code (frame indexing, bounds, loaders) and snapshot UI assertions only when deterministic.
- When touching loaders/compositor, validate with `cargo test loader compositor` and sanity-run a short session via `cargo run` to ensure frames decode and placeholders display.
- Prefer cheap, isolated tests over slow integration; mock filesystem paths where possible.

## Commit & Pull Request Guidelines
- Message format: short imperative summary plus context in the body when needed (`Fix file-comp placeholder offset`, `Explain bounds rebalance`).
- PRs must describe repro, the chosen approach, and visual diffs or logs when UI/UX changes apply. Link tickets or log lines when referencing bugs.
- Keep diffs focused: separate refactors from behavior changes; update docs (`docs/frame_flow.md`, `plan_cdx.md`) alongside code changes.

## Timeline & Playback Notes
- Comps must rebound their `start/end` from children on activation; keep play_range aligned only when the user had the full-range default.
- Frame requests should early-return placeholder buffers (sized from comp attrs) when outside play range or media bounds, and loaders should skip frames with no backing file path.
