# Task CRIT-03: State Synchronization Verification

**Target File:** `src/app/mod.rs`

**Issue:** The `App` struct uses `Arc<Mutex<ConversationManager>>` for state sharing, but there's no test verifying that background tasks can successfully mutate the shared state.

**Definition of Done:**
1. Add a test that spawns a background task to mutate the conversation state.
2. Verify the main thread can see the mutation.
3. Test `test_crit_03_state_sync` passes.

**Context:**
The `conversation` field in `App` is wrapped in `Arc<Mutex<>>` to allow shared access between the UI thread and background API tasks. This test verifies that the synchronization primitive works correctly.

**Anchor Test:** `test_crit_03_state_sync` in `src/app/mod.rs`

**Expected Behavior:**
```rust
// Background task should be able to:
let mut lock = conversation.lock().await;
lock.add_message("test".into());

// Main thread should see:
let lock = conversation.lock().await;
assert!(lock.has_message("test"));