# Task CORE-14: Panic Hook Terminal Restore

**Target File:** `src/terminal/mod.rs` (hook installation), `src/app/mod.rs` (early registration call)

**ADR:** ADR-013, ADR-011 §4 (terminal lifecycle resilience), ADR-012 gate #7

**Depends on:** Nothing. This task is fully independent and MUST be dispatched before
any Phase 1 task, because raw mode is already activated by `TuiFrontend` and a panic
during Phase 1 testing would leave the terminal broken without the hook.

---

## Issue

`App::drop` calls `crate::terminal::restore()` on normal exit, but there is no panic
hook. If any code path panics while raw mode is active (including during Phase 1
development), the terminal is left in raw mode with the cursor hidden and bracketed
paste enabled, requiring `reset` or terminal restart.

ADR-011 §4 requires terminal lifecycle to be resilient on panic paths.
ADR-012 gate #7 blocks deployment until this is implemented.

---

## Decision

1. In `App::new()` (or the earliest point before raw mode is enabled in `main.rs`),
   install a panic hook:
   ```rust
   let prev_hook = std::panic::take_hook();
   std::panic::set_hook(Box::new(move |info| {
       // Best-effort terminal restore — ignore errors; the process is dying.
       let _ = crate::terminal::restore();
       prev_hook(info);
   }));
   ```
   This chains with the default hook so panic messages still print to stderr.

2. Guard against double-registration: if `App::new` can be called more than once
   in the same process (e.g. tests), the hook must not stack. Use a
   `std::sync::Once` gate:
   ```rust
   static PANIC_HOOK_INSTALLED: std::sync::Once = std::sync::Once::new();

   PANIC_HOOK_INSTALLED.call_once(|| {
       let prev_hook = std::panic::take_hook();
       std::panic::set_hook(Box::new(move |info| {
           let _ = crate::terminal::restore();
           prev_hook(info);
       }));
   });
   ```

3. `crate::terminal::restore()` must already exist (it is called by `App::drop`).
   Do not create a second restore path — call the same function.

4. The hook MUST be installed before the first call to anything that enables raw
   mode, bracketed paste, or hides the cursor.

---

## Definition of Done

1. Panic hook is installed in `App::new` (or equivalent) via `std::sync::Once`.
2. Hook calls `crate::terminal::restore()` before invoking the previous hook.
3. Multiple `App::new` calls in the same process do not stack hooks.
4. `cargo test --all-targets` remains green (hook does not interfere with test
   teardown — `restore()` is idempotent or a no-op when not in raw mode).

---

## Anchor Verification

`test_terminal_restored_after_simulated_panic`

Because causing a real panic in a test is fragile, the anchor uses the `Once`
completion state as a proxy:

```rust
#[test]
fn test_terminal_restored_after_simulated_panic() {
    // This test verifies the hook is installed without actually leaving a broken
    // terminal. It asserts that PANIC_HOOK_INSTALLED has been set after App::new.
    // The integration-level check (terminal is actually restored) is a manual
    // verification step: run the TUI, trigger a panic, and confirm the terminal
    // is usable afterward.

    // Programmatic assertion:
    assert!(
        PANIC_HOOK_INSTALLED.is_completed(),
        "panic hook must be installed after App::new"
    );
}
```

**What NOT to do:**
- Do not replace `App::drop` terminal restore — it handles normal exit. The hook
  handles panic paths. Both are needed.
- Do not use `std::process::exit` in the hook — let the default hook print the
  panic message first.
- Do not add any state that would cause the hook to be skipped in release builds.
- Do not touch `src/state/`, `src/api/`, or `src/tools/`.
