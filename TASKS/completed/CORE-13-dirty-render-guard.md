# Task CORE-13: Dirty-State Render Guard

**Target File:** `src/app/mod.rs` (TuiFrontend or App render path)

**ADR:** ADR-013, ADR-011 §1–§2 (render loop efficiency), ADR-012 gate #6

**Depends on:** CORE-08 (`overlay_renders_after_base_panes` must be green — render
path must be stable before adding a guard to it)
**Can run in parallel with:** CORE-10

---

## Issue

`TuiFrontend::render` (or the equivalent draw call) is invoked on every loop iteration
via `poll(Duration::from_millis(16))` unconditionally, regardless of whether any state
changed. This causes hot idle redraws.

ADR-011 §1–§2 require render to be event-driven with a controlled tick policy.
ADR-012 gate #6 blocks deployment until unconditional idle redraws are eliminated.

---

## Decision

1. Add `dirty: bool` to `TuiFrontend` (or the struct that owns the terminal draw call).
   Initialize to `true` so the first frame is always drawn.

2. Set `dirty = true` when:
   - `on_model_update` receives any `UiUpdate` (state has changed)
   - `on_user_input` or `on_interrupt` modifies any visible state
   - Overlay state changes (open, close, keypress feedback)
   - The scroll offset changes

3. Define two tick intervals:
   ```rust
   const CURSOR_TICK_MS: u64 = 500;   // cursor blink
   const STATUS_TICK_MS: u64  = 120;  // status bar / spinner
   ```
   Read from `VEX_CURSOR_TICK_MS` and `VEX_STATUS_TICK_MS` env vars with
   safe defaults. Both are bounded: cursor 100–2000ms, status 50–500ms.

4. In the render loop:
   ```rust
   if self.dirty || last_tick.elapsed() >= tick_interval {
       terminal.draw(|frame| { /* draw */ })?;
       self.dirty = false;
       last_tick = Instant::now();
   }
   ```

5. The `poll` timeout MUST be `min(CURSOR_TICK_MS, STATUS_TICK_MS)` — not 16ms —
   so the thread does not spin at 60fps when nothing is happening.

---

## Definition of Done

1. `terminal.draw(...)` is not called when `dirty` is false and the tick interval
   has not elapsed.
2. Every observable state change (model update, user input, overlay toggle) sets
   `dirty = true` before the next render check.
3. Poll timeout is derived from tick intervals, not hard-coded to 16ms.
4. Prior render-order anchors from CORE-08 remain green.

---

## Anchor Verification

`test_render_not_called_when_state_unchanged`

Because `terminal.draw` requires a real terminal, the anchor test uses a render
counter on a headless stub that records whether `render` was invoked:

```rust
#[test]
fn test_render_not_called_when_state_unchanged() {
    // Build a HeadlessFrontend with a render_count field
    // Run two loop ticks with no input and no UiUpdate
    // Assert render_count has not incremented between tick 1 and tick 2
    // Then send a UiUpdate, run one more tick
    // Assert render_count incremented exactly once
}
```

**What NOT to do:**
- Do not add a `dirty` flag to `TuiMode` — it belongs on the frontend, not the mode.
- Do not change `RuntimeMode` trait methods.
- Do not alter CORE-08 draw order.
- Do not touch `src/state/`, `src/api/`, or `src/tools/`.
