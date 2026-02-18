# Task CORE-03: Docs Workflow with Pinned Versions

**Target File:** `.github/workflows/docs-build-and-deploy.yml`

**Issue:** Docs CI/CD workflow must be standardized with explicit branch behavior and pinned tool versions.

**Definition of Done:**
1. Add docs workflow file at `.github/workflows/docs-build-and-deploy.yml`.
2. Workflow builds docs on pull requests.
3. Workflow deploys only on `main` for non-PR runs.
4. Pin required tools/versions in workflow:
   - Rust `1.93.1`
   - mdBook `0.5.0`
   - terraform-docs `0.20.0`
   - helm-docs `1.14.2`
   - Node.js LTS `24.13.0` (optional step for OpenAPI tooling)
5. Use minimum required deploy permissions:
   - `pages: write`
   - `id-token: write`

**Anchor Verification:** Workflow YAML includes trigger logic, deploy guard, pinned versions, and required permissions.
