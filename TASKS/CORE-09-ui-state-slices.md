# Task CORE-09: UI State Slices In App Mod

**Target File:** `src/app/mod.rs`

**Issue:** UI state should be grouped into explicit slices for maintainability and testability.

**Decision Record (February 18, 2026):** Do not add a new global `src/state.rs`; keep UI state local to `App` in `src/app/mod.rs` to avoid collision with the existing `src/state/` runtime namespace.

**Definition of Done:**
1. Introduce grouped UI state holders (`HistoryState`, `InputState`, `OverlayState`).
2. Keep UI state local to `App` and avoid adding global `src/state.rs`.
3. Migrate ad-hoc fields into grouped structures without protocol changes.

**Anchor Verification:** UI behavior is unchanged while state ownership is explicit and grouped.
