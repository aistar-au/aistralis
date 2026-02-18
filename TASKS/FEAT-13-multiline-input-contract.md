# Task FEAT-13: Multiline Input Contract

**Target File:** `src/app/mod.rs`, `src/ui/render.rs`

**Issue:** Multiline input behavior must remain deterministic with pane expansion and overlay interactions.

**Definition of Done:**
1. `Shift+Enter` and `Ctrl+J` insert newline.
2. `Enter` submits only outside overlay mode.
3. Input pane expands to max rows then scrolls internally.

**Anchor Verification:** Multiline input behaves deterministically while preserving submit rules.

