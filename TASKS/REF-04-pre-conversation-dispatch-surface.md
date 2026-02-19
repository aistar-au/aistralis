# TASK: REF-04-pre — Expose ConversationManager dispatch surface

**Status:** Merged (prerequisite for REF-04 Track A satisfied)
**Blocks:** REF-04 `start_turn` implementation
**Scope:** `src/state/conversation.rs`, `src/api/client.rs`

---

## Why this exists

`RuntimeContext::start_turn` is still a no-op stub until REF-04 Track A is wired.
REF-04 §0 discovery originally identified four API-surface gaps. This task
added those methods with minimal scope so Track A can implement dispatch.

---

## What to add

### 1. `ConversationManager::push_user_message`

```rust
// src/state/conversation.rs
pub fn push_user_message(&mut self, input: String) {
    self.api_messages.push(ApiMessage {
        role: "user".to_string(),
        content: input,
    });
}
```

Appends a user message to the internal `api_messages` vec. Does not call the
API. Counterpart to the push that currently happens inside `send_message`.

### 2. `ConversationManager::messages_for_api`

```rust
pub fn messages_for_api(&self) -> Vec<ApiMessage> {
    self.api_messages.clone()
}
```

Read-only snapshot of the current message history for passing to the API.

### 3. `ConversationManager::client` — requires Arc refactor

This is the non-trivial one. Currently `ApiClient` is `Clone` and stored by
value inside `ConversationManager`. To return a shared reference suitable for
passing across async tasks, it needs to be `Arc`-wrapped.

**Changes required:**

In `src/state/conversation.rs`:
- Change field `client: ApiClient` → `client: Arc<ApiClient>`
- Keep `new(client, executor)` and `new_mock(client, ...)` taking `ApiClient`,
  and wrap internally with `Arc::new(client)`
- Add accessor: `pub fn client(&self) -> Arc<ApiClient> { Arc::clone(&self.client) }`

In `src/app/mod.rs`:
- Keep `App::new()` constructing `ConversationManager::new(client, ...)`
  without call-site `Arc::new(...)` wrapping
- Confirm no other construction sites rely on call-site wrapping

**Grep to find all construction sites before starting:**
```bash
grep -n "ConversationManager::new\b" src/ -r
grep -n "ApiClient::new\b\|ApiClient::new_mock\b" src/ -r
```

### 4. `ApiClient::create_stream_with_cancel`

```rust
// src/api/client.rs
pub async fn create_stream_with_cancel(
    &self,
    messages: &[ApiMessage],
    token: tokio_util::sync::CancellationToken,
) -> Result<ByteStream> {
    // For now: ignore token, delegate to create_stream.
    // REF-04 Track A will wire cancellation properly.
    let _ = token;
    self.create_stream(messages).await
}
```

This unblocks `start_turn` compilation without requiring full cancellation
plumbing in the same PR. Cancellation wiring can be a follow-up.

---

## Verification

```bash
# All existing tests must stay green
cargo test --all

# New surface compiles and is accessible from context.rs
cargo check --all-targets

# Confirm no duplicate push logic left in send_message
grep -n "api_messages.push\|push_user" src/state/conversation.rs
```

---

## Definition of done

- [ ] `ConversationManager::push_user_message` exists and compiles
- [ ] `ConversationManager::messages_for_api` exists and compiles
- [ ] `ConversationManager::client() -> Arc<ApiClient>` exists
- [ ] `ApiClient` field in `ConversationManager` is `Arc<ApiClient>`
- [ ] `ApiClient::create_stream_with_cancel` exists (token ignored, delegates to `create_stream`)
- [ ] All existing tests pass — no regressions
- [ ] `cargo check --all-targets` clean

## What NOT to do

- Do not implement cancellation logic — just accept the token parameter and ignore it
- Do not change `send_message` behaviour or split it up
- Do not touch `src/runtime/context.rs` — that's REF-04 Track A's job once this merges
- Do not add new tests beyond what's needed to verify the new methods compile

---

## After this merges

Un-ignore `test_ref_04_start_turn_dispatches` in the `#[cfg(test)]` module in
`src/runtime/context.rs`, implement `start_turn` in `src/runtime/context.rs`,
and close REF-04.
