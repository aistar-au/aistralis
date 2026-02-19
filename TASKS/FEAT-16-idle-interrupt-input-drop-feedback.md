# Task FEAT-16: Idle Interrupt Exit and Input-Drop Feedback

**Target File:** `src/app/mod.rs`

**ADR:** ADR-013, ADR-009 §1–§2 (interaction contract), ADR-012 gates #1 and #2

**Depends on:** CORE-10 (`overlay_blocks_submit` must be green — interrupt routing
and input guard share the same `TuiMode` state machine and must not conflict)

---

## Issue

Two violations of ADR-009 and ADR-012 remain after REF-08:

**Issue 1 — Silent input drop (gate #1):**
In `TuiMode::on_user_input`, when `turn_in_progress` is true, the method returns
without feedback. The user types a message and it disappears silently. ADR-009 §1
forbids silent drop.

**Issue 2 — Idle interrupt is a no-op (gate #2):**
In `TuiMode::on_interrupt`, only active turns are cancelled:
```rust
if self.turn_in_progress { ctx.cancel_turn(); ... }
```
When `turn_in_progress` is false, `on_interrupt` does nothing. The user presses
`Ctrl+C` at the idle prompt and sees no response. ADR-009 §2 requires idle `Ctrl+C`
to trigger defined exit behavior.

---

## Decision

### Fix 1 — Input-drop feedback

In `TuiMode::on_user_input`, replace the silent early return with visible rejection:

```rust
if self.turn_in_progress {
    // ADR-012 gate #1: no silent drop. Reject with visible feedback.
    self.history.messages.push(
        "[busy — turn in progress, input discarded]".to_string()
    );
    return;
}
```

If preserving the input for re-submit is preferred over discarding it, the buffer
may be stashed in `InputState` and restored to the editor. The manifest permits
either approach; the anchor test accepts either — but the behavior must be
user-visible.

### Fix 2 — Idle interrupt double-press exit

Add `pending_quit: bool` to `TuiMode` (or `InputState` after CORE-09).
Initialize to `false`.

In `TuiMode::on_interrupt`:

```rust
pub fn on_interrupt(&mut self, ctx: &mut RuntimeContext) {
    if self.turn_in_progress {
        // Existing behavior: cancel active turn
        ctx.cancel_turn();
        self.history.messages.push("[turn cancelled]".to_string());
        // Clear the pending_quit flag — a successful cancel resets exit intent
        self.pending_quit = false;
        return;
    }

    // Idle path
    if self.pending_quit {
        // Second Ctrl+C: exit
        ctx.request_quit();   // or set a quit flag the loop checks
    } else {
        self.pending_quit = true;
        self.history.messages.push(
            "[press Ctrl+C again to exit]".to_string()
        );
    }
}
```

`ctx.request_quit()` signals the `Runtime::run` loop to set `should_quit = true`
on the `FrontendAdapter`. If `RuntimeContext` does not yet have a quit signal,
add a boolean field `quit_requested: bool` and expose
`pub fn request_quit(&mut self) { self.quit_requested = true; }`.
The `TuiFrontend::should_quit` implementation reads this flag.

`pending_quit` MUST be reset to `false` when a new turn starts (in `on_user_input`
after the guard passes), so a brief idle `Ctrl+C` during a session does not linger.

---

## Definition of Done

1. A submitted input while `turn_in_progress` is true produces a visible history
   line — the input is never silently discarded.
2. First idle `Ctrl+C` pushes `"[press Ctrl+C again to exit]"` and sets
   `pending_quit = true`.
3. Second idle `Ctrl+C` triggers exit (`should_quit()` returns `true` on next loop
   tick).
4. `Ctrl+C` during an active turn still cancels the turn and resets `pending_quit`.
5. `pending_quit` resets to `false` when a new turn is accepted.
6. No new `UiUpdate` variants are added.
7. `test_ref_03_tui_mode_overlay_blocks_input` remains green (overlay guard is not
   disturbed).

---

## Anchor Verification

Two anchor tests, both in `src/app/mod.rs` `#[cfg(test)]` block:

```rust
#[test]
fn test_idle_interrupt_shows_feedback() {
    // Build TuiMode with turn_in_progress = false
    // Call on_interrupt once
    // Assert pending_quit == true
    // Assert history contains "[press Ctrl+C again to exit]"
    // Call on_interrupt again
    // Assert should_quit is signalled (ctx.quit_requested == true or equivalent)
}

#[test]
fn test_input_drop_shows_feedback() {
    // Build TuiMode with turn_in_progress = true
    // Call on_user_input("hello")
    // Assert turn_in_progress is still true (no new turn started)
    // Assert history contains "[busy" (prefix match)
    // Assert "hello" did not become a user-line in history
}
```

**What NOT to do:**
- Do not use a sentinel string for interrupt — `UserInputEvent::Interrupt` is already
  typed (REF-08 DELTA-E). This task only affects `on_interrupt` behavior.
- Do not alter CORE-10's overlay key routing.
- Do not add CLI flags or new env vars.
- Do not touch `src/state/`, `src/api/`, or `src/tools/`.
