# Task CORE-07: Layout Manager Extraction

**Target File:** `src/ui/layout.rs` (new) and `src/app/mod.rs`

**Issue:** Pane split logic should be centralized for deterministic 3-pane rendering.

**Definition of Done:**
1. Add canonical layout helper for header/history/input split.
2. Use helper from `draw_tui_frame`.
3. Preserve dynamic input-height behavior.

**Anchor Verification:** Frame always splits into header, history, input in fixed order.

