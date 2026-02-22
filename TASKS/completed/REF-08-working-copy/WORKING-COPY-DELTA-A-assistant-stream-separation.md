# REF-08 DELTA-A: Assistant Stream Separation

**Status:** Completed and verified (2026-02-19)
**ADR mapping:** ADR-008 Decision #1; ADR-012 gate #1

## Problem

`UiUpdate::StreamDelta` could append to the most recent history line, including
the user prompt line (`> ...`), which merges user/assistant text.

## Target

`src/app/mod.rs`

## Decision

1. Track `active_assistant_index: Option<usize>` in `TuiMode`.
2. On accepted user submit:
   1. Push user line `> {input}`.
   2. Push an empty assistant placeholder.
   3. Set `active_assistant_index` to the placeholder index.
3. On `UiUpdate::StreamDelta(text)`:
   1. Append only to the active assistant index.
   2. If none exists, create one and set the index.
4. On `TurnComplete`, `Error`, and interrupt-cancel:
   1. Clear `active_assistant_index`.
5. Keep approval behavior and status rendering unchanged.

## Required test

`test_ref_08_stream_delta_appends_to_assistant_placeholder_not_user_line`

## Acceptance

No `StreamDelta` update may mutate a user-prefixed (`> `) line.
