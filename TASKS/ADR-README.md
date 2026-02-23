# Architecture Decision Records (TASKS Canonical)

All ADR files live under `TASKS/`.

- Open/dispatch ADRs: `TASKS/ADR-XXX-*.md`
- Accepted/superseded ADR records: `TASKS/completed/ADR-XXX-*.md`

`docs/` is reserved for mdBook/GitHub Pages content and must not be used to
store ADR files.

## Status vocabulary

| Status | Meaning |
| :--- | :--- |
| **Proposed** | Under discussion and dispatchable |
| **Accepted** | In effect — code must conform |
| **Superseded by ADR-XXX** | Replaced; retained for history |
| **Deprecated** | Was accepted, no longer applies |

## Open ADRs (Dispatch)

| ADR | Title | Status |
| :--- | :--- | :--- |
| [ADR-013](ADR-013-tui-completion-deployment-plan.md) | TUI completion and deployment plan | Proposed |
| [ADR-018](ADR-018-managed-tui-scrollback-streaming-cell-overlays.md) | Managed TUI scrollback, streaming cell, overlays | Proposed |
| [ADR-021](ADR-021-codebase-audit-dead-weight-duplication-shared-code-opportunities.md) | Codebase audit: dead weight, duplication, and shared-code opportunities | Accepted (follow-up maintenance items remain) |

## Completed ADR Records

| ADR | Title | Status |
| :--- | :--- | :--- |
| [ADR-001](completed/ADR-001-tdm-agentic-manifest-strategy.md) | Test-Driven Manifest (TDM) as primary agentic development methodology | Accepted |
| [ADR-002](completed/ADR-002-lexical-path-normalization.md) | Lexical path normalization over `fs::canonicalize()` in tool executor | Accepted |
| [ADR-003](completed/ADR-003-dual-protocol-api-auto-detection.md) | Dual-protocol API client with URL-inferred protocol selection | Accepted |
| [ADR-004](completed/ADR-004-runtime-seam-headless-first.md) | Runtime seam refactor — headless-first architecture (REF track) | Superseded operationally by ADR-006 and ADR-007 |
| [ADR-005](completed/ADR-005-cfg-test-mock-injection.md) | `#[cfg(test)]` mock injection field on production `ApiClient` struct | Accepted |
| [ADR-006](completed/ADR-006-runtime-mode-contracts.md) | Runtime mode contracts — `RuntimeMode`, `RuntimeContext`, `RuntimeEvent`, `FrontendAdapter` | Accepted |
| [ADR-007](completed/ADR-007-runtime-canonical-dispatch-no-alt-routing.md) | Runtime-core canonical dispatch — no alternate routing | Accepted |
| [ADR-008](completed/ADR-008-runtime-cutover-parity-guardrails.md) | Runtime cutover parity guardrails | Accepted |
| [ADR-009](completed/ADR-009-runtime-core-tui-interaction-contract.md) | Runtime-core TUI interaction contract | Accepted |
| [ADR-010](completed/ADR-010-runtime-core-tui-viewport-and-transcript.md) | Runtime-core TUI viewport and transcript model | Accepted |
| [ADR-011](completed/ADR-011-runtime-core-tui-render-loop-and-lifecycle.md) | Runtime-core TUI render loop and lifecycle | Accepted |
| [ADR-012](completed/ADR-012-runtime-core-tui-deployment-gate.md) | Runtime-core TUI deployment gate | Accepted |
| [ADR-014](completed/ADR-014-runtime-core-policy-dedup-and-enforcement.md) | Runtime-core policy deduplication and enforcement | Accepted |
| [ADR-015](completed/ADR-015-local-endpoint-text-protocol-default.md) | Local endpoint text-protocol default for tool loop reliability | Accepted |
| [ADR-016](completed/ADR-016-local-tool-loop-guard-and-correction.md) | Local tool-loop guard and correction path | Accepted |
| [ADR-017](completed/ADR-017-append-terminal-single-session.md) | Append-terminal single session runtime | Superseded by ADR-018 |
| [ADR-019](completed/ADR-019-adr-018-follow-up-correctness-cutover-cleanup.md) | ADR-018 follow-up sequencing for correctness, cutover, and cleanup | Accepted |
| [ADR-020](completed/ADR-020-looping-architecture-enriched-response-correctness.md) | Looping architecture and enriched tool response correctness | Accepted |
