# Task CORE-12: Bounded Transcript

**Target File:** `src/app/mod.rs`

**ADR:** ADR-013, ADR-010 §4 (transcript retention), ADR-012 gate #5

**Depends on:** CORE-09 (`ui_state_slices_compile` must be green)
**Parallel-safe with:** CORE-07 chain (target file changes are isolated to HistoryState)

---

## Issue

`TuiMode::history` (or `HistoryState::messages` after CORE-09) is a plain `Vec<String>`
with no upper bound. A long session accumulates entries indefinitely, causing unbounded
memory growth.

ADR-010 §4 requires transcript retention to be bounded. ADR-012 gate #5 blocks
deployment until this is implemented.

---

## Decision

1. Add a constant (or configurable value):
   ```rust
   const MAX_HISTORY_LINES: usize = 2000;
   ```
   Configurable via `AISTAR_MAX_HISTORY_LINES` env var (parsed with `parse_bool_flag`
   pattern; default 2000 if absent or unparseable).

2. After every push to `history.messages` (in `on_user_input` and `on_model_update`),
   enforce the cap:
   ```rust
   fn enforce_history_cap(messages: &mut Vec<String>, cap: usize) {
       if messages.len() > cap {
           let excess = messages.len() - cap;
           messages.drain(..excess);
           // Adjust active_assistant_index if it shifts
       }
   }
   ```

3. If `active_assistant_msg` (the index into the vec for the current in-flight turn)
   would be invalidated by the drain, recalculate it:
   ```rust
   if let Some(idx) = self.current_assistant_msg {
       self.current_assistant_msg = idx.checked_sub(excess);
   }
   ```

4. Call `enforce_history_cap` at the end of `on_user_input` and after each push in
   `on_model_update`. Do not call it mid-stream-delta (only on message boundary) to
   avoid off-by-one with `active_assistant_msg`.

---

## Definition of Done

1. `history.messages.len()` never exceeds `MAX_HISTORY_LINES` at the end of any
   `on_model_update` or `on_user_input` call.
2. `active_assistant_msg` index remains valid after a drain event.
3. `AISTAR_MAX_HISTORY_LINES` env var overrides the default cap.
4. Existing tests (`test_ref_08_stream_delta_appends_to_assistant_placeholder_not_user_line`
   and related) remain green.

---

## Anchor Verification

`test_transcript_does_not_exceed_cap_after_n_turns`

```rust
#[test]
fn test_transcript_does_not_exceed_cap_after_n_turns() {
    // Override cap to a small value (e.g. 10 lines) via AISTAR_MAX_HISTORY_LINES
    // Drive N > cap/2 turns of user input + StreamDelta + TurnComplete
    // Assert history.messages.len() <= cap at all times
}
```

**What NOT to do:**
- Do not change `ConversationManager` message history — this is UI-layer history only.
- Do not add new `UiUpdate` variants.
- Do not touch `src/state/`, `src/api/`, or `src/tools/`.
- Do not alter overlay state or input routing.
