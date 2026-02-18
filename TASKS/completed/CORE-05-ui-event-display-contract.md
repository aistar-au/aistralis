# Task CORE-05: UI Event Display Contract (Visual Parity)

**Status:** Superseded by Phase 5 tasks `FEAT-11-overlay-renderer-family.md` and `CORE-10-overlay-event-router.md`.
**Supersession Mapping:** Archived for record-keeping only; do not implement directly.

**Target File:** `CONTRIBUTING.md` plus renderer contract references

**Issue:** UI parity rules need to be codified as a deterministic contract instead of styling-only guidance.

**Definition of Done:**
1. Document two-zone layout contract:
   - top transcript
   - pinned bottom prompt surface
2. Document bottom prompt behavior:
   - dimmed panel
   - multiline expansion
   - cursor anchoring to prompt input
   - modal input lock during tool approval
3. Document transcript grammar contract:
   - symbols (`•`, `└`, phase bars)
   - spacing, wrapping, and truncation rules
4. Scope wording explicitly separates renderer-visible behavior from transport implementation details.

**Anchor Verification:** Contract section in docs includes all four required behavior groups and scope notes.
