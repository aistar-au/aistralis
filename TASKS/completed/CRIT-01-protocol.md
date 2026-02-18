# Task CRIT-01: Anthropic Protocol Mock Test

**Target File:** `src/state/conversation.rs`

**Issue:** The `ConversationManager` handles multi-turn conversations with tool use, but there's no test verifying the correct protocol flow: User message -> Assistant response -> Tool use -> Tool result -> Final response.

**Definition of Done:**
1. Create a mock API client that simulates Anthropic's streaming response format.
2. Add test `test_crit_01_protocol_flow` that verifies the full conversation loop.
3. Test passes with `cargo test test_crit_01_protocol_flow`.

**Context:**
The Anthropic API expects a specific message format:
1. User sends a message
2. Assistant responds (possibly with tool_use blocks)
3. If tool_use exists, client must send tool_result
4. Assistant provides final response

This test verifies the `send_message` method correctly:
- Builds the message history
- Handles tool execution
- Returns the final assistant text

**Anchor Test:** `test_crit_01_protocol_flow` in `src/state/conversation.rs`

**Expected Behavior:**
```rust
// Mock should simulate:
// 1. First response: assistant text + tool_use
// 2. Second response: final text after tool_result

let final_text = manager.send_message("What is in file.txt?".into(), None).await?;
assert!(final_text.contains("file content"));