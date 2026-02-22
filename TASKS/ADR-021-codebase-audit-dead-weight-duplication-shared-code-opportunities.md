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
- **Status**: **Completed (2026-02-22)**
- **Evidence**:
  - Added runtime render scheduling controls:
    - `IDLE_RENDER_TICK` at `src/runtime/loop.rs:12`
    - `IDLE_LOOP_BACKOFF` at `src/runtime/loop.rs:13`
  - Updated `Runtime::run` to render only when first frame, state change, or
    tick due (`src/runtime/loop.rs:24`, `src/runtime/loop.rs:35`,
    `src/runtime/loop.rs:46`, `src/runtime/loop.rs:48`).
  - Added idle-regression test:
    `test_render_is_tick_or_state_driven_when_idle`
    (`src/runtime/loop.rs:242`).
- **Priority**: **P0**
- **Implementation (short)**:
  - Replaced unconditional per-iteration draw with tick/state-driven draw and
    idle backoff sleep to avoid hot idle redraw loops while preserving first
    frame + responsive updates.

### 17.a) P0.17 Delta (Insertions/Deletions)

Measured with:

```bash
git diff --numstat -- src/runtime/loop.rs
```

| File | Insertions | Deletions |
| :--- | ---: | ---: |
| `src/runtime/loop.rs` | 80 | 9 |
| **Total** | **80** | **9** |

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

### 21) “Unhandled `git` command panics” in `ToolOperator::run_git`
- **Status**: **Not accurate (current tree)**
- **Evidence**:
  - `src/tools/operator.rs::run_git` uses:
    `Command::new("git").current_dir(...).args(...).output().context(...) ?`.
  - Spawn/exec failures are returned as `anyhow::Error`; they do not panic.
- **Priority**: **N/A**

### 22) `StreamBlock::ToolCall` deltas ignored in `TuiMode::on_model_update`
- **Status**: **Partially accurate**
- **Evidence**:
  - `src/app.rs` ignores `StreamBlockDelta` payload for
    `StreamBlock::ToolCall`/`StreamBlock::ToolResult`.
  - This does not break current UX because approval/preview surfaces are fed by
    explicit tool approval/update paths, not incremental tool-call text render.
- **Risk posture**:
  - Potential future mismatch if UI begins to rely on incremental tool-call
    JSON in `active_stream_blocks`.
- **Priority**: **P2**
- **Follow-up**:
  - Either document this as intentional or implement incremental tool-call
    input accumulation in `active_stream_blocks`.

### 23) UTF-8 safety concern in `strip_incomplete_tool_tag_suffix`
- **Status**: **Not accurate (current tree)**
- **Evidence**:
  - `rfind('<')` returns the byte index of ASCII `<`, which is always a valid
    UTF-8 scalar boundary.
  - `truncate(last_open)` therefore truncates on a valid boundary.
- **Priority**: **N/A**

### 24) Startup event draining and paste-noise heuristics
- **Status**: **Partially accurate**
- **Evidence**:
  - `src/bin/vex.rs::drain_startup_events` and
    `should_ignore_startup_paste` are heuristic/best-effort filters.
  - They can trade off occasional false-positive drops against transcript-noise
    suppression.
- **Priority**: **P2**
- **Follow-up**:
  - Keep behavior, but add telemetry/debug counters or explicit opt-out env
    switch if false positives are observed in practice.

### 25) Late `StreamDelta` dropped when no active turn slot
- **Status**: **Partially accurate**
- **Evidence**:
  - `src/app.rs::on_model_update` drops `UiUpdate::StreamDelta` when
    `active_assistant_index` is `None` and `turn_in_progress` is false.
  - This is intentional stale-delta protection after cancel/complete, but can
    discard straggler data from delayed streams.
- **Priority**: **P2**
- **Follow-up**:
  - Keep current guard; consider debug-only observability for dropped stale
    deltas if field reports indicate truncation symptoms.

### 26) SSE parser buffer can grow unbounded without frame delimiter
- **Status**: **Confirmed**
- **Evidence**:
  - `src/api/stream.rs::StreamParser::process` appends chunk data into
    `self.buffer`.
  - Drain only occurs when `\n\n` frame delimiters are found (`start > 0`).
  - A malformed upstream stream with no delimiters can grow buffer
    indefinitely.
- **Priority**: **P1**
- **Follow-up**:
  - Add a bounded buffer cap (e.g., `MAX_SSE_BUFFER_CHARS`) and fail fast with
    a surfaced `UiUpdate::Error` path when exceeded.

### 27) `active_assistant_index` drift race claim during history cap
- **Status**: **Not accurate (current tree)**
- **Evidence**:
  - UI updates are processed sequentially in the runtime loop (single consumer
    of `UiUpdate` stream), not concurrently mutating `TuiMode`.
  - `enforce_history_cap` uses `checked_sub` for index rebasing.
  - Existing regression tests validate index safety under cap+stream flow.
- **Priority**: **N/A**
- **Note**:
  - Related stale-delta behavior remains tracked in item 25.

### 28) Read-only intent heuristic can produce false positives
- **Status**: **Partially accurate**
- **Evidence**:
  - `src/state/conversation/tools.rs::is_read_only_user_request` uses keyword
    heuristics over read-only/mutating hint sets.
  - Mixed-intent prompts can still be misclassified despite mutating hint
    checks.
- **Priority**: **P2**
- **Follow-up**:
  - Keep guard as default safety net; refine with stronger disambiguation or
    explicit user/model override path.

### 29) `append_incremental_suffix` overlap algorithm cost on large deltas
- **Status**: **Partially accurate**
- **Evidence**:
  - `src/state/conversation/streaming.rs::append_incremental_suffix` iterates
    char boundaries and calls `ends_with` repeatedly.
  - Correct for UTF-8 safety, but can be expensive for very large streaming
    payloads.
- **Priority**: **P3**
- **Follow-up**:
  - Keep current logic for correctness; consider optimized overlap matching if
    profiling shows this path is hot.

### 30) `RuntimeContext` `RwLock` optimization suggestion
- **Status**: **Not accurate (current tree)**
- **Evidence**:
  - `ConversationManager` work is mutation-heavy per turn; lock is held for
    send/stream orchestration by design.
  - `RwLock` does not meaningfully improve current single active-turn model.
- **Priority**: **N/A**

## Immediate Dispatch Recommendation

1. Keep P0 items 1–4 and P0.17 closed.
2. Address P1.18, P1.19, and P1.26 (input bounds + parse-error surfacing + SSE buffer cap).
3. Treat items 21, 23, 27, and 30 as closed (`not accurate`), no implementation work.
4. Keep items 22, 24, 25, and 28 as P2 design/observability follow-ups.
5. Keep item 29 as a P3 performance optimization candidate.
6. Continue P2/P3 refactors in ADR-backed, test-gated batches.

## Validation Commands

```bash
cargo check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```
