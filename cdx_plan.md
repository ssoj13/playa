Analysis plan for hotkey issue (F/A working in timeline but not viewport)
1) Map hotkey flow: identify hotkey hub, EventBus usage, message types, and handlers for timeline vs viewport; note window detection logic.
2) Trace viewport input handling: how focus/active window is determined, how EventBus subscriptions are set up, and how F/A are consumed; contrast with timeline handling.
3) Inspect recent changes or TODOs: search for partial implementations, gating flags, or commented-out code affecting viewport hotkeys.
4) Summarize findings: enumerate bugs/misbehaviors, explain causes, and propose concrete fixes/tests; prepare to run verification steps on command.
