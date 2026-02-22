# ADR-021: Codebase Audit — Dead Weight, Duplication, and Shared-Code Opportunities

- **Status**: Accepted (P0 items 1–4 implemented)
- **Date**: 2026-02-22
- **Context**: Consolidated static review of the current `main` codebase.
- **Goal**: Reduce maintenance drag, align production behavior with tested behavior, and remove duplicated control flow.

## Accuracy Review of Submitted Findings

This ADR records the submitted audit with verification against the current tree.
Items are marked as:

- **Confirmed**: validated directly in code.
- **Partially accurate**: valid concern but scope/details need correction.
- **Not accurate (current tree)**: claim does not hold for current `main`.
- **Pending deep audit**: plausible, but not fully validated in this pass.

## P0 — Fix Now

### 1) `InputEditor` test-only coverage vs production behavior
- **Status**: **Completed (2026-02-22)**
- **Evidence**:
  - Added shared production editor module: `src/ui/editor.rs`.
  - Exported editor module from `src/ui.rs`.
  - `src/bin/vex.rs` now owns `InputEditor` and delegates editing/submit actions through `InputAction`.
  - Removed test-only editor implementation duplication from `src/app.rs`.

### 2) `submit_input()` trims leading whitespace
- **Status**: **Completed (2026-02-22)**
- **Evidence**:
  - Shared submit path now uses:
    - `trim_end_matches('\n')`
    - `trim_end_matches('\r')`
  - Leading whitespace is preserved for submitted prompts.

### 3) Scroll metrics mismatch with wrapped rendering
- **Status**: **Completed (2026-02-22)**
- **Evidence**:
  - `src/ui/render.rs` now computes visual line count with wrapping:
    `history_visual_line_count(messages, content_width)`.
  - Added shared width helper: `history_content_width_for_area(messages, area)`.
  - `src/app.rs` uses width-aware count for `status_line()` and `max_scroll_offset()`.
  - `src/bin/vex.rs` updates `TuiMode` history content width each frame before render.

### 4) UTF-8 cursor logic duplicated across test/prod editors
- **Status**: **Completed (2026-02-22)**
- **Evidence**:
  - Consolidated UTF-8 cursor and edit operations into `src/ui/editor.rs`.
  - Removed duplicate cursor/edit implementations from `src/bin/vex.rs`.
  - `src/app.rs` tests now import and exercise the shared editor module.

## P0 Implementation Delta (Insertions/Deletions)

Measured with:

```bash
git add -N src/ui/editor.rs
git diff --numstat -- src/ui/editor.rs src/app.rs src/bin/vex.rs src/ui.rs src/ui/render.rs
```

| File | Insertions | Deletions |
| :--- | ---: | ---: |
| `src/ui/editor.rs` | 274 | 0 |
| `src/app.rs` | 14 | 258 |
| `src/bin/vex.rs` | 45 | 139 |
| `src/ui.rs` | 1 | 0 |
| `src/ui/render.rs` | 25 | 8 |
| **Total** | **359** | **405** |

## P1 — Dead Weight / Cleanup Claims

### 5) `execute_tool_blocking_with_operator` dead wrapper
- **Status**: **Not accurate (current tree)**
- **Correction**:
  - Non-test wrapper is used by production execution path from
    `execute_tool_with_timeout` in `src/state/conversation/tools.rs`.

### 6) `looks_like_terminal_transcript` family likely bypassed
- **Status**: **Not accurate (current tree)**
- **Correction**:
  - Functions are live and used in production path:
    `src/bin/vex.rs:101` and `src/bin/vex.rs:113`.

### 7) Empty `on_model_update` in `runtime/loop.rs` as production dead weight
- **Status**: **Not accurate (current tree)**
- **Correction**:
  - Empty implementation is in test-only `InterruptMode` under `#[cfg(test)]`
    in `src/runtime/loop.rs`.

### 8) Post-cutover comment debt
- **Status**: **Partially accurate**
- **Note**:
  - There are transition-era comments; triage should separate stale comments
    from still-useful rationale.

## P2 — Live Duplication Claims

### 9) Tool error dispatch block repeated in conversation core
- **Status**: **Confirmed**
- **Note**:
  - Multiple branches in `src/state/conversation/core.rs` repeat similar
    emit/format/truncate/push patterns.

### 10) Scroll handling duplication in app state
- **Status**: **Confirmed**
- **Evidence**:
  - Repeated line/page/home/end patterns in `src/app.rs`.

### 11) Approval input parsing duplicated
- **Status**: **Confirmed**
- **Evidence**:
  - `handle_approval_input` and `handle_patch_overlay_input` in `src/app.rs`.

### 12) Diff row styling logic duplicated
- **Status**: **Confirmed**
- **Evidence**:
  - `history_row_style` and `styled_diff_line` in `src/ui/render.rs`.

### 13) `required_tool_string*` variants are mostly overlapping
- **Status**: **Confirmed**
- **Evidence**:
  - Related helpers in `src/state/conversation/tools.rs`.

### 14) Auto-follow reconciliation repeated outside shared helper
- **Status**: **Confirmed**
- **Evidence**:
  - `on_model_update` branches in `src/app.rs` repeat follow/clamp behavior.

### 15) `MAX_INPUT_PANE_ROWS` not applied in production path
- **Status**: **Confirmed**
- **Evidence**:
  - Constant + clamp helper are test-only in `src/app.rs`.
  - Production render in `src/bin/vex.rs` computes input rows without that cap.

## P3 — Architectural Opportunities

The following are design proposals and were not evaluated as strict true/false
bugs in this pass:

- Decompose `send_message` in `src/state/conversation/core.rs`.
- Promote editor into production module (e.g., `src/ui/editor.rs`).
- Unify scroll behavior behind shared abstraction.
- Introduce shared approval parser helper.
- Centralize tool metadata (`ToolKind`/registry approach).
- Separate structured/text protocol output strategies.
- Increase use of `StatefulWidget`-style encapsulation for UI state.
- Split large app module for navigation ergonomics.
- Expand `src/util.rs` for repeated parsing/truncation helpers.
- Add stronger `src/test_support.rs` harness helpers.
- Move prompt/schema blobs out of `src/api/client.rs` where practical.

## External Audit Follow-up (2026-02-22)

This section triages the externally submitted debugging report against the
current tree.

### 16) “Runtime not wired after REF-08” in `src/bin/vex.rs`
- **Status**: **Not accurate (current tree)**
- **Evidence**:
  - `src/bin/vex.rs` uses `#[tokio::main]`.
  - `main` constructs runtime/context via `build_runtime(config)?`.
  - `runtime.run(&mut frontend, &mut ctx).await` is executed.
- **Note**:
  - This was a historical issue reflected in older review text, now resolved.

### 17) Unconditional redraw loop / hot idle rendering
- **Status**: **Confirmed**
- **Evidence**:
  - `src/runtime/loop.rs` calls `frontend.render(&self.mode)` every iteration.
  - `src/bin/vex.rs` polls input at fixed cadence (`event::poll(16ms)`), so
    render is still called repeatedly even when state is unchanged.
- **Priority**: **P0**
- **Follow-up**:
  - Implement dirty/tick-aware render guard in runtime/frontend path.

### 18) Unbounded input buffer in production editor
- **Status**: **Confirmed**
- **Evidence**:
  - `src/ui/editor.rs::insert_str` appends without size cap.
  - Large paste input can grow buffer unbounded.
- **Priority**: **P1**
- **Follow-up**:
  - Add max input length cap (configurable/default bounded).

### 19) SSE parse failures are logged but not surfaced to UI
- **Status**: **Confirmed**
- **Evidence**:
  - `src/api/stream.rs` logs parse failures via `emit_sse_parse_error(...)`.
  - No parse-error event is emitted into `ConversationStreamUpdate`/`UiUpdate`;
    UI may only observe a stalled turn.
- **Priority**: **P1**
- **Follow-up**:
  - Add explicit parse-error propagation path to `UiUpdate::Error`.

### 20) `edit_file` race condition (read-modify-write window)
- **Status**: **Partially accurate**
- **Evidence**:
  - `src/tools/operator.rs::edit_file` performs read/validate/write sequence.
  - A concurrent external writer can race between read and write.
- **Risk posture**:
  - Acceptable for current single-user local-agent target, but still a known
    TOCTOU class risk.
- **Priority**: **P2**
- **Follow-up**:
  - Evaluate optional lock/atomic-write strategy if multi-writer scenarios are
    in scope.

## Immediate Dispatch Recommendation

1. Keep P0 items 1–4 closed and add P0.17 (dirty/tick-aware render scheduling).
2. Address P1.18 and P1.19 (input bounds + parse-error UI surfacing).
3. Continue P2 dedup/cleanup items in small PRs with regression tests.
4. Track P3 refactors as separate ADR-backed batches with explicit gates.

## Validation Commands

```bash
cargo check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```
