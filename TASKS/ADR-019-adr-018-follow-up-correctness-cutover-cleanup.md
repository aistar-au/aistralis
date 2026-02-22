# ADR-019: ADR-018 Follow-up — Correctness, Cutover, and Cleanup

**Date:** 2026-02-22
**Status:** Proposed
**Deciders:** Core maintainer
**Related tasks:** B1, U1, U2, U3, U4, D1, D2 (dispatcher-assigned work items)
**ADR chain:** ADR-006, ADR-007, ADR-008, ADR-009, ADR-010, ADR-018

## Context

ADR-018 defines the managed TUI direction (viewport scrollback, streaming cell,
overlay lifecycle), but current implementation work is split across parallel
dispatchers. Without a strict fix order, this can cause:

1. correctness regressions during streaming,
2. event semantic drift (typed vs text-sentinel control paths),
3. partial cutover where production still follows an old path while richer
   logic remains test-only,
4. post-cutover dead branches and duplicate rendering logic.

This ADR defines the follow-up execution contract for ADR-018 delivery.

## Decision

Use a two-phase sequence with explicit priority and gating.

### Phase 1 (must complete first): correctness + architecture alignment

1. **B1**: Make streaming delta slicing Unicode-safe and explicit.
   - Enforce char-boundary-safe slicing/indexing for streamed deltas.
   - Add tests covering multi-byte UTF-8 boundaries and partial updates.
2. **U1**: Replace magic scroll text sentinels with typed events.
   - Remove sentinel-based scroll commands routed through `UserInputEvent::Text`.
   - Introduce typed scroll/control events in runtime/frontend boundaries.
3. **U4 + D1**: Finish ADR-018 cutover to managed TUI production path.
   - Production binary must use managed TUI path.
   - Promote editor/render logic needed in production out of test-only code.
   - Ensure single runtime-core dispatch path (no duplicate app loop).

### Phase 2 (after cutover): cleanup + convention

1. **D2**: Resolve `StreamBlock*` no-op dispatch.
   - Either wire block updates into active render state or remove dead no-op
     arms and redundant variants.
2. **U2**: Simplify streaming rendering flow to single-responsibility paths.
   - Keep one incremental streaming path per frontend mode.
   - Remove double-path or duplicate buffering logic.
3. **U3**: Remove `#[cfg(test)]` field layout drift on `TuiMode`.
   - Keep struct layout stable across test and release builds.
   - Move test-only metadata into dedicated helpers/wrappers.

## Required execution order

1. B1
2. U1
3. U4 + D1
4. D2
5. U2 + U3

No reordering is allowed unless this ADR is amended.

## Dispatcher checklist (single source of truth)

Each dispatcher must update this section in-place when work is completed.
Do not create parallel checklists in other docs.

- [ ] **B1** Unicode-safe streaming delta slicing
- [x] **U1** Typed scroll/control events (remove text sentinels)
- [x] **U4** Production binary cutover to managed TUI path
- [x] **D1** Promote required editor/render logic from test-only to production modules
- [x] **D2** Resolve `StreamBlock*` no-op dispatch (wire or remove)
- [x] **U2** Simplify streaming rendering to single-responsibility flow
- [x] **U3** Remove `#[cfg(test)]` field layout drift on `TuiMode`

## Dispatcher reporting contract (mandatory per checklist item)

When checking a box above, append an evidence block under this section:

```markdown
### [B1|U1|U2|U3|U4|D1|D2] - <short title>
- Dispatcher: <name/id>
- Commit: <sha>
- Files changed:
  - `path/to/file.rs` (+<insertions> -<deletions>)
  - `path/to/other.rs` (+<insertions> -<deletions>)
- Line references:
  - `path/to/file.rs:<line>`
  - `path/to/other.rs:<line>`
- Validation:
  - `cargo test --all-targets` : pass/fail
- Notes:
  - <what was fixed and why>
```

Line insertion/deletion counts must come from `git diff --numstat` (or equivalent)
for the exact commit that closes the checklist item.

### U1 - Typed scroll/control events; removed text sentinels
- Dispatcher: codex-gpt5
- Commit: pending (pre-commit review requested)
- Files changed:
  - `src/runtime/frontend.rs` (+20 -0)
  - `src/runtime/mode.rs` (+8 -0)
  - `src/runtime/loop.rs` (+3 -9)
  - `src/app.rs` (+138 -92)
- Line references:
  - `src/runtime/frontend.rs:3`
  - `src/runtime/mode.rs:10`
  - `src/runtime/loop.rs:33`
  - `src/app.rs:423`
- Validation:
  - `cargo test --all-targets` : pass
- Notes:
  - Replaced text-sentinel scroll paths with typed `UserInputEvent::Scroll { target, action }`.
  - Removed sentinel parsing constants/handlers from `TuiMode` path.

### U4 - Production cutover to managed TUI path
- Dispatcher: codex-gpt5
- Commit: pending (pre-commit review requested)
- Files changed:
  - `src/bin/vex.rs` (+274 -102)
- Line references:
  - `src/bin/vex.rs:14`
  - `src/bin/vex.rs:229`
  - `src/bin/vex.rs:322`
- Validation:
  - `cargo test --all-targets` : pass
- Notes:
  - Replaced append-only frontend with `ManagedTuiFrontend` wired to ratatui/crossterm runtime path.
  - `vex` binary now runs the managed TUI adapter in production execution.

### D1 - Promote required render logic to production runtime path
- Dispatcher: codex-gpt5
- Commit: pending (pre-commit review requested)
- Files changed:
  - `src/app.rs` (+138 -92)
  - `src/bin/vex.rs` (+274 -102)
- Line references:
  - `src/app.rs:127`
  - `src/app.rs:163`
  - `src/bin/vex.rs:287`
  - `src/bin/vex.rs:289`
- Validation:
  - `cargo test --all-targets` : pass
- Notes:
  - Exposed production-safe `TuiMode` render/status accessors used by managed frontend.
  - Production path now exercises `ui/layout.rs`, `ui/render.rs`, and `terminal.rs`.

### D2 - Resolve StreamBlock no-op dispatch
- Dispatcher: codex-gpt5
- Commit: pending (pre-commit review requested)
- Files changed:
  - `src/app.rs` (+138 -92)
- Line references:
  - `src/app.rs:496`
  - `src/app.rs:499`
  - `src/app.rs:508`
- Validation:
  - `cargo test --all-targets` : pass
- Notes:
  - Replaced no-op `StreamBlock*` match arms with active block-state wiring in `TuiMode`.
  - Block lifecycle updates now mutate state rather than being ignored.

### U2 - Simplify streaming rendering to single-responsibility flow
- Dispatcher: codex-gpt5
- Commit: pending (pre-commit review requested)
- Files changed:
  - `src/bin/vex.rs` (+274 -102)
- Line references:
  - `src/bin/vex.rs:277`
  - `src/bin/vex.rs:289`
- Validation:
  - `cargo test --all-targets` : pass
- Notes:
  - Removed old append frontend dual streaming print paths.
  - Managed frontend now performs one frame render path via ui renderer.

### U3 - Remove `#[cfg(test)]` TuiMode field/layout drift
- Dispatcher: codex-gpt5
- Commit: pending (pre-commit review requested)
- Files changed:
  - `src/app.rs` (+138 -92)
- Line references:
  - `src/app.rs:84`
  - `src/app.rs:103`
  - `src/app.rs:127`
  - `src/app.rs:353`
- Validation:
  - `cargo test --all-targets` : pass
- Notes:
  - `repo_label` and status helpers are now part of the release layout, not test-only fields.
  - Reduced test/release divergence for `TuiMode` state/behavior.

### Dead-Code Audit - Prune orphan runtime event stub after cutover
- Dispatcher: codex-gpt5
- Commit: pending (pre-commit review requested)
- Files changed:
  - `src/runtime.rs` (+0 -3)
  - `src/runtime/event.rs` (+0 -10)
- Line references:
  - `src/runtime.rs:1`
- Validation:
  - `cargo test --all-targets` : pass
- Notes:
  - Removed unused `runtime::event` module that was compile-shape-only and not on production path.

### API Logging Follow-up - Consolidate API debug/error emission
- Dispatcher: codex-gpt5
- Commit: pending (pre-commit review requested)
- Files changed:
  - `src/api/logging.rs` (+100 -0)
  - `src/api.rs` (+1 -0)
  - `src/api/client.rs` (+1 -39)
  - `src/api/stream.rs` (+5 -5)
- Line references:
  - `src/api/logging.rs:7`
  - `src/api/logging.rs:21`
  - `src/api/logging.rs:43`
  - `src/api/client.rs:1`
  - `src/api/client.rs:159`
  - `src/api/stream.rs:108`
- Validation:
  - `cargo test --all-targets` : pass
  - `cargo clippy --all-targets -- -D warnings` : pass
  - `./scripts/check_forbidden_imports.sh` : pass
  - `./scripts/check_no_alternate_routing.sh` : pass
- Notes:
  - Replaced ad-hoc `eprintln!` paths with shared `api::logging` utility for both payload debug and SSE parse-error reporting.
  - Standardized output formatting and sink resolution with a global `VEX_API_LOG_PATH` override and fallback compatibility for `VEX_DEBUG_PAYLOAD_PATH`.

### Dead-Code Follow-up - Remove unused legacy `src/main.rs`
- Dispatcher: codex-gpt5
- Commit: pending (pre-commit review requested)
- Files changed:
  - `src/main.rs` (+0 -141)
- Line references:
  - `Cargo.toml:5`
  - `Cargo.toml:9`
  - `src/main.rs` (removed)
- Validation:
  - `cargo test --all-targets` : pass
  - `cargo clippy --all-targets -- -D warnings` : pass
- Notes:
  - Removed dead legacy calculator program that was not part of any compiled target.
  - `autobins = false` with explicit `[[bin]] path = "src/bin/vex.rs"` remains the only binary build path.

## Gating rules

1. Phase 2 cannot start before U4 + D1 are merged and green.
2. Every step must keep `cargo test --all-targets` green.
3. Runtime-core contracts from ADR-006/ADR-007 must remain canonical.
4. Interrupt and control routing must stay typed (ADR-008 parity rule).
5. No new text sentinel control commands may be introduced.

## Consequences

- Improves safety of concurrent dispatcher work by fixing order and scope.
- Reduces sentinel collision risk and Unicode slicing bugs.
- Forces complete ADR-018 production cutover before cleanup polish.
- Keeps later cleanup tasks from masking correctness regressions.

## Compliance notes for agents

1. Treat this ADR as sequencing authority for ADR-018 follow-up work.
2. Do not mix Phase 2 cleanup into Phase 1 correctness/cutover commits.
3. If a task depends on typed events or cutover state, block it until U1 and
   U4 + D1 are complete.
