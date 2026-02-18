# Task CORE-08: Frame Composition Order

**Target File:** `src/app/mod.rs`, `src/ui/render.rs`

**Issue:** Render order must be deterministic with overlay z-order guarantees.

**Definition of Done:**
1. Render order is fixed: header -> history -> input -> overlay.
2. Overlay render remains last and does not alter pane geometry.
3. Add test/snapshot coverage for draw order guarantees.

**Anchor Verification:** Overlay draw occurs after base panes across all interactive states.

