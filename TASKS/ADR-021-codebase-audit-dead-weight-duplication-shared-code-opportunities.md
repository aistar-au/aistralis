# ADR-021: Codebase Audit — Dead Weight, Duplication, and Shared-Code Opportunities

- **Status**: Proposed
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
- **Status**: **Confirmed**
- **Evidence**:
  - Test-only editor stack under `#[cfg(test)]` in `src/app.rs`:
    `InputEditor`, `InputState`, `EditorSnapshot`, `InputAction`,
    `RenderGuard`, `RenderPass`, `overlay_event_to_user_input`,
    `input_rows_for_buffer`, `MAX_INPUT_PANE_ROWS`.
  - Production path uses `ManagedTuiFrontend` in `src/bin/vex.rs`.

### 2) `submit_input()` trims leading whitespace
- **Status**: **Confirmed**
- **Evidence**:
  - `src/bin/vex.rs:174` uses `self.input_buffer.trim().to_string()`.
  - `src/app.rs:731` (`InputEditor::submit`) also uses `.trim().to_string()`.

### 3) Scroll metrics mismatch with wrapped rendering
- **Status**: **Confirmed**
- **Evidence**:
  - `src/ui/render.rs:77` wraps rows in `render_messages`.
  - `src/ui/render.rs:109` `history_visual_line_count` counts only embedded `\n`.
  - `src/app.rs:291` uses `history_visual_line_count` for max scroll math.

### 4) UTF-8 cursor logic duplicated across test/prod editors
- **Status**: **Confirmed**
- **Evidence**:
  - Similar boundary/edit methods in `src/app.rs` test-only `InputEditor`
    and `src/bin/vex.rs` `ManagedTuiFrontend`.

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

## Immediate Dispatch Recommendation

1. Ship P0 items 1–4 first (behavior correctness).
2. Triage P1 claims 5–8 with corrected scope (5–7 are not current bugs).
3. Batch P2 dedup items in small PRs with regression tests.
4. Track P3 as refactor ADRs with explicit acceptance criteria.

## Validation Commands

```bash
cargo check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```
