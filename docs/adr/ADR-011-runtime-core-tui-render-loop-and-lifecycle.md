# ADR-011: Runtime-core TUI render loop and lifecycle

**Date:** 2026-02-19
**Status:** Accepted
**Deciders:** Core maintainer
**Related tasks:** Runtime-core TUI feature track (follow-up manifests)
**Supersedes operationally:** none (complements ADR-007 through ADR-010)

## Context

Runtime-core dispatch is established, but a production-quality ratatui app also
needs explicit render-loop, frame scheduling, and terminal lifecycle guarantees.

## Decision (normative)

1. MUST: TUI render scheduling MUST be event-driven with controlled tick policy,
   not unconditional redraw on every loop iteration.
2. MUST: The frontend MUST redraw on state changes (input, model updates,
   overlay changes) and periodic ticks only when required (cursor blink, timers).
3. MUST: Frame cadence and poll interval MUST be configurable via bounded env
   settings with safe defaults.
4. MUST: Terminal lifecycle MUST be resilient:
   raw mode, bracketed paste, and cursor visibility are restored on normal exit,
   panic, and interruption paths.
5. MUST: Runtime loop behavior under idle conditions MUST avoid unnecessary CPU
   load from hot redraw loops.
6. MUST: Runtime/UI error paths MUST keep terminal state recoverable and emit
   visible diagnostics.

## Rationale

Conventional ratatui applications prioritize predictable responsiveness, stable
terminal restoration, and bounded idle resource use.

## Consequences

1. Runtime/frontend require dirty-state tracking and tick management.
2. Additional lifecycle tests and manual verification points.
3. Some complexity increase in render scheduling.

## Compliance checks

1. Tests for render-trigger behavior (state-driven vs idle loops).
2. Tests/manual checks for terminal restoration on panic and cancellation.
3. Profiling/checks showing bounded idle redraw behavior.
