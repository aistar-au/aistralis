# REF-08 Delta Notes

This directory captures the reviewed REF-08 cutover deltas that were staged in
`/tmp` and reconciled with the merged branch state.

**Archive status:** Completed and archived under `TASKS/completed/`.
**Working copy:** `TASKS/completed/REF-08-working-copy/`.
**Consistency baseline:** ADR-008 and ADR-012.

## Delta index

- `WORKING-COPY-DELTA-A-assistant-stream-separation.md`
- `WORKING-COPY-DELTA-B-blockdelta-stream-filter.md`
- `WORKING-COPY-DELTA-C-input-editor-utf8-safety.md`
- `WORKING-COPY-DELTA-D-frontend-mode-aware-poll-contract.md`
- `WORKING-COPY-DELTA-E-typed-interrupt-routing.md`
- `WORKING-COPY-DELTA-F-deterministic-env-tests-and-cancel-progression.md`
- `WORKING-COPY-review-checklist.md`

## Notes

- Delta A/B/C matched implementation and were imported directly.
- Delta D from `/tmp` was stale and has been corrected to the typed
  `UserInputEvent` frontend contract.
- Delta E/F document the remaining REF-008 guardrails implemented on this
  branch (typed interrupt behavior, post-cancel progression proof, and
  deterministic env-mutating tests).
- Merged `main` history (including PR-13 commit `0e83eef`) continues to enforce
  ADR-007 no-alternate-message-routing checks.
