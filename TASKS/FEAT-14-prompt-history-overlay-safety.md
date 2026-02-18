# Task FEAT-14: Prompt History Overlay Safety

**Target File:** `src/app/mod.rs`

**Issue:** Prompt history navigation should remain stable and isolated from overlay interactions.

**Definition of Done:**
1. Preserve stash/restore semantics for history up/down traversal.
2. Prevent history mutation while overlay is active.
3. Keep history behavior deterministic during streaming and modal transitions.

**Anchor Verification:** History traversal remains stable and does not interfere with overlay-focused input handling.
