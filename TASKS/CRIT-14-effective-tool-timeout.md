# Task CRIT-14: Effective Tool Timeout

**Target File:** `src/state/conversation.rs`

**Issue:** Timeout wrappers must be enforceable even when tool execution is blocking.

**Definition of Done:**
1. Run tool execution in cancellable spawned blocking context.
2. Enforce `AISTAR_TOOL_TIMEOUT_SECS` via timeout on spawned task.
3. Return deterministic timeout error text and keep turn state consistent.
4. Add/keep regression coverage for timeout path behavior.

**Anchor Verification:** Timeout expires under blocking tool execution and returns the expected timeout error.

