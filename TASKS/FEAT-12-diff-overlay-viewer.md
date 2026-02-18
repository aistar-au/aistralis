# Task FEAT-12: Diff Overlay Viewer

**Target File:** `src/ui/render.rs`, `src/app/mod.rs`

**Issue:** Diff approval requires readable, scrollable modal rendering in constrained terminal sizes.

**Definition of Done:**
1. Add diff overlay with scroll support (`Up/Down/PageUp/PageDown`).
2. Render line semantics for add/delete/context rows.
3. Preserve modal approve/deny key bindings.

**Anchor Verification:** Large diffs are scrollable and readable in modal without breaking approval flow.

