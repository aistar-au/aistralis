# REF-08 DELTA-E: Typed interrupt routing (no magic string)

## Problem

Interrupt behavior must not rely on a sentinel text payload because user text can
collide with that sentinel.

## Targets

1. `src/runtime/frontend.rs`
2. `src/runtime/loop.rs`
3. `src/runtime/mode.rs`
4. `src/app/mod.rs`

## Decision

1. Represent interrupt as `UserInputEvent::Interrupt`.
2. Keep user text as `UserInputEvent::Text(String)`.
3. Add `RuntimeMode::on_interrupt(&mut self, ctx: &mut RuntimeContext)` with a
   default no-op.
4. In `TuiMode::on_interrupt`, cancel only active turns and record cancellation
   state/history.

## Required tests

1. `test_ref_08_interrupt_dispatches_on_interrupt_only`
2. `test_interrupt_is_typed_event_not_magic_string_collision`

## Acceptance

A user-typed string that matches any former sentinel is still treated as normal
text input.
