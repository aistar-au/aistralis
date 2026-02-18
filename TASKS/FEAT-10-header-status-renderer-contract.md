# Task FEAT-10: Header and Status Renderer Contract

**Target File:** `src/ui/render.rs`, `src/app/mod.rs`

**Issue:** Header/status row contract should remain stable and explicit.

**Definition of Done:**
1. Keep top-row header/status reserved in all TUI states.
2. Show compact mode/approval/history/repo status values.
3. Preserve contract under streaming and overlay states.

**Anchor Verification:** Header row remains stable and populated across interactive states.

