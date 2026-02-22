# ADR-020: Looping Architecture and Enriched Tool Response Correctness

**Date:** 2026-02-22
**Status:** Proposed
**Deciders:** Core maintainer
**Related tasks:** L1, L2, L3, L4, L5, L6
**ADR chain:** ADR-018, ADR-019

## Context

Post-cutover runtime behavior still had six correctness gaps in `send_message`
tool rounds and streaming block handling:

1. tool execution failures were surfaced as `Complete` in block status,
2. multi-tool rounds could early-return and skip remaining tool results,
3. incremental suffix dedupe could drop valid repeated short text,
4. history anchor preservation could permanently block pruning when anchor drifted to index 0,
5. padded block indices were inserted without emitting matching block-start events,
6. a test-only `execute_tool` path was dead and diverged from timeout behavior.

These issues collectively caused retry churn, protocol incompleteness across
multi-tool rounds, and fragile frontend state alignment.

## Decision

Apply a single correctness sweep on conversation loop/tool handling with
explicit regression tests.

### L1 - Tool status correctness on execution failures

- Introduce `ToolStatus::Error`.
- Set tool-call final status to `Error` whenever tool execution returns `Err`.

### L2 - Multi-tool round completeness

- Remove early `return` from missing-location and denied-approval branches.
- Emit error tool results for those branches and continue processing remaining
  tool calls in the same round before the next model request.

### L3 - Incremental suffix dedupe safety

- Remove unconditional `ends_with` fast-drop behavior.
- Keep overlap-based dedupe but treat short trailing full-overlap suffixes as
  new content to avoid silent drops.

### L4 - Anchor-aware history pruning without unbounded growth

- Change anchor preservation to a soft preference (small distance window) rather
  than an absolute floor that can freeze pruning.

### L5 - Block padding event parity

- When padding block indices in `upsert_turn_block`, emit `BlockStart` for each
  placeholder index so frontend index maps stay aligned.

### L6 - Remove dead test-only tool execution path

- Delete unused `#[cfg(test)] execute_tool`.
- Route tests through `execute_tool_with_timeout` to keep behavior aligned with
  production dispatch.

## Dispatcher checklist

- [x] **L1** Tool execution errors emit `ToolStatus::Error`
- [x] **L2** Multi-tool rounds collect all tool results before next API round
- [x] **L3** Incremental suffix dedupe does not drop short trailing repeats
- [x] **L4** History pruning remains bounded when anchor is far behind
- [x] **L5** Padded block indices emit corresponding `BlockStart`
- [x] **L6** Remove dead test-only tool execution path

## Evidence

### L1-L6 - Loop/enriched response correctness sweep
- Dispatcher: codex-gpt5
- Commit: pending (pre-commit review requested)
- Files changed:
  - `src/state/conversation.rs` (+367 -56)
  - `src/state/stream_block.rs` (+1 -0)
- Line references:
  - `src/state/conversation.rs:528`
  - `src/state/conversation.rs:592`
  - `src/state/conversation.rs:632`
  - `src/state/conversation.rs:805`
  - `src/state/conversation.rs:917`
  - `src/state/conversation.rs:1297`
  - `src/state/conversation.rs:2626`
  - `src/state/conversation.rs:2674`
  - `src/state/conversation.rs:3330`
  - `src/state/conversation.rs:3386`
  - `src/state/stream_block.rs:27`
- Validation:
  - `cargo test --all-targets` : pass
  - `cargo clippy --all-targets -- -D warnings` : pass
- Notes:
  - Tool-loop control flow now completes round-local tool result protocol even
    when one tool is denied or has invalid location data.
  - Error-state tool lifecycle is explicit and stream-visible, instead of
    presenting failed tools as completed.
  - History pruning remains bounded in long tool-loop sessions.
  - Regression tests were added for each bug to prevent reintroduction.
