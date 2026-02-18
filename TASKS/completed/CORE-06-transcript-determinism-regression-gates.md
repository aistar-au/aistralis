# Task CORE-06: Transcript Determinism and Visual Regression Gates

**Status:** Superseded by Phase 5 tasks `FEAT-12-diff-overlay-viewer.md` and the Phase 5 test-plan regression set.
**Supersession Mapping:** Archived for record-keeping only; do not implement directly.

**Target File:** transcript formatting/render test modules

**Issue:** The UI transcript output needs deterministic tests and prompt-surface parity checks to prevent regressions.

**Definition of Done:**
1. Add golden/snapshot tests for transcript formatting output.
2. Add parity tests for prompt-surface visibility during:
   - streaming state
   - tool execution state
   - error state
3. Ensure tests validate deterministic grammar and prevent noisy formatting regressions.
4. Integrate tests into normal `cargo test` execution path.

**Anchor Verification:** New regression tests run and fail on transcript/prompt-surface drift.
