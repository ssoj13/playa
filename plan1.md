# Plan

1) Project recon: scan repository structure, configs, deps, and build scripts to understand architecture, targets, and event bus setup.
2) EventBus mapping: locate event definitions, publishers, and subscribers; map all controls and UI triggers to events; enumerate missing/duplicate wiring.
3) Hotkeys and input controls: inventory keybindings, shortcuts, and their handlers; verify event-based flow and conflicts.
4) Static analysis for dead/unused code: search for unused modules/types/functions, incomplete refactors, and dangling assets; note rationale and removal/cleanup plan.
5) Runtime/behavior checks: run relevant automated tests or lightweight app invocations to confirm handler wiring and surface runtime errors.
6) Issue list: document illogical behavior, errors, gaps, and propose best-practice, minimal, elegant fixes (no feature cuts); link each to files/lines.
7) Reporting: prepare comprehensive findings and recommended solutions; outline implementation steps and validation plan.
