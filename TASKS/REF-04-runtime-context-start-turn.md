# TASK: REF-04 — Wire `RuntimeContext::start_turn` to API dispatch

**Status:** Ready — REF-04-pre merged; Track A dispatch wiring pending  
**Phase:** 5 (correctness)  
**Track:** Runtime seam (REF track)  
**Depends on:** REF-03 green (`test_ref_03_tui_mode_overlay_blocks_input` passing)  
**Blocks:** REF-04 Track A follow-up dispatch wiring (does not block REF-05)  
**ADR:** ADR-006 §2 (`RuntimeContext`), §6 (`Runtime<M>` loop)  
**Scope:** `src/runtime/context.rs` (implementation + anchor unit tests), `src/runtime/mod.rs` (anchor regression fix), `src/app/mod.rs` (call sites only)

---

## Background

REF-02 created a `RuntimeContext<'a>` stub with a **borrowed**
`&'a mut ConversationManager` and a `start_turn` body that is a no-op
(`// wired in REF-04`). REF-03 used this borrowed shape in `TuiMode`
and in the `dummy_ctx()` test helper.

REF-04 Track B is complete and unblocks REF-05. `TASKS/REF-04-pre-conversation-dispatch-surface.md`
is merged; remaining REF-04 work is Track A dispatch wiring.

REF-04 changes the `RuntimeContext` struct shape in two ways:

1. **Owned field:** `conversation: ConversationManager` (no lifetime parameter).
   The `<'a>` borrowed form was a stub convenience. ADR-006 §2 requires owned
   to avoid threading a lifetime through the runtime loop in REF-05.
2. **New field:** `cancel: CancellationToken` for per-turn cancellation.

After this task `RuntimeContext` has no lifetime parameter. All call sites in
`src/app/mod.rs` and the `dummy_ctx()` helper in the REF-03 test block must be
updated to construct the owned form.

REF-03 also left `TuiMode::on_user_input` with a `TODO(REF-04)` comment that
drops `input` and `ctx`. That placeholder is removed in this task.

---

## Step 0 — Discover real API method names before writing any code

The implementation guide in §3 uses descriptive placeholder names. Before
writing any implementation, run these commands and record the actual names:

```bash
# ConversationManager public methods
grep -n "pub fn\|pub async fn" src/state/conversation.rs

# ApiClient public methods
grep -n "pub fn\|pub async fn" src/api/client.rs

# Existing mock client pattern
grep -n "pub fn\|impl " src/api/mock_client.rs 2>/dev/null || echo "no mock_client.rs"

# How App currently dispatches a turn (what to replace)
grep -n "send_message\|message_tx\|turn\|dispatch" src/app/mod.rs
```

Fill in the table below before touching any `.rs` file. If a row has no match,
it is a **gap** — follow the gap procedure in §4 instead of guessing.

| Placeholder name | Real name (fill in) | File |
| :--- | :--- | :--- |
| `push_user_message(input)` | | `src/state/conversation.rs` |
| `messages_for_api()` | | `src/state/conversation.rs` |
| `client()` → `Arc<ApiClient>` | | `src/state/conversation.rs` |
| `stream_messages(msgs, token)` | | `src/api/client.rs` |
| `ConversationManager::new_with_client(client)` | | `src/state/conversation.rs` |
| `MockApiClient::with_response(chunks)` | | `src/api/mock_client.rs` |

---

## Scope — files permitted to change

| File | Change |
| :--- | :--- |
| `src/runtime/context.rs` | Replace borrowed stub with owned struct; implement `start_turn` and `cancel_turn`; add/update REF-04 anchor unit tests |
| `src/runtime/mod.rs` | Update REF-02 anchor compile check for non-generic `RuntimeContext` |
| `src/app/mod.rs` | Update `dummy_ctx()` helper; remove old dispatch call sites |
| `Cargo.toml` | Add `tokio-util` if not already present (see §2) |

All other files are **out of scope**. If a compile error requires touching
another file, stop and raise it as a gap.

---

## §1 — Migrate `RuntimeContext` from borrowed stub to owned

Replace the entire contents of `src/runtime/context.rs`:

```rust
use crate::runtime::UiUpdate;
use crate::state::ConversationManager;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Capability surface passed to `RuntimeMode` methods.
///
/// Owns `ConversationManager` (not a borrow) so that REF-05's runtime loop
/// can hold it without a lifetime parameter. See ADR-006 §2.
pub struct RuntimeContext {
    pub(crate) conversation: ConversationManager,
    pub(crate) update_tx: mpsc::UnboundedSender<UiUpdate>,
    pub(crate) cancel: CancellationToken,
}

impl RuntimeContext {
    pub fn new(
        conversation: ConversationManager,
        update_tx: mpsc::UnboundedSender<UiUpdate>,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            conversation,
            update_tx,
            cancel,
        }
    }

    pub fn start_turn(&mut self, input: String) {
        // Implemented in §3 below.
        todo!("REF-04: implement start_turn")
    }

    pub fn cancel_turn(&mut self) {
        // Implemented in §3 below.
        todo!("REF-04: implement cancel_turn")
    }
}
```

The `todo!` stubs are temporary scaffolding; they are replaced in §3.
Writing the struct first and confirming `cargo check` passes before
proceeding avoids compound errors.

The REF-04 anchor test lives in a `#[cfg(test)]` module in
`src/runtime/context.rs`. Keep fields `pub(crate)` and use the public
`RuntimeContext::new(...)` constructor so test and production construction
follow the same path.

Also update the `dummy_ctx()` helper in `src/app/mod.rs` (inside the
existing `#[cfg(test)]` block). The owned form requires a real
`ConversationManager`; use the same mock constructor used in the anchor
test (see §5):

```rust
// src/app/mod.rs — update dummy_ctx() in the existing #[cfg(test)] block
#[cfg(test)]
fn dummy_ctx() -> crate::runtime::context::RuntimeContext {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    // Substitute real constructor — see §0 table
    let conversation = /* REAL_CONSTRUCTOR */;
    crate::runtime::context::RuntimeContext::new(
        conversation,
        tx,
        tokio_util::sync::CancellationToken::new(),
    )
}
```

Also update `src/runtime/mod.rs` so the REF-02 anchor remains green after
removing the `RuntimeContext<'a>` lifetime:

```rust
// before (REF-02 borrowed shape):
let _ = std::mem::size_of::<Option<RuntimeContext<'static>>>();

// after (REF-04 owned shape):
let _ = std::mem::size_of::<Option<RuntimeContext>>();
```

Confirm `cargo check --all-targets` passes after this step before continuing.

---

## §2 — Add `tokio-util` if needed

```bash
grep "tokio-util" Cargo.toml
```

If absent:

```toml
tokio-util = { version = "0.7", features = ["rt"] }
```

Alternative if you prefer no new dependency: replace `CancellationToken` with
`Arc<std::sync::atomic::AtomicBool>`. Adapt `child_token()` → `Arc::clone(&self.cancel)`,
`is_cancelled()` → `load(Ordering::Relaxed)`, and the reset in `cancel_turn()` →
`store(false, Ordering::Relaxed)`.

---

## §3 — Implement `start_turn` and `cancel_turn`

Replace the `todo!` stubs with real bodies. Substitute all placeholder names
with the real names from the §0 table.

```rust
impl RuntimeContext {
    pub fn start_turn(&mut self, input: String) {
        // 1. Record the user message in conversation history.
        self.conversation.REAL_push_user_message(input.clone());

        // 2. Child token: cancel_turn() stops only THIS task, not future ones.
        //    Using the root token directly would poison all future turns.
        let turn_cancel = self.cancel.child_token();

        // 3. Clone owned data before spawn. The borrow checker rejects holding
        //    &mut self across .await boundaries inside the closure.
        let tx = self.update_tx.clone();
        let messages = self.conversation.REAL_messages_for_api();
        let client = self.conversation.REAL_client();  // expects Arc<ApiClient>

        tokio::spawn(async move {
            let result = client
                .REAL_stream_messages(messages, turn_cancel.clone())
                .await;

            match result {
                Ok(mut stream) => {
                    while let Some(delta) = stream.next().await {
                        if turn_cancel.is_cancelled() { break; }
                        let _ = tx.send(UiUpdate::StreamDelta(delta));
                    }
                    let _ = tx.send(UiUpdate::TurnComplete);
                }
                Err(e) => {
                    let _ = tx.send(UiUpdate::Error(e.to_string()));
                }
            }
        });
    }

    pub fn cancel_turn(&mut self) {
        self.cancel.cancel();
        self.cancel = CancellationToken::new();
    }
}
```

### Remove old dispatch call sites

```bash
grep -n "message_tx\|send_message\|turn_dispatch" src/app/mod.rs
```

For each hit: if the call site is inside an `on_user_input` path, delete it
(`ctx.start_turn` is now the dispatch). If it is inside `App::new`'s spawned
task and is needed for `update_rx` setup, leave it with `// REF-05: migrate`.
Do not delete the `update_rx` receiver field — REF-05 migrates it.

---

## §4 — Gap procedure

If any method from the §0 table does not exist in the codebase:

1. Do not invent or add the missing method. That is outside this task's scope.
2. Commit what compiles with `// REF-04: gap — <method_name> not found on <Type>` in place of the call.
3. Mark the anchor test as `#[ignore]` with a comment explaining which gap blocks it.
4. Update this task's status to **Blocked** and record the gap in a comment at the top of the file.

The definition of done has two tracks (see §6) precisely because a gap is a
valid outcome — it surfaces a missing precondition, not a failure.

---

## §5 — Anchor test

```rust
// src/runtime/context.rs (inside #[cfg(test)] mod tests)
use super::RuntimeContext;
use crate::runtime::UiUpdate;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn test_ref_04_start_turn_dispatches_message() {
    let (tx, mut rx) = mpsc::unbounded_channel::<UiUpdate>();

    // Substitute the real mock constructor from src/api/mock_client.rs (ADR-005).
    // If no mock exists, see §4 gap procedure.
    let client = REAL_MockApiClient::with_response(vec!["Hello", " world"]);
    let conversation = REAL_ConversationManager::new_with_client(client);

    let mut ctx = RuntimeContext::new(conversation, tx, CancellationToken::new());

    ctx.start_turn("test input".to_string());

    let mut saw_delta = false;
    let mut saw_complete = false;
    loop {
        match tokio::time::timeout(
            std::time::Duration::from_millis(500),
            rx.recv()
        ).await {
            Ok(Some(UiUpdate::StreamDelta(_))) => saw_delta = true,
            Ok(Some(UiUpdate::TurnComplete)) => { saw_complete = true; break; }
            Ok(Some(UiUpdate::Error(e))) => panic!("unexpected error: {e}"),
            Ok(None) | Err(_) => break,
            _ => {}
        }
    }

    assert!(saw_delta, "expected at least one StreamDelta");
    assert!(saw_complete, "expected TurnComplete");
}
```

---

## §6 — Definition of done (two tracks)

### Track A — no gaps found

- [ ] `RuntimeContext` struct has no lifetime parameter; fields are `conversation: ConversationManager`, `update_tx`, `cancel`.
- [ ] `RuntimeContext::new(conversation, update_tx, cancel)` exists and is used by the anchor unit test.
- [ ] `start_turn` and `cancel_turn` fully implemented — no `todo!` or `// wired in REF-04` comments remain in `src/runtime/context.rs`.
- [ ] `src/runtime/mod.rs` REF-02 anchor no longer references `RuntimeContext<'static>` and remains green with the owned shape.
- [ ] `dummy_ctx()` in `src/app/mod.rs` constructs the owned form.
- [ ] `test_ref_04_start_turn_dispatches_message` passes.
- [ ] `test_ref_03_tui_mode_overlay_blocks_input` passes (regression).
- [ ] `test_ref_02_runtime_types_compile` passes (regression).
- [ ] `cargo test --all` green.
- [ ] `cargo clippy -- -D warnings` clean.
- [ ] No active duplicate dispatch paths in `src/app/mod.rs`.

### Track B — gap(s) found

- [ ] All found methods from §0 table are implemented.
- [ ] Missing methods replaced with `// REF-04: gap — <description>` comments.
- [ ] Anchor test marked `#[ignore]` with gap explanation.
- [ ] Task status updated to **Blocked** at top of file.
- [ ] `cargo check --all-targets` passes (gaps must not cause compile errors).
- [ ] `test_ref_03_tui_mode_overlay_blocks_input` passes (regression).
- [ ] `test_ref_02_runtime_types_compile` passes (regression).
- [ ] Gap documented; next action is for the human architect to add the missing method to `ConversationManager` or `ApiClient`.

---

## Common failure modes

**Lifetime compile errors after §1:** the existing REF-03 test anchor uses
`dummy_ctx<'a>() -> RuntimeContext<'a>`. After removing the lifetime from
`RuntimeContext`, that signature no longer compiles. Fix: update `dummy_ctx`
to return `RuntimeContext` (no lifetime) as shown in §1.

**Double dispatch:** both `ctx.start_turn` and the old `message_tx.send` path
remain active. Symptom: assistant response appears twice per submit.
Fix: delete the old call site.

**Borrow across `.await`:** holding `&mut conversation` across an `.await`
boundary is rejected by the borrow checker. Fix: clone owned data (`messages`,
`client` Arc) before the `tokio::spawn` call. See §3 pattern.

**`CancellationToken` child vs root:** passing `self.cancel` directly (not
`child_token()`) into the spawn means `cancel_turn()` cancels the root and
poisons all future turns. Always use `child_token()` per turn.

---

## Notes for REF-05

REF-05 adds `Runtime<M>::run()`. At that point `update_rx` moves from `App`
to `Runtime<M>`, and `RuntimeContext` is constructed inside `run()` on each
tick from components `Runtime<M>` owns. Leave `// REF-05: migrate` annotations
at relevant call sites and stop.
