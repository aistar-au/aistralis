# Task FEAT-15: Scrollback Viewport

**Target File:** `src/app/mod.rs`

**ADR:** ADR-013, ADR-010 (viewport and transcript model), ADR-012 gate #3

**Depends on:** CORE-09 (`ui_state_slices_compile` must be green)
**Parallel-safe with:** CORE-07 and CORE-08 chains (history-state work in `src/app/mod.rs`)

---

## Issue

`render_messages` is called with a hard-coded `scroll_offset: 0`. There is no
scrollback state in `TuiMode` (or `HistoryState` after CORE-09). PageUp/PageDown
inputs are not handled. Auto-follow is not modeled.

ADR-010 §2 and §3 require: viewport supports `PageUp`, `PageDown`, `Home`, `End`,
and auto-follow; new output while scrolled up MUST NOT force-scroll to bottom.
ADR-012 gate #3 blocks deployment until this is implemented.

---

## Decision

1. Add to `HistoryState` (post-CORE-09) or directly to `TuiMode` (pre-CORE-09):
   ```rust
   scroll_offset: usize,
   auto_follow: bool,  // default: true
   ```
2. In the render call path, pass live `scroll_offset` state to existing
   `render_messages(...)`. Never call with hard-coded `0`.
3. In `TuiFrontend::poll_user_input` (or the key dispatch path), handle:
   - `PageUp` → decrement `scroll_offset` by viewport height; set `auto_follow = false`
   - `PageDown` → increment `scroll_offset` (clamped to max); if at bottom, set `auto_follow = true`
   - `Home` (Ctrl+Home) → `scroll_offset = 0`; `auto_follow = false`
   - `End` (Ctrl+End) → `scroll_offset = max`; `auto_follow = true`
4. In `on_model_update` for `StreamDelta`, `TurnComplete`:
   if `auto_follow` is true, recalculate `scroll_offset = max_scroll(history.len(), viewport_height)`
5. Do not alter pane geometry or any overlay behavior.

---

## Definition of Done

1. The `render_messages(...)` call site uses live `scroll_offset` state; no call
   site passes hard-coded `0`.
2. PageUp/PageDown/Home/End key events update scroll state correctly.
3. Auto-follow advances the viewport on new output when enabled.
4. A scrolled-up viewport is not force-scrolled on new `StreamDelta` when
   `auto_follow` is false.

---

## Anchor Verification

`test_scrollback_retains_position_during_streaming`

The test drives `on_model_update(StreamDelta(...))` while `auto_follow = false`
and asserts that `scroll_offset` is unchanged after the update.

```rust
#[test]
fn test_scrollback_retains_position_during_streaming() {
    // Set auto_follow = false, scroll_offset = 5
    // Send StreamDelta
    // Assert scroll_offset == 5 (not reset to bottom)
}
```

**What NOT to do:**
- Do not move render logic that belongs to CORE-08 (frame composition order).
- Do not modify `src/ui/render.rs` for this task.
- Do not add new `UiUpdate` variants.
- Do not touch `src/state/`, `src/api/`, or `src/tools/`.
- Do not add CLI flags or env vars.
