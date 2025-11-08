# Repository Guidelines

## Project Structure & Module Organization
Playa is a two-member workspace: the core application lives at the repository root while automation utilities sit in `xtask/`. Entry begins in `src/main.rs`, with focused modules like `player.rs` (UI state), `sequence.rs` (frame discovery), `cache.rs` (LRU + async loading), and `viewport.rs` (OpenGL view). Unit tests typically sit inline with those modules; add new files under `src/` only when functionality warrants a dedicated namespace. Installer metadata, icons, and packaging glue live in `Cargo.toml`, `build.rs`, and `icon.png`. Build artifacts go to `target/`—never commit its contents.

## Build, Test, and Development Commands
- `cargo build --release` – default exrs backend, optimized binary in `target/release/playa`.
- `cargo build --release --features openexr` – enables the OpenEXR C++ stack (requires a C++ compiler + CMake).
- `cargo xtask build [--debug|--openexr]` – orchestrated builds that also copy native deps/shaders; prefer this for anything packaged.
- `cargo xtask verify` – confirms shared libraries are present before distributing builds.
- `cargo xtask wipe` – removes stale binaries so exrs builds don’t ingest wrong artifacts.
- `cargo test` / `cargo test --release` – runs module tests; release mode catches timing/caching regressions.
- `cargo clippy -- -D warnings` and `cargo fmt` – mandatory before any PR.

## Coding Style & Naming Conventions
Use Rust 2024 defaults with 4-space indentation; `cargo fmt` is the source of truth. Modules/files should mirror one another (e.g., `sequence.rs` → `pub mod sequence`). Functions and locals stay `snake_case`, types `CamelCase`, constants `SCREAMING_SNAKE_CASE`. Favor `anyhow::Result` with `.context()` over bare `unwrap()`. When touching UI code, keep egui layout helpers declarative and prefer `Arc<Mutex<_>>` patterns already in `player.rs`.

## Testing Guidelines
Add unit tests beside the logic they exercise (e.g., `#[cfg(test)] mod tests` within `sequence.rs`). Run `cargo test --features openexr` whenever you change EXR-loading paths so both backends stay green. Target cache/sequence regressions with table-driven tests that simulate frame patterns rather than loading assets. There is no formal coverage gate, but pull requests should justify risky areas and explain any manual verification (e.g., `cargo run -- --sequence shots/render.0001.exr`).

## Commit & Pull Request Guidelines
Follow Conventional Commits (`feat(ui):`, `fix(cache):`, etc.) as enforced in `CONTRIBUTING.md`. The optional helper `gpush2.cmd "feat: …"` stages, commits, and pushes with the correct prefix. Each PR must link the relevant issue, describe how to reproduce/verify, and include screenshots or screen recordings for UI-facing work. Before requesting review, run `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test [--release]`, and mention any skipped steps in the PR template.

## Security & Configuration Tips
Never commit signing material; use `apple_cert.sh` to export Developer ID certificates and load them into the `APPLE_CERTIFICATE`/`APPLE_CERTIFICATE_PASSWORD` GitHub secrets expected by CI. Local configs resolve in priority: `--config-dir` CLI flag, `PLAYA_CONFIG_DIR`, existing files in the working folder, then platform defaults (XDG, `%APPDATA%`, or `~/Library/Application Support`). Document any config shape changes in `README.md` and double-check that `cargo xtask verify` still succeeds before shipping installers.
