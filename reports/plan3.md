# Plan 3: Critical EventBus Bug Fix Report

**Date**: 2025-12-05
**Status**: COMPLETED

## Executive Summary

Fixed a critical bug in the EventBus system that caused **all event handlers to silently fail**. The bug was in `downcast_event()` function in `event_bus.rs` where Rust's method resolution selected the wrong trait implementation.

## Problem Description

### Symptoms
- All Project window buttons (Add Comp, Clear All, Delete) appeared to not work
- Events were being emitted correctly (verified by logging)
- Events were being polled from the queue correctly
- But `downcast_event::<EventType>()` always returned `None`

### Root Cause

The bug is caused by Rust's blanket implementation interaction with trait objects.

```rust
// In event_bus.rs
pub trait Event: Any + Send + Sync + 'static {
    fn as_any(&self) -> &dyn Any;
    fn type_name(&self) -> &'static str;
}

// Blanket impl for ALL types that qualify
impl<T: Any + Send + Sync + 'static> Event for T {
    fn as_any(&self) -> &dyn Any {
        self
    }
}
```

**The problem**: `Box<dyn Event>` ALSO implements `Event` through this blanket impl because:
- `Box<T>: Any` for any `T: 'static`
- `Box<dyn Event>: Send + Sync` because `dyn Event: Send + Sync`

When calling `event.as_any()` on `&BoxedEvent` (i.e., `&Box<dyn Event>`), Rust's method resolution picks the blanket impl for `Box<dyn Event>` **before** attempting to deref to `dyn Event`.

This causes `as_any()` to return `&dyn Any` containing `Box<dyn Event>`'s TypeId, not the original event type's TypeId!

### Why downcast always failed

```rust
// BROKEN CODE:
pub fn downcast_event<E: Event>(event: &BoxedEvent) -> Option<&E> {
    event.as_any().downcast_ref::<E>()  // as_any returns Box's TypeId!
}
```

The `downcast_ref::<AddCompEvent>()` checks if the TypeId matches `AddCompEvent`, but `as_any()` returned TypeId for `Box<dyn Event>`, so it never matches.

## The Fix

Use explicit dereference to force the call through `dyn Event`'s vtable:

```rust
// FIXED CODE:
pub fn downcast_event<E: Event>(event: &BoxedEvent) -> Option<&E> {
    (**event).as_any().downcast_ref::<E>()
    //^^ explicit deref: Box<dyn Event> -> dyn Event
}
```

## Files Modified

### 1. `src/event_bus.rs`
- **Line 265**: Changed `event.as_any()` to `(**event).as_any()` in `downcast_event()`
- **Line 125**: Changed `event.as_any()` to `(*event).as_any()` in `EventBus::emit_boxed()`
- **Line 226**: Changed `event.as_any()` to `(*event).as_any()` in `EventEmitter::emit_boxed()`
- Added comprehensive documentation explaining the bug and fix

### 2. `src/main_events.rs`
- Added module-level documentation (lines 1-18) explaining the bug for future reference

### 3. Debug logging cleanup
- Removed temporary debug logging from `main.rs` and `project_ui.rs`

## Project Window Dataflow (Verified Working)

```
┌─────────────────────────────────────────────────────────────────────┐
│                    PROJECT WINDOW DATAFLOW                          │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  1. User clicks button (e.g., "Add Comp")                          │
│                          ↓                                          │
│  2. project_ui.rs render() creates event                            │
│     actions.send(AddCompEvent { name, fps })                        │
│                          ↓                                          │
│  3. main.rs dispatches events                                       │
│     self.event_bus.emit_boxed(evt)                                  │
│                          ↓                                          │
│  4. EventBus queues event (Vec<BoxedEvent>)                         │
│                          ↓                                          │
│  5. handle_events() polls queue (next frame)                        │
│     let events = self.event_bus.poll()                              │
│                          ↓                                          │
│  6. main_events.rs::handle_app_event() downcasts                    │
│     if let Some(e) = downcast_event::<AddCompEvent>(&event)         │
│                          ↓                                          │
│  7. Returns EventResult with action                                 │
│     result.new_comp = Some((name, fps))                             │
│                          ↓                                          │
│  8. main.rs executes deferred action                                │
│     project.create_comp(&name, fps, emitter)                        │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

## Testing

Verified all Project window buttons now work:
- **Save/Load**: File dialogs appear, projects save/load correctly
- **Add Clip**: File dialog opens, clips are added to project
- **Add Comp**: New composition created and activated
- **Clear All**: All media removed from project
- **Delete (X) button**: Individual clips/comps removed

## Lessons Learned

1. **Blanket implementations can be dangerous with trait objects** - They may intercept method calls you expect to go through vtable dispatch.

2. **TypeId comparison alone isn't sufficient for debugging** - The TypeId can match at one point but fail at another if you're comparing different things.

3. **Explicit dereferencing matters** - When working with `Box<dyn Trait>`, be explicit about whether you want to call methods on the Box or on the inner trait object.

4. **Test your event systems thoroughly** - Silent failures in event dispatch can be very hard to debug.

## Memory Note

Bug details saved to MCP memory as entity `PlayaEventBusBug` for future reference.

## Checklist

- [x] Identified root cause
- [x] Fixed `downcast_event()` function
- [x] Fixed `emit_boxed()` functions (2 locations)
- [x] Added documentation to `event_bus.rs`
- [x] Added documentation to `main_events.rs`
- [x] Removed debug logging
- [x] Verified build succeeds
- [x] Verified all Project buttons work
- [x] Saved findings to MCP memory
- [x] Created comprehensive report
