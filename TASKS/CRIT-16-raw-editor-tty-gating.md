# Task CRIT-16: Raw Editor TTY Gating

**Target File:** `src/app/mod.rs`

**Issue:** Raw prompt editor mode must not activate when stdout is redirected.

**Definition of Done:**
1. Require both stdin and stdout TTY for raw prompt editor mode in non-TUI path.
2. Align `stream_input_mode` and raw-mode restore checks with `App::new()` TTY gating.
3. Ensure redirected stdout output does not contain interactive cursor-control leakage.

**Anchor Verification:** `aistar | tee out.log` path contains no raw prompt control-sequence noise.

