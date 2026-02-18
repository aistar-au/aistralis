# Task CORE-10: Overlay Event Router

**Target File:** `src/app/mod.rs`

**Issue:** Overlay focus must hard-lock input routing while modal state is active.

**Definition of Done:**
1. `Overlay::None` routes to normal input editor keymap.
2. `Overlay::Some` routes only to overlay keymap and blocks message submit.
3. Input cursor/focus behavior is consistent while overlay is active.

**Anchor Verification:** Input submit is impossible while overlay is active; overlay keys resolve modal actions.

