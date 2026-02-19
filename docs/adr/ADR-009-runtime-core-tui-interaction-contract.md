# ADR-009: Runtime-core TUI interaction contract

**Date:** 2026-02-19
**Status:** Accepted
**Deciders:** Core maintainer
**Related tasks:** Runtime-core TUI feature track (follow-up manifests)
**Supersedes operationally:** none (complements ADR-007 and ADR-008)

## Context

REF-08 established canonical runtime dispatch and baseline TUI behavior. To ship a
fully featured, conventional ratatui TUI, interaction semantics must be explicit
and testable.

## Decision (normative)

1. MUST: Submitted user input MUST NOT be silently dropped.
   If a turn is already in progress, submitted text must be queued, restored to
   the editor, or rejected with explicit user-visible feedback.
2. MUST: `Ctrl+C` behavior MUST be conventional and deterministic.
   If a turn is active, it cancels that turn.
   If no turn is active, it initiates exit behavior (immediate or configured
   double-press policy), with explicit status feedback.
3. MUST: Tool approval interaction MUST support direct single-key resolution
   (`1/y`, `2/a`, `3/n`, `Esc`) without requiring `Enter`.
4. MUST: Approval mode input handling MUST be isolated from normal compose mode.
5. MUST: Keybinding policy MUST be documented and covered by tests for:
   submit, cancel/interrupt, history navigation, multiline entry, and approval
   decisions.

## Rationale

Conventional TUI behavior requires predictable interrupt semantics and durable
input handling. Hidden input loss is unacceptable for operator trust.

## Consequences

1. Runtime mode/frontend must track compose state and in-flight turn state with
   explicit transitions.
2. Approval UI needs dedicated key routing.
3. Tests must verify non-lossy input and interrupt policy.

## Compliance checks

1. Unit tests proving no silent input drop during active turns.
2. Unit tests proving idle `Ctrl+C` exit behavior and active-turn cancellation.
3. Unit/integration tests for direct single-key tool approval decisions.
