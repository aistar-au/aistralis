# ADR REF-06-7 — REF-04-pre Completion & Arc Ownership Design Record

**Status:** Accepted  
**Track:** REF (Refactor)  
**Parent ADR:** REF-06 (TUI Dispatch Integration)  
**Closes:** REF-04-pre (ConversationManager dispatch surface)  
**Merged as:** PR #7 (corrected from PR #8 draft)  
**Date:** 2026-02-19

---

## Context

REF-04-pre was the gating blocker for the entire REF track. Two competing PRs (#7, #8)
implemented the same surface but diverged on where `Arc` wrapping should live. A third
corrected version was merged, resolving the divergence in favour of PR #8's encapsulation
approach with three targeted fixes applied.

This document records:

1. What REF-04-pre delivered
2. The design decision on `Arc<ApiClient>` ownership
3. The three corrections made before merge
4. What is now unblocked

---

## What REF-04-pre Delivered

### `ConversationManager` dispatch surface (new public API)

```rust
// src/state/conversation.rs

pub fn push_user_message(&mut self, input: String) {
    self.api_messages.push(ApiMessage {
        role: "user".to_string(),
        content: Content::Text(input),
    });
}

pub fn messages_for_api(&self) -> Vec<ApiMessage> {
    self.api_messages.clone()
}

pub fn client(&self) -> Arc<ApiClient> {
    Arc::clone(&self.client)
}
```

`send_message` now delegates to `push_user_message` internally — single definition of
how a user message enters the history.

### `Arc<ApiClient>` storage

`client` field on `ConversationManager` changed from `ApiClient` to `Arc<ApiClient>`.

### `create_stream_with_cancel` stub on `ApiClient`

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

Token is ignored at this stage. Cancellation wiring is a REF-04 deliverable.

---

## Decision: Arc Wrapping is Internal to `ConversationManager`

### Alternatives considered

**Option A (PR #7):** Callers pass `Arc<ApiClient>`; constructors accept `Arc<ApiClient>`.

```rust
// call site
ConversationManager::new(Arc::new(client), executor)
// constructor signature
pub fn new(client: Arc<ApiClient>, executor: ToolExecutor) -> Self
```

**Option B (PR #8 corrected, merged):** Constructors accept `ApiClient` by value and
wrap internally.

```rust
// call site — unchanged from before REF-04-pre
ConversationManager::new(client, executor)
// constructor — wraps internally
pub fn new(client: ApiClient, executor: ToolExecutor) -> Self {
    Self { client: Arc::new(client), ... }
}
```

### Rationale for Option B

The `Arc` is an implementation detail of how `ConversationManager` shares the client
with the runtime context. No caller outside `ConversationManager` needs to hold a
co-owned Arc handle; `client()` provides a clone on demand for the runtime to use
during a turn. Leaking `Arc` to call sites would encode the sharing strategy in every
construction site, which increases friction when a new mode (e.g., a future batch mode)
is added — those callers should not be aware of or responsible for the wrapping.

The design choice ensures that adding `TuiMode` or any other `RuntimeMode` implementor
never requires touching `ConversationManager` construction call sites.

---

## Three Corrections Applied to PR #8 Before Merge

### Fix 1 — `tokio-util` features

| | Cargo.toml entry |
|---|---|
| PR #8 (original) | `tokio-util = { version = "0.7", features = ["full"] }` |
| Merged | `tokio-util = "0.7"` |

`features = ["full"]` pulled in hashbrown, slab, futures-io, futures-util as transitive
deps for a stub that ignores the cancellation token. Default features cover
`tokio_util::sync::CancellationToken` with no additional weight.

### Fix 2 — `create_stream_with_cancel` placement in `client.rs`

| | Location |
|---|---|
| PR #8 (original) | After `create_stream` (end of inherent block, line ~175) |
| Merged | Immediately before `create_stream` (line ~88, within the streaming method group) |

The stub and the method it delegates to belong together. Moving it earlier ensures a
reader sees the full cancellable surface alongside the non-cancellable one.

### Fix 3 — Redundant double-Arc in `app/mod.rs`

PR #8 (original) introduced:

```rust
// app/mod.rs
let client = Arc::new(crate::api::ApiClient::new(&config)?);
let conversation = Arc::new(Mutex::new(ConversationManager::new(
    (*client).clone(),  // deref + clone: dead Arc, internal re-wraps
    executor,
)));
```

The outer `Arc::new(client)` was dead weight — immediately dereferenced and cloned to
produce a plain `ApiClient` so `ConversationManager::new` could re-wrap it. The merged
version removes the outer Arc entirely:

```rust
// app/mod.rs
let client = ApiClient::new(&config)?;
let conversation = Arc::new(Mutex::new(ConversationManager::new(client, executor)));
```

This also prompted adding an explicit `use crate::api::ApiClient;` import to keep the
call site readable without the `crate::api::` path prefix.

---

## Migration Checklist Update (REF-04-pre items)

```
[x] ConversationManager::push_user_message exposed
[x] ConversationManager::messages_for_api exposed
[x] ConversationManager::client() -> Arc<ApiClient> exposed
[x] client field is Arc<ApiClient> internally
[x] new() and new_mock() constructors wrap internally (call sites unchanged)
[x] create_stream_with_cancel stub on ApiClient
[x] tokio-util added (default features only)
[x] All existing tests green (Arc wrapping is transparent to tests)
[ ] REF-04 gap: start_turn() is still a no-op stub — test_ref_04_start_turn_dispatches still #[ignore]
```

---

## What Is Now Unblocked

| Task | Was blocked by | Status |
|------|---------------|--------|
| REF-04 | `start_turn` no-op, no dispatch surface | **Unblocked** — surface exists |
| REF-06 Task A | Independent | **Unblocked** |
| REF-06 Task B | Independent | **Unblocked** |
| REF-06 Task C | D1 (Task B) | Unblocked after Task B |
| REF-06 Task D | D4 (Task C) | Unblocked after Task C |

Next recommended action: **REF-06 Task A** (RuntimeContext::new constructor). It is
independent, low-risk, and defuses the D3 time-bomb before REF-04 lands.

---

## Consequences

**Positive:**
- REF track is unblocked end-to-end
- `ConversationManager` construction call sites are stable across future mode additions
- `tokio-util` dep is minimal (no surprise transitive crates)
- `client()` accessor provides a valid shared handle for runtime use without external Arc management

**Negative / Trade-offs:**
- `create_stream_with_cancel` token is silently ignored; callers must not rely on cancellation until REF-04 wires it. The stub signature is correct so future wiring requires no API changes.
- `messages_for_api()` clones the full message Vec on every call. Acceptable for now; REF-04 should evaluate whether a slice reference suffices for the streaming path.
