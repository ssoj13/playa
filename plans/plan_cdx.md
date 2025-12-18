# Codex Control File — Playa Audit

This file is the short, action-oriented control surface for the longer report:
- See `plan1.md` for full details and rationale.

## Approval Checklist

Choose what you want me to implement next (check one or more).

### Stage 0 — Correctness hotfixes
- [+] Fix CPU compositor crop to avoid mutating cached frames (`src/entities/compositor.rs:282`).
- [+] Add regression test for the cache-mutation bug.

### Stage 1 — Settings correctness
- [+] Ensure `AppSettings.cache_strategy` is applied after startup restore and after project/playlist loads.

### Stage 2 — Legacy cleanup (no feature removal)
- [+] Remove or rewire legacy stubs (`CompNode::emit_attrs_changed`, `CompEventEmitter` plumbing).
- [+] Remove or handle unused events (`CompositorBackendChangedEvent`, `PreloadFrameEvent`).

### Stage 3 — GPU compositor roadmap
- [+] Implement viewport-only GPU blending using cache-only source frames (incremental).
- [+] Rework preload to avoid redundant CPU compositing when GPU is selected (larger change).

### Stage 4 — Clippy hardening
- [+] Fix `cargo clippy -p playa --all-targets -- -D warnings`.
- [+] Fix `cargo clippy --workspace --all-targets -- -D warnings` (includes `xtask`).

## Default Recommendation
- Start with Stage 0 + Stage 1.

## Notes
- I will keep changes minimal and reviewable (small PR-sized chunks).
- I will not remove user-visible features.
