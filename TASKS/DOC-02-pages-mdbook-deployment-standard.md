# Task DOC-02: GitHub Pages and mdBook Deployment Standard

**Target File:** `CONTRIBUTING.md`

**Issue:** Docs deployment prerequisites and permissions are not fully standardized in contributor guidance.

**Definition of Done:**
1. Document GitHub Pages preflight requirements:
   - Pages source set to GitHub Actions.
   - Required repository permissions and branch policy.
2. Document workflow permission minimums:
   - `pages: write`
   - `id-token: write`
3. Document canonical docs structure requirements:
   - `docs/book.toml`
   - `docs/src/SUMMARY.md`
4. Keep the guidance scoped to docs deployment and avoid unrelated runtime behavior changes.

**Anchor Verification:** Confirm the deployment standard section exists in `CONTRIBUTING.md` with all required bullets.
