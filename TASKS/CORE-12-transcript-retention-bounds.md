# Task CORE-12: Transcript Retention Bounds

**Target File:** `src/app/mod.rs`
**Depends on:** `CORE-09`
**Can run in parallel with:** `CORE-07`, `CORE-08`, `FEAT-15`

**Issue:** Transcript history growth must be bounded in memory without changing runtime-core dispatch ownership.

**Definition of Done:**
1. Add bounded transcript retention behavior in TUI history state (ring buffer or compaction policy).
2. Preserve streaming correctness while pruning (assistant slot updates remain valid for active turns).
3. Keep runtime-core canonical dispatch unchanged; do not add alternate app-owned message routing paths.
4. Add tests that prove deterministic pruning behavior at and beyond configured limits.

**Anchor Verification:** Long-running transcripts remain memory-bounded while runtime-core-only dispatch invariants stay intact.
