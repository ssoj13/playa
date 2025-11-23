# Repository Guidelines

## Project Structure & Module Organization
- `src/main.rs` drives the app lifecycle; `player.rs`, `events.rs`, `ui.rs`, and `workers.rs` orchestrate playback, UI, and background tasks. Subdirs: `entities/` (data/state models), `dialogs/` (UI popups), `widgets/` (egui components), `utils/` (helpers).
- `xtask/` is the workspace helper for builds/releases; prefer its commands over ad-hoc scripts. `bootstrap.ps1` / `bootstrap.sh` set up vcpkg, cargo tools, then forward to `cargo xtask`.
- Optional EXR backend lives behind the `openexr` feature in `Cargo.toml`; build artifacts land in `target/`. CI/workflow config is under `.github/`; packaging details live in `build.rs`.

## Build, Test, and Development Commands
- You always build with start.cmd or it's not gonna work
- Direct automation: still with start.cmd: `start.cmd`
- Run tests: `.\bootstrap.ps1 test` / `./bootstrap.sh test` (forwards to `cargo test`). For quick checks, `cargo test --all-targets`.
- Hygiene before PRs: `cargo fmt --all` and `cargo clippy --all-targets --all-features -D warnings`. Launch locally with `cargo run` or `cargo run --features openexr` to spot runtime regressions.

## Coding Style & Naming Conventions
- Rust 2024, 4-space indent, rustfmt defaults. Prefer `anyhow::Result` with `?` for error flow and structured logs via `env_logger`/`log` (avoid `println!`).
- Naming: snake_case for modules/functions, CamelCase types, SCREAMING_SNAKE consts. Keep UI/state helpers inside `widgets/` and `entities/`; file IO/config logic belongs in `config.rs`/`utils/`.

## Testing Guidelines
- Add unit tests next to code with `#[cfg(test)]`; place broader scenarios in `tests/` if introduced. Use temp dirs for file/sequence fixtures and avoid GPU-specific expectations.
- Cover sequence detection, FFmpeg/vcpkg path handling, event routing, and player state transitions; prefer deterministic, headless tests for egui logic.

## Commit & Pull Request Guidelines
- Conventional Commits (see `cliff.toml`) keep the changelog clean: `feat(ui): add shuttle indicator`, `fix(io): guard missing FFmpeg libs`, `chore: bump deps`.
- Develop on feature branches; target PRs at `dev`, then promote releases to `main` via `cargo xtask pr` / `cargo xtask tag-*`.
- PR checklist: describe the change and rationale, list commands run (`cargo fmt`, `cargo clippy`, tests/build backend used), note platform/backend (exrs vs OpenEXR), and attach screenshots for UI changes.



