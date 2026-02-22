# REF-08 DELTA-B: Filter BlockDelta -> StreamDelta mirroring

**Status:** Completed and verified (2026-02-19)
**ADR mapping:** ADR-008 Decision #2; ADR-012 gate #4 (overlay/tool-stream correctness)

## Problem

`ConversationStreamUpdate::BlockDelta` mirrored unconditionally to
`UiUpdate::StreamDelta`, leaking tool `partial_json` into user-visible stream
text.

## Target

`src/runtime/context.rs`

## Decision

1. Track per-block class by `index` from `BlockStart`.
2. Classify:
   1. Textual: `StreamBlock::Thinking`, `StreamBlock::FinalText`.
   2. Non-textual: `StreamBlock::ToolCall`, `StreamBlock::ToolResult`.
3. On `BlockDelta`:
   1. Always emit `UiUpdate::StreamBlockDelta`.
   2. Emit `UiUpdate::StreamDelta` only for textual blocks.
4. On `BlockComplete`, clear tracked class for the index.
5. If `BlockStart` is `FinalText { content }` and content is non-empty, emit
   one `StreamDelta` for parity with text-only consumers.

## Required test

`test_ref_08_block_delta_partial_json_not_mirrored_to_stream_delta`

## Acceptance

Tool-input JSON must never appear in `UiUpdate::StreamDelta`.
