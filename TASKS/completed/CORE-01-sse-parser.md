# Task CORE-01: SSE Stream Parser Refinement
**Target File:** src/api/stream.rs

**Problem:** The current SSE parser may fail on fragmented packets or double-newline delimiters (\n\n), leading to duplicate events or missed data.

**Requirements:**
1. Implement a buffer-draining loop.
2. Split strictly on "\n\n".
3. Handle partial JSON fragments by retaining them in the buffer.
4. Filter out Anthropic 'ping' events.

**Definition of Done:**
- Anchor test 'test_sse_fragmentation' passes.
- No duplicate ContentBlockDelta events during streaming.
