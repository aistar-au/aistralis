# PR: REF-04-pre — Expose ConversationManager Dispatch Surface

**Track:** REF  
**ADR:** REF-06-7  
**Blocks:** REF-04, REF-06 Tasks A–D

---

## Summary

Exposes the three methods that the runtime engine requires to dispatch a conversation turn
(`push_user_message`, `messages_for_api`, `client()`), wraps `ApiClient` in `Arc`
internally inside `ConversationManager`, and adds a `create_stream_with_cancel` stub to
`ApiClient` with a `CancellationToken` parameter. No behavioural changes — all existing
tests pass unchanged.

---

## Changes

### `src/state/conversation.rs`

**`client` field type changed: `ApiClient` → `Arc<ApiClient>`**

Constructors (`new`, `new_mock`) accept `ApiClient` by value and wrap internally.
Call sites are unchanged. The Arc is an implementation detail; no caller needs to manage
it externally.

**Three new public methods:**

```rust
pub fn push_user_message(&mut self, input: String)
pub fn messages_for_api(&self) -> Vec<ApiMessage>
pub fn client(&self) -> Arc<ApiClient>
```

`send_message` now delegates its user-message push to `push_user_message`, removing the
duplicated `api_messages.push(...)` inline.

`Arc` import promoted from `#[cfg(test)]` to unconditional (required by the `client`
field type).

### `src/api/client.rs`

**New method added immediately before `create_stream`:**

```rust
pub async fn create_stream_with_cancel(
    &self,
    messages: &[ApiMessage],
    token: tokio_util::sync::CancellationToken,
) -> Result<ByteStream> {
    let _ = token;
    self.create_stream(messages).await
}
```

Token is accepted but not yet wired. The signature is final; REF-04 will implement
actual cancellation without an API change.

### `src/app/mod.rs`

Added `use crate::api::ApiClient;` import. `ApiClient::new(&config)?` call site
simplified (removed a spurious `Arc::new` wrapper that was immediately dereferenced).

### `Cargo.toml` / `Cargo.lock`

Added `tokio-util = "0.7"` (default features). Only `tokio_util::sync::CancellationToken`
is used; no additional feature flags required.

---

## Design Notes

**Why Arc wrapping is internal:** `ConversationManager` owns the client; the runtime
gets a clone-on-demand via `client()`. This keeps construction call sites stable when
new `RuntimeMode` implementors (TUI, batch, headless) are added — they call
`ctx.conversation.client()` rather than requiring the caller to have constructed an Arc.

**Why `create_stream_with_cancel` ignores the token:** The stub establishes the correct
call signature at the correct location so REF-04's implementation can be a surgical
fill-in. A `// TODO(REF-04): wire cancellation` comment marks the site in source.

---

## Testing

All existing tests pass. No new tests in this PR — the `#[ignore]`d REF-04 anchor test
(`test_ref_04_start_turn_dispatches`) remains ignored; `start_turn` is still a no-op
stub. Completing it is REF-04's scope.

```
cargo test            # green
cargo clippy          # clean
cargo fmt --check     # clean
```

---

## Checklist

- [x] `push_user_message` / `messages_for_api` / `client()` exposed
- [x] `client` stored as `Arc<ApiClient>`; constructors accept by value
- [x] `create_stream_with_cancel` stub compiles with correct signature
- [x] `tokio-util` default features only (no `features = ["full"]`)
- [x] No double-Arc anti-pattern in `app/mod.rs`
- [x] All tests green
- [ ] REF-04: implement `start_turn` (next PR)
