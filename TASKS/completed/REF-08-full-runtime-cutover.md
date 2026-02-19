# TASK: REF-08 — Full Runtime-Core Cutover

**Status:** Completed (merged to `main` on 2026-02-19; PR-13 guardrail commit `0e83eef` included in merged history)  
**Track:** REF (Refactor)  
**Depends on:** REF-07  
**Blocks:** none (final REF-track task)  
**ADRs:** ADR-006, ADR-007, ADR-008, ADR-009, ADR-010, ADR-011, ADR-012  

## Scope

1. `src/app/mod.rs`
2. `src/runtime/context.rs`
3. `src/util/mod.rs` (new)
4. `src/lib.rs`
5. `scripts/check_no_alternate_routing.sh` (new)
6. `scripts/check_forbidden_imports.sh` (new)
7. `.github/workflows/arch-contracts.yml` (new)
8. `docs/adr/` updates

## Problem Statement

Before REF-08, `App` still owned an alternate dispatch path (`message_tx` /
`update_rx` worker) running in parallel with runtime-core dispatch.

## Completed Outcomes

1. Canonical dispatch only:
   user input now flows through `Runtime<M>::run` ->
   `RuntimeMode::on_user_input` -> `RuntimeContext::start_turn`.
2. Alternate app routing removed:
   no `message_tx`/`message_rx` worker and no production `send_message` call
   site outside runtime context; no alternate message routing path remains
   outside `RuntimeContext::start_turn`.
3. Runtime protocol parity and safety hardening landed:
   REF-08 deltas A-F are implemented and documented.
4. Layering/contract enforcement added to CI:
   architecture checks and `cargo test --all-targets`.
5. ADR alignment completed:
   ADR-007/ADR-008 define cutover rules;
   ADR-009..ADR-012 define TUI completion/deployment contracts.

## Validation and Verification

Validated gates for REF-08 completion:

1. `cargo test --all-targets`
2. `bash scripts/check_no_alternate_routing.sh`
3. `bash scripts/check_forbidden_imports.sh`

Post-merge confirmation on `main` (including PR-13 commit `0e83eef`) keeps the
same no-alternate-routing guarantees green via checks #2 and #3.

## Delta Archive

Authoritative archived delta records:

1. `TASKS/completed/REF-08-deltas/DELTA-A-assistant-stream-separation.md`
2. `TASKS/completed/REF-08-deltas/DELTA-B-blockdelta-stream-filter.md`
3. `TASKS/completed/REF-08-deltas/DELTA-C-input-editor-utf8-safety.md`
4. `TASKS/completed/REF-08-deltas/DELTA-D-frontend-mode-aware-poll-contract.md`
5. `TASKS/completed/REF-08-deltas/DELTA-E-typed-interrupt-routing.md`
6. `TASKS/completed/REF-08-deltas/DELTA-F-deterministic-env-tests-and-cancel-progression.md`
7. `TASKS/completed/REF-08-deltas/review-checklist.md`

Working references (mirrored copy):

1. `docs/dev/ref-08/`

## Notes

1. REF-08 is complete and archived.
2. Runtime-core TUI deployment remains governed by ADR-012 no-go policy.
