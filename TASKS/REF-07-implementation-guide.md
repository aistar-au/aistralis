# REF-07 Implementation Guide
## For use in a fresh chat with the codebase mounted

---

## IMPORTANT — Read Before Doing Anything

This prompt will be sent as the **first message** in a new chat.

**The second message will contain the full GitHub repository codebase.**

**Do not begin any steps below until the codebase has been provided in the second message.** Do not assume file contents, do not write any code, do not run any commands. Just acknowledge this message and wait.

Once the codebase arrives, start at Step 1.

---

## What You Are Implementing

**REF-07 — Runtime Execution Contract (Async + Guard)**

Two guarantees:
1. `Runtime::run()` is `async fn` — compile-time enforcement that call sites must `.await` it
2. `RuntimeContext::start_turn()` gets a runtime guard — runtime safety for direct calls, emits `UiUpdate::Error` instead of panicking, and leaves history clean

Two files to touch, nothing else:
- `src/runtime/loop.rs`
- `src/runtime/context.rs`

---

## Step 1 — Orient Yourself

Run these to understand the project layout before touching anything:

```bash
# Top-level structure
find . -name "*.rs" | grep -v target | sort

# The two files you'll be editing
cat src/runtime/loop.rs
cat src/runtime/context.rs

# Supporting types you'll reference
cat src/runtime/update.rs
cat src/runtime/frontend.rs
cat src/runtime/mode.rs
cat src/runtime/mod.rs
```

---

## Step 2 — Verify the Current State of the Codebase

Check that the upstream dependencies (REF-04 Track A and REF-05) are actually present before proceeding. REF-07 cannot be implemented without them.

**REF-04 Track A — `start_turn` must exist:**
```bash
grep -n "fn start_turn" src/runtime/context.rs
# Must return a matching definition line. If missing, stop — REF-04 Track A is not merged.
```

**REF-04 Track A — `cancel_turn` must exist:**
```bash
grep -n "fn cancel_turn" src/runtime/context.rs
# Must return a matching definition line. If missing, stop — REF-04 Track A is not merged.
```

**REF-05 — `HeadlessFrontend` or the existing loop test must exist:**
```bash
grep -n "HeadlessFrontend\|test_ref_05" src/runtime/loop.rs
# Must return results. If missing, stop — REF-05 is not merged.
```

**Check `run()` signature state:**
```bash
grep -n "pub fn run\|pub async fn run" src/runtime/loop.rs
# REF-07 merged state: should show pub async fn run.
# If sync pub fn run appears, Step 5 still needs to be applied.
```

**Check `start_turn` runtime guard state:**
```bash
grep -n "try_current\|Handle" src/runtime/context.rs
# REF-07 merged state: should show Handle::try_current() usage.
# If no match, Step 5 still needs to be applied.
```

**Check `UiUpdate::Error` variant exists (you'll need it for the guard):**
```bash
grep -n "Error" src/runtime/update.rs
# Must show: Error(String) variant
```

**Confirm `TuiMode::new()` exists (used in tests):**
```bash
grep -rn "pub fn new" src/app/ | grep -i "tuimode\|TuiMode\|impl TuiMode"
```

---

## Step 3 — Check Call Sites for `run()`

Before making `run()` async, find every place it's called so you know what else needs `.await` added:

```bash
grep -rn "\.run(" src/ | grep -v target | grep -v "#\[test\]\|test_ref"
grep -rn "\.run(" src/ | grep -v target
```

Every call site that isn't already inside an `async` context will need fixing. Common locations: `src/main.rs`, integration test harnesses, any `block_on` wrapper.

```bash
# Also check main.rs specifically
cat src/main.rs
```

If `run()` is called with `runtime.run(...)` (no `.await`), you'll add `.await`. If it's called inside `std::thread::spawn` or a sync context, it needs `tokio::runtime::Runtime::block_on` instead.

---

## Step 4 — Check the Test Module in loop.rs

Understand the existing test before adding to it:

```bash
grep -n "#\[tokio::test\]\|async fn test_\|fn test_" src/runtime/loop.rs
```

The existing `test_ref_05_headless_loop_terminates` test should be `#[tokio::test] async fn` and should await `runtime.run(...)`. Confirm:

```bash
grep -n "runtime.run" src/runtime/loop.rs
# REF-07 merged state: should show runtime.run(&mut frontend, &mut ctx).await;
```

---

## Step 5 — Make the Changes

### Change 1: `src/runtime/loop.rs` — make `run()` async

Find this exact signature:
```rust
pub fn run<F>(&mut self, frontend: &mut F, ctx: &mut RuntimeContext)
```

Replace with:
```rust
/// Execute the runtime loop.
///
/// Must be called within a Tokio runtime context (e.g., `#[tokio::main]`
/// or `block_on`). The async signature enforces `.await` at compile time
/// for the loop path.
pub async fn run<F>(&mut self, frontend: &mut F, ctx: &mut RuntimeContext)
```

The body does **not** change. No `await` points are added inside the loop.

### Change 2: `src/runtime/loop.rs` — update existing test

Find:
```rust
runtime.run(&mut frontend, &mut ctx);
```

Replace with:
```rust
runtime.run(&mut frontend, &mut ctx).await;
```

### Change 3: `src/runtime/loop.rs` — add new anchor test

Add this test after `test_ref_05_headless_loop_terminates`, still inside `mod tests`:

```rust
#[tokio::test]
async fn test_ref_07_async_run_terminates() {
    let mock = Arc::new(MockApiClient::new(vec![]));
    let client = ApiClient::new_mock(mock);
    let conversation = ConversationManager::new_mock(client, HashMap::new());

    let (tx, update_rx) = mpsc::unbounded_channel::<UiUpdate>();
    let mut ctx = RuntimeContext::new(
        conversation,
        tx,
        tokio_util::sync::CancellationToken::new(),
    );
    let mode = crate::app::TuiMode::new();
    let mut runtime = Runtime::new(mode, update_rx);

    let mut frontend = HeadlessFrontend::new(vec!["hello"], 2);
    runtime.run(&mut frontend, &mut ctx).await;

    assert_eq!(frontend.render_count, 2);
}
```

### Change 4: `src/runtime/context.rs` — add guard to `start_turn()`

Find the start of `start_turn`:
```rust
pub fn start_turn(&mut self, input: String) {
    self.conversation.push_user_message(input);
```

Replace with:
```rust
pub fn start_turn(&mut self, input: String) {
    // Guard: refuse to spawn without an active Tokio runtime.
    // Must precede push_user_message so history stays clean on error path.
    if tokio::runtime::Handle::try_current().is_err() {
        let _ = self.update_tx.send(UiUpdate::Error(
            "runtime error: start_turn requires active Tokio runtime".to_string(),
        ));
        return;
    }

    self.conversation.push_user_message(input);
```

**Critical ordering rule:** the guard must come before `push_user_message`. The no-runtime test asserts that `messages_for_api().is_empty()` after the guard fires. If you push first then guard, that assertion fails and history is corrupted on error.

### Change 5: `src/runtime/context.rs` — add no-runtime guard test

Add this test inside the existing `mod tests` block, after `test_ref_04_start_turn_dispatches_message`:

```rust
/// REF-07: calling start_turn without a Tokio runtime must not panic.
/// Emits UiUpdate::Error and leaves conversation history untouched.
#[test]
fn test_ref_07_no_runtime_guard() {
    let (tx, mut rx) = mpsc::unbounded_channel::<UiUpdate>();
    let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
    let conversation = ConversationManager::new_mock(client, HashMap::new());
    let mut ctx = RuntimeContext::new(conversation, tx, CancellationToken::new());

    // No #[tokio::test] — no runtime is active.
    ctx.start_turn("test".to_string());

    // Must emit an error, not spawn.
    let update = rx.try_recv().expect("expected error update");
    match update {
        UiUpdate::Error(msg) => {
            assert!(
                msg.contains("requires active Tokio runtime"),
                "unexpected error message: {msg}"
            );
        }
        _ => panic!("expected UiUpdate::Error, got something else"),
    }

    // No message appended to history on guard failure.
    assert!(
        ctx.conversation.messages_for_api().is_empty(),
        "history must stay clean when guard fires"
    );
}
```

**Why plain `#[test]` and not `#[tokio::test]`:** the absence of `#[tokio::test]` is what makes this test work — there is no active Tokio runtime, so `Handle::try_current()` returns `Err`, triggering the guard. If you accidentally use `#[tokio::test]`, the guard will never fire and the test will hang waiting for a stream response from the empty mock.

---

## Step 6 — Fix Any Remaining Call Sites

If Step 3 found call sites for `run()` outside the test module:

```bash
# Re-check after edits
grep -rn "\.run(" src/ | grep -v target | grep -v "test_ref\|#\[test\]"
```

For each remaining call site:
- If inside `async fn` or `#[tokio::main]`: add `.await`
- If inside `std::thread::spawn` or sync main: wrap with `tokio::runtime::Runtime::new().unwrap().block_on(...)`

---

## Step 7 — Verify

Run the four REF anchor tests individually first, then the full suite:

```bash
# New REF-07 tests
cargo test test_ref_07_async_run_terminates -- --nocapture
cargo test test_ref_07_no_runtime_guard -- --nocapture

# Prior anchors must stay green
cargo test test_ref_05_headless_loop_terminates -- --nocapture
cargo test test_ref_04_start_turn_dispatches_message -- --nocapture

# Full suite
cargo test --all

# Clippy (must be clean — treat warnings as errors)
cargo clippy --all-targets -- -D warnings
```

---

## Definition of Done Checklist

- [ ] `Runtime::run()` is `pub async fn run(...)`
- [ ] `start_turn()` checks `Handle::try_current()` before any other logic
- [ ] Guard fires → `UiUpdate::Error` emitted → function returns (no panic, no spawn)
- [ ] Guard fires → `messages_for_api()` is empty (history not touched)
- [ ] `test_ref_07_async_run_terminates` passes
- [ ] `test_ref_07_no_runtime_guard` passes (plain `#[test]`, NOT `#[tokio::test]`)
- [ ] `test_ref_05_headless_loop_terminates` still passes
- [ ] `test_ref_04_start_turn_dispatches_message` still passes
- [ ] `cargo test --all` green
- [ ] `cargo clippy --all-targets -- -D warnings` clean

---

## What NOT to Do

- Do NOT make `FrontendAdapter` methods async
- Do NOT add new `UiUpdate` variants — use the existing `Error(String)`
- Do NOT change the spawn/stream logic inside `start_turn` when a runtime IS present
- Do NOT refactor `App`, `TuiMode`, or anything outside the two target files
- Do NOT use `#[tokio::test]` on `test_ref_07_no_runtime_guard` — it must be a plain sync test
