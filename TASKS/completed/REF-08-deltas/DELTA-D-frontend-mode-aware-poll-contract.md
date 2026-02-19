# REF-08 DELTA-D: Strict frontend poll contract

## Problem

The frontend poll path must be mode-aware and typed; returning plain `String`
from poll does not carry interrupt semantics safely.

## Targets

1. `src/runtime/frontend.rs`
2. `src/runtime/loop.rs`
3. `src/runtime/mod.rs` tests
4. `src/runtime/loop.rs` tests

## Decision

1. Use typed frontend trait:
   `pub trait FrontendAdapter<M: RuntimeMode> { fn poll_user_input(&mut self, mode: &M) -> Option<UserInputEvent>; fn render(&mut self, mode: &M); fn should_quit(&self) -> bool; }`
2. In runtime loop call:
   `frontend.poll_user_input(&self.mode)`.
3. Route poll events explicitly:
   1. `UserInputEvent::Text(text)` -> `RuntimeMode::on_user_input`.
   2. `UserInputEvent::Interrupt` -> `RuntimeMode::on_interrupt`.
4. Update all frontend implementations and test stubs to typed signature.

## Required test/compile gate

1. Runtime loop tests compile with the updated trait signature.
2. Interrupt-only event dispatch is covered by
   `test_ref_08_interrupt_dispatches_on_interrupt_only`.

## Acceptance

No `FrontendAdapter` implementation exists without typed mode-aware poll.
