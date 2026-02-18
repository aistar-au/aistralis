# Task CORE-04: IaC Auto-Doc Generation Pipeline

**Target File:** `scripts/` and docs build wiring

**Issue:** IaC documentation generation must run automatically and fail loudly when tools or generation steps break.

**Definition of Done:**
1. Add a generator script executed before `mdbook build`.
2. Generate Terraform, Helm, and OpenAPI markdown into the docs source tree.
3. Ensure generator failures stop the pipeline (no silent empty output).
4. Wire the generator into local docs build path and CI docs workflow path.

**Anchor Verification:** A docs build run executes generator first and fails non-zero on generator/tool errors.
