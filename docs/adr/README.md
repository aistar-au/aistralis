# Architecture Decision Records

This directory captures *why* significant decisions were made in `vexcoder`. ADRs are permanent — even superseded records stay in the log. When an agent or contributor questions a design choice, the ADR is the authoritative answer.

## Relationship to TASKS/

| File | Purpose | Audience |
| :--- | :--- | :--- |
| `docs/adr/ADR-XXX-*.md` | Why an architecture exists | Humans and agents reading context |
| `TASKS/ID-*.md` | What to implement and how to verify it | Agents executing a work order |
| `CONTRIBUTING.md` | How the workflow operates | All contributors |

A task manifest may reference an ADR. An ADR may spawn one or more tasks. They are complementary, not redundant.

## Status vocabulary

| Status | Meaning |
| :--- | :--- |
| **Proposed** | Under discussion, not yet binding |
| **Accepted** | In effect — code must conform |
| **Superseded by ADR-XXX** | Replaced; kept for history |
| **Deprecated** | Was accepted, no longer applies |

## Index

| ADR | Title | Status |
| :--- | :--- | :--- |
| [ADR-001](ADR-001-tdm-agentic-manifest-strategy.md) | Test-Driven Manifest (TDM) as primary agentic development methodology | Accepted |
| [ADR-002](ADR-002-lexical-path-normalization.md) | Lexical path normalization over `fs::canonicalize()` in tool executor | Accepted |
| [ADR-003](ADR-003-dual-protocol-api-auto-detection.md) | Dual-protocol API client with URL-inferred protocol selection | Accepted |
| [ADR-004](ADR-004-runtime-seam-headless-first.md) | Runtime seam refactor — headless-first architecture (REF track) | Superseded operationally by ADR-006 and ADR-007 |
| [ADR-005](ADR-005-cfg-test-mock-injection.md) | `#[cfg(test)]` mock injection field on production `ApiClient` struct | Accepted |
| [ADR-006](ADR-006-runtime-mode-contracts.md) | Runtime mode contracts — `RuntimeMode`, `RuntimeContext`, `RuntimeEvent`, `FrontendAdapter` | Accepted |
| [ADR-007](ADR-007-runtime-canonical-dispatch-no-alt-routing.md) | Runtime-core canonical dispatch — no alternate routing | Accepted |
| [ADR-008](ADR-008-runtime-cutover-parity-guardrails.md) | Runtime cutover parity guardrails | Accepted |
| [ADR-009](ADR-009-runtime-core-tui-interaction-contract.md) | Runtime-core TUI interaction contract | Accepted |
| [ADR-010](ADR-010-runtime-core-tui-viewport-and-transcript.md) | Runtime-core TUI viewport and transcript model | Accepted |
| [ADR-011](ADR-011-runtime-core-tui-render-loop-and-lifecycle.md) | Runtime-core TUI render loop and lifecycle | Accepted |
| [ADR-012](ADR-012-runtime-core-tui-deployment-gate.md) | Runtime-core TUI deployment gate | Accepted |
| [ADR-014](ADR-014-runtime-core-policy-dedup-and-enforcement.md) | Runtime-core policy deduplication and enforcement | Accepted |
| [ADR-015](ADR-015-local-endpoint-text-protocol-default.md) | Local endpoint text-protocol default for tool loop reliability | Accepted |
| [ADR-016](ADR-016-local-tool-loop-guard-and-correction.md) | Local tool-loop guard and correction path | Accepted |
| [ADR-017](ADR-017-append-terminal-single-session.md) | Append-terminal single session runtime | Accepted |

## Template

Copy this block to start a new ADR:

```markdown
# ADR-XXX: Title

**Date:** YYYY-MM-DD
**Status:** Proposed
**Deciders:** (names or roles)
**Related tasks:** TASKS/ID-description.md

## Context
## Decision
## Rationale
## Alternatives considered
## Consequences
## Compliance notes for agents
```
