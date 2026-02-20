# Task FEAT-11: Overlay Renderer Family

**Target File:** `src/ui/render.rs`, `src/app/mod.rs`

**Issue:** Overlay rendering should use one shared modal contract for all modal types.

**Definition of Done:**
1. Support modal classes for command confirm, patch approve, tool permission, and error.
2. Use shared centered modal layout with clear body bounds and shortcut footer.
3. Reuse common rendering primitives instead of per-modal ad-hoc formatting.

**Anchor Verification:** All modal classes render through a unified renderer path.

