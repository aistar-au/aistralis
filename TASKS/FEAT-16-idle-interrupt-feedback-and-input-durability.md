# Task FEAT-16: Idle Interrupt Feedback and Input Durability

**Target File:** `src/app/mod.rs`
**Depends on:** `CORE-10`
**Can run in parallel with:** `CORE-11`

**Issue:** Idle interrupt behavior and submit-while-busy handling need explicit user-visible outcomes; silent drop is forbidden.

**Definition of Done:**
1. Idle `Ctrl+C` triggers defined exit feedback/behavior in the active TUI path.
2. Submitting input while a turn is in progress must be preserved (queue/restore) or explicitly rejected with visible feedback.
3. Keep typed interrupt routing (`UserInputEvent::Interrupt`) and avoid text-sentinel fallbacks.
4. Add tests for idle interrupt feedback and non-lossy active-turn submit behavior.

**Anchor Verification:** Idle interrupt path is visible/deterministic and active-turn submit cannot silently drop user input.
