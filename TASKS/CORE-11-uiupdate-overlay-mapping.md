# Task CORE-11: UiUpdate to Overlay Mapping

**Target File:** `src/app/mod.rs`

**Issue:** UiUpdate events need explicit overlay-state mapping and one-shot resolution guarantees.

**Definition of Done:**
1. Map `ToolApprovalRequest` into overlay state with captured responder.
2. Map errors to modal/history by documented policy.
3. Ensure overlay dismissal sends exactly one decision on responder channels.

**Anchor Verification:** Tool approval sender is resolved exactly once through overlay lifecycle.

