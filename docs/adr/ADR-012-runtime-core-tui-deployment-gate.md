# ADR-012: Runtime-core TUI deployment gate

**Date:** 2026-02-19
**Status:** Accepted
**Deciders:** Core maintainer
**Related tasks:** `TASKS/completed/REF-08-full-runtime-cutover.md`,
`TASKS/completed/REF-08-deltas/`, runtime-core TUI feature follow-up manifests
**Supersedes operationally:** none (consolidates ADR-009, ADR-010, ADR-011 for release gating)

## Context

A full codebase review identified five blocking gaps for a conventional
production ratatui TUI:

1. submitted input can be silently dropped while a turn is active,
2. idle `Ctrl+C` is a no-op,
3. scrollback and viewport controls are missing in the active path,
4. tool approval modal is defined but not rendered,
5. transcript growth is unbounded in memory.

ADR-009 through ADR-011 define category-level contracts. This ADR defines the
single release gate used to decide if TUI deployment is allowed.

## Decision (normative)

Deployment of the runtime-core TUI is blocked until all conditions below are
implemented and verified:

1. Input durability:
   user submit while `turn_in_progress` MUST be preserved (queue, restore, or
   explicit reject with visible feedback). Silent drop is forbidden.
2. Interrupt behavior:
   idle `Ctrl+C` MUST trigger defined exit behavior; active-turn `Ctrl+C` MUST
   cancel exactly one active turn.
3. Viewport/scrollback:
   transcript viewport MUST support `PageUp`, `PageDown`, `Home`, `End`, and
   auto-follow state.
4. Overlay rendering and frame composition:
   approval/error overlays MUST render as modal surfaces on top of a
   deterministic three-area base frame (`header/status`, transcript viewport,
   compose input), MUST own focus while active, and MUST NOT alter base-pane
   geometry.
5. Transcript retention:
   transcript memory MUST be bounded (ring buffer or compaction).
6. Render-loop efficiency:
   frontend redraw MUST be state/tick-driven; hot idle redraw loops are
   forbidden.
7. Terminal lifecycle:
   raw mode, cursor visibility, and bracketed paste MUST restore on normal
   exit, panic, and interrupt paths.

## Required verification before deployment

1. Existing regression suites remain green:
   `cargo test --all-targets`
2. Architecture contracts remain green:
   `bash scripts/check_no_alternate_routing.sh`
   `bash scripts/check_forbidden_imports.sh`
3. New targeted tests are required for each gate above:
   input preservation, idle `Ctrl+C`, scrollback behavior, overlay visibility
   and focus routing, three-area frame composition and overlay z-order/geometry
   invariance, transcript retention, idle redraw behavior, terminal restore
   behavior.

## No-go policy

If any gate item fails, deployment is not permitted and the release is blocked.

## Rationale

This consolidates all review findings into a single operational deployment rule,
so TUI release decisions are deterministic and auditable.
