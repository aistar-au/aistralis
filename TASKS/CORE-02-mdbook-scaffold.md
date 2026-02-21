# Task CORE-02: mdBook Scaffold

**Target File:** `docs/`

**Issue:** The repository needs a canonical mdBook scaffold for docs publishing.

**Definition of Done:**
1. Add `docs/book.toml` configured for this repository.
2. Add base docs pages under `docs/src/` and wire them in `docs/src/SUMMARY.md`.
3. Set site base path for Pages to `/vexcoder/` unless repository/domain policy explicitly changes.
4. `mdbook build docs` succeeds with the new scaffold.

**Anchor Verification:** `mdbook build docs` exits successfully and generates site output.
