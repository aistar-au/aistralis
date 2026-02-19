# ADR-013: TUI Completion and Deployment Plan — 3-Pane View with Overlay

**Date:** 2026-02-20
**Status:** Accepted
**Deciders:** Core maintainer
**Related tasks:** CORE-07 through CORE-14, FEAT-10 through FEAT-16
**ADR chain:** Implements ADR-012 deployment gate; governed by ADR-007, ADR-008, ADR-009, ADR-010, ADR-011
**Supersedes operationally:** nothing

---

## Context

REF-08 merged on 2026-02-19 and delivered:

- Canonical runtime dispatch: `Runtime<M>::run` → `RuntimeMode::on_user_input` → `RuntimeContext::start_turn`
- `TuiMode` with overlay state, approval handling, `InputEditor` with undo/redo and
  UTF-8 boundary safety, typed `UserInputEvent::Interrupt` routing
- Architecture contract CI: `check_no_alternate_routing.sh`, `check_forbidden_imports.sh`
- Env-test determinism via `crate::test_support::ENV_LOCK`

What REF-08 did **not** deliver — and ADR-012 requires before TUI deployment — falls
into two categories:

**Category A — Existing task manifests already written, not yet dispatched:**
CORE-07, CORE-08, CORE-09, CORE-10, CORE-11, FEAT-10, FEAT-11, FEAT-12, FEAT-13,
FEAT-14. These manifests exist in `TASKS/` and are ready to dispatch in the sequence
defined below.

**Category B — Gaps not covered by any existing manifest:**
Five runtime/lifecycle items identified during ADR-012 review that have no task
manifest. New manifests CORE-12 through CORE-14 and FEAT-15 through FEAT-16 are
created alongside this ADR.

### What the existing tasks deliver

**CORE-09** groups `App`'s ad-hoc fields into `HistoryState`, `InputState`,
`OverlayState` without protocol changes. Prerequisite for everything else.

**CORE-07** extracts pane split logic into `src/ui/layout.rs`. After this, the
frame always splits header → history → input in a fixed, testable order.

**CORE-08** fixes render order to header → history → input → overlay, with the
overlay draw as the last call in every frame and no pane geometry alteration.

**CORE-10** hard-locks input routing: `Overlay::None` → normal keymap;
`Overlay::Some` → overlay keymap only; submit is blocked while overlay is active.

**CORE-11** maps `ToolApprovalRequest` into overlay state with one-shot responder
resolution — the approval sender fires exactly once per overlay lifecycle.

**FEAT-10** through **FEAT-14** deliver the rendered modal surface family (header
stability, unified modal renderer, diff viewer, multiline input, history safety).

### What the existing tasks do not cover

| Gap | ADR-012 gate | New task |
| :--- | :--- | :--- |
| `scroll_offset: 0` is hard-coded in `render_messages` | gate #3 | FEAT-15 |
| `TuiMode::history` is unbounded `Vec<String>` | gate #5 | CORE-12 |
| `poll(Duration::from_millis(16))` redraws unconditionally | gate #6 | CORE-13 |
| No panic hook to restore terminal on unwind | gate #7 | CORE-14 |
| Idle `Ctrl+C` has no feedback; input silently dropped during turn | gate #1/#2 | FEAT-16 |

---

## Decision

Execute the work in two phases with the sequencing below.

### Phase 1 — Overlay and render correctness

| Order | Task | Target files | ADR-012 gate | Anchor test |
| :--- | :--- | :--- | :--- | :--- |
| 1 | CORE-09 | `src/app/mod.rs` | prerequisite | `ui_state_slices_compile` |
| 2 | CORE-07 | `src/ui/layout.rs` (new), `src/app/mod.rs` | #3 viewport | `layout_splits_into_three_panes` |
| 3 | CORE-08 | `src/app/mod.rs`, `src/ui/render.rs` | #4 overlay z-order | `overlay_renders_after_base_panes` |
| 4 | CORE-10 | `src/app/mod.rs` | #2 interrupt, #4 overlay | `overlay_blocks_submit` |
| 5 | CORE-11 | `src/app/mod.rs` | #4 overlay | `approval_sender_resolved_exactly_once` |
| 6 | FEAT-10 | `src/ui/render.rs`, `src/app/mod.rs` | #4 | `header_stable_during_streaming` |
| 7 | FEAT-11 | `src/ui/render.rs`, `src/app/mod.rs` | #4 modal surface | `all_modals_use_unified_renderer` |
| 8 | FEAT-12 | `src/ui/render.rs`, `src/app/mod.rs` | #4 scrollable modal | `diff_overlay_scrolls` |
| 9 | FEAT-13 | `src/app/mod.rs`, `src/ui/render.rs` | #1 input durability | `multiline_submit_outside_overlay_only` |
| 10 | FEAT-14 | `src/app/mod.rs` | #1 input durability | `history_stable_during_overlay` |

**Dependency rules:** CORE-09 has no upstream dependency and must be dispatched first.
CORE-07 and CORE-08 may be dispatched once CORE-09 anchor is green. CORE-10 and
CORE-11 require CORE-08. FEAT-10 through FEAT-14 require CORE-10 and CORE-11.

### Phase 2 — Runtime correctness and lifecycle

New task manifests at `TASKS/CORE-12-*.md`, `TASKS/CORE-13-*.md`,
`TASKS/FEAT-15-*.md`, and `TASKS/FEAT-16-*.md`, with CORE-14 archived at
`TASKS/completed/CORE-14-panic-hook-terminal-restore.md`.

| Task | Target files | ADR-012 gate | Anchor test |
| :--- | :--- | :--- | :--- |
| FEAT-15 | `src/app/mod.rs` | #3 scrollback | `scrollback_retains_position_during_streaming` |
| CORE-12 | `src/app/mod.rs` | #5 transcript retention | `transcript_does_not_exceed_cap_after_n_turns` |
| CORE-13 | `src/app/mod.rs` | #6 render efficiency | `render_not_called_when_state_unchanged` |
| CORE-14 | `src/terminal/mod.rs`, `src/app/mod.rs` | #7 terminal lifecycle | `terminal_restored_after_simulated_panic` |
| FEAT-16 | `src/app/mod.rs` | #1 + #2 | `idle_interrupt_shows_feedback`, `input_drop_shows_feedback` |

**Dispatch order for Phase 2:** CORE-14 (panic hook) is independent and low-risk — it
must be dispatched before any Phase 1 task because raw mode is already active and a
panic during Phase 1 testing leaves the terminal broken without it. FEAT-15 and CORE-12
are independent of the Phase 1 chain and may be dispatched in parallel with CORE-07
once CORE-09 is green. CORE-13 may run in parallel with CORE-10. FEAT-16 requires
CORE-10 (shared interrupt/input routing path).

---

## Normative rules added by this ADR

These extend ADR-012's gate policy with implementation-level invariants enforceable
in code review and CI:

1. **Overlay z-order:** `TuiFrontend::render` MUST draw the modal surface as the last
   draw call in every frame. No pane geometry may change due to overlay presence.

2. **Scroll parameters:** `render_messages` callers MUST pass live
   `scroll_offset` state from `TuiMode`/`HistoryState`; hard-coded `0` is forbidden.

3. **History cap:** `TuiMode::history.len()` (or the equivalent `HistoryState`
   field after CORE-09) MUST NOT exceed `MAX_HISTORY_LINES` at the end of any
   `on_model_update` or `on_user_input` call.

4. **Dirty flag:** `TuiFrontend::render` MUST NOT call `terminal.draw(...)` when
   `dirty` is false and the tick interval has not elapsed.

5. **Panic hook:** `crate::terminal::restore()` MUST be registered in the panic hook
   before raw mode is enabled. Re-registration on a second `App::new` MUST NOT stack
   hooks.

6. **No silent drop:** Any input guard that returns early MUST push a visible
   history line explaining the rejection. Silent discard is forbidden (ADR-009 §1).

7. **Idle interrupt output:** `on_interrupt` MUST produce user-visible output
   regardless of `turn_in_progress` state (ADR-009 §2).

8. **Scope discipline:** CORE-07 through CORE-14 and FEAT-10 through FEAT-16 MUST NOT
   add new `UiUpdate` variants, new env vars (except `MAX_HISTORY_LINES`), or touch
   `src/state/`, `src/api/`, or `src/tools/`.

---

## Task sequencing diagram

```
CORE-14 (panic hook) ──── dispatch first, independent of all below

CORE-09
  ├── CORE-07
  │     └── CORE-08
  │           ├── CORE-10 ────────── FEAT-16 (idle interrupt + drop feedback)
  │           │     └── CORE-11
  │           │           ├── FEAT-10
  │           │           ├── FEAT-11
  │           │           │     └── FEAT-12
  │           │           ├── FEAT-13
  │           │           └── FEAT-14
  │           └── CORE-13 (dirty render guard)
  ├── FEAT-15 (scrollback)          ← parallel with CORE-07 chain
  └── CORE-12 (bounded transcript)  ← parallel with CORE-07 chain
```

---

## ADR-012 gate verification matrix

All of the following must be green before TUI deployment:

```bash
# Ongoing regression suite — must stay green throughout
cargo test --all-targets
bash scripts/check_no_alternate_routing.sh
bash scripts/check_forbidden_imports.sh

# Phase 1 anchors
cargo test layout_splits_into_three_panes
cargo test overlay_renders_after_base_panes
cargo test overlay_blocks_submit
cargo test approval_sender_resolved_exactly_once
cargo test header_stable_during_streaming
cargo test all_modals_use_unified_renderer
cargo test diff_overlay_scrolls
cargo test multiline_submit_outside_overlay_only
cargo test history_stable_during_overlay

# Phase 2 anchors
cargo test scrollback_retains_position_during_streaming
cargo test transcript_does_not_exceed_cap_after_n_turns
cargo test render_not_called_when_state_unchanged
cargo test terminal_restored_after_simulated_panic
cargo test idle_interrupt_shows_feedback
cargo test input_drop_shows_feedback
```

---

## Out of scope for this ADR

- CORE-02 / CORE-03 / CORE-04 (mdBook docs scaffold and CI workflow) — docs
  publishing is independent of TUI correctness and must not block TUI deployment.
- DOC-01 / DOC-02 — contributing guide editorial changes.
- Network or backend changes — no new API protocol paths.
- A second `RuntimeMode` implementation (batch/headless) — separate track.

---

## Consequences

**After Phase 1:** The approval overlay is rendered as a modal surface that owns
focus. Frame composition order is deterministic and tested. All `UiUpdate` variants
have an explicit overlay mapping with one-shot resolution guarantees.

**After Phase 2:** The terminal is unconditionally restored on normal exit, panic,
and interrupt. Transcript memory is bounded. Idle redraw cost is eliminated. Users
receive visible feedback on every rejected or interrupted input.

**ADR-012 no-go policy is satisfied** when all Phase 1 and Phase 2 anchor tests are
green and all three CI scripts pass.

---

## Compliance notes for agents

1. CORE-09 is the first task. Do not start CORE-07, CORE-08, or any later task
   until `ui_state_slices_compile` is green.
2. CORE-14 (panic hook) is independent — dispatch it before any Phase 1 task.
3. FEAT-15 and CORE-12 may be dispatched in parallel with Phase 1 starting from
   CORE-07. They share no files with CORE-07 through CORE-11.
4. Do not add new `UiUpdate` variants. All overlay routing uses existing variants.
5. Do not touch `src/state/`, `src/api/`, or `src/tools/` during this track.
6. `cargo test --all-targets` must be green after every task. Prior anchor tests
   must not regress.
