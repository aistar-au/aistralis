# Task CRIT-15: Prune Safety for Tool Pairing

**Target File:** `src/state/conversation.rs`

**Issue:** History prune must not retain orphaned `tool_result` messages without preceding assistant `tool_use`.

**Definition of Done:**
1. Prune anchor logic prevents first retained message from being orphan tool-result content.
2. Retained history remains protocol-valid for structured tool exchanges.
3. Add regression tests for orphan-tool-result edge cases near message limits.

**Anchor Verification:** Pruned history is empty or starts at a valid user message that is not a leading orphan tool-result block.

