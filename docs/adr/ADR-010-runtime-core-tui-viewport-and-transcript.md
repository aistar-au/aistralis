# ADR-010: Runtime-core TUI viewport and transcript model

**Date:** 2026-02-19
**Status:** Accepted
**Deciders:** Core maintainer
**Related tasks:** Runtime-core TUI feature track (follow-up manifests)
**Supersedes operationally:** none (complements ADR-006 through ADR-009)

## Context

The runtime-core TUI currently has minimal transcript rendering. A conventional
ratatui interface requires explicit rules for scrollback, transcript structure,
and overlays.

## Decision (normative)

1. MUST: Transcript state MUST be modelled as structured entries (user,
   assistant, tool, system/error) rather than implicit plain-string coupling.
2. MUST: Message viewport MUST support scrollback navigation:
   `PageUp`, `PageDown`, `Home`, `End`, and auto-follow behavior.
3. MUST: New incoming output while scrolled up MUST NOT force-scroll the
   viewport to bottom unless auto-follow is enabled.
4. MUST: Transcript retention MUST be bounded (ring buffer or compaction policy)
   to prevent unbounded memory growth.
5. MUST: Approval and error overlays MUST be rendered as explicit modal/overlay
   surfaces, not only status-line text.
6. MUST: Overlay focus rules MUST define which inputs are consumed by overlay
   vs compose editor.
7. MUST: Active TUI composition MUST preserve a deterministic three-area base
   frame (`header/status`, transcript viewport, compose input), with overlays
   rendered as a top layer that does not alter base-pane geometry.

## Rationale

Conventional terminal chat UX depends on predictable scroll behavior and explicit
focus management, especially during tool approvals and long sessions.

## Consequences

1. Additional UI state for viewport offset and auto-follow.
2. Additional structured transcript types.
3. Additional tests for overlay focus and scroll correctness.
4. Deterministic frame composition checks for base-pane order and overlay z-order.

## Compliance checks

1. Tests for viewport navigation and scroll retention under streaming updates.
2. Tests for bounded transcript memory policy.
3. Tests that approval overlay is visible and focus-correct.
4. Tests that base-frame composition remains `header/status -> transcript -> input`
   and overlay render occurs last without changing pane geometry.
