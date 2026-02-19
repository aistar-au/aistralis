# Task FEAT-15: Scrollback Navigation Controls

**Target File:** `src/app/mod.rs`
**Depends on:** `CORE-09`
**Can run in parallel with:** `CORE-07`, `CORE-08`, `CORE-12`

**Issue:** Transcript viewport needs conventional user-visible scrollback controls and stable auto-follow behavior.

**Definition of Done:**
1. Support `PageUp`, `PageDown`, `Home`, and `End` transcript navigation in active TUI path.
2. Implement explicit auto-follow behavior; incoming output must not force-scroll to bottom when auto-follow is disabled.
3. Preserve overlay input lock and compose-input behavior while navigating history.
4. Add tests for navigation, auto-follow transitions, and streaming while scrolled.

**Anchor Verification:** Viewport navigation is deterministic and user-visible across streaming and non-streaming states.
