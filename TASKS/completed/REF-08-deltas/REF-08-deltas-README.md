# REF-08 Delta Notes

This directory captures the reviewed REF-08 cutover deltas that were staged in
`/tmp` and reconciled with the merged branch state.

**Archive status:** Completed and archived under `TASKS/completed/`.
**Working copy:** `TASKS/completed/REF-08-working-copy/`.
**Consistency baseline:** ADR-008 and ADR-012.

## Delta index

- `DELTA-A-assistant-stream-separation.md`
- `DELTA-B-blockdelta-stream-filter.md`
- `DELTA-C-input-editor-utf8-safety.md`
- `DELTA-D-frontend-mode-aware-poll-contract.md`
- `DELTA-E-typed-interrupt-routing.md`
- `DELTA-F-deterministic-env-tests-and-cancel-progression.md`
- `review-checklist.md`

## Notes

- Delta A/B/C matched implementation and were imported directly.
- Delta D from `/tmp` was stale and has been corrected to the typed
  `UserInputEvent` frontend contract.
- Delta E/F document the remaining REF-008 guardrails implemented on this
  branch (typed interrupt behavior, post-cancel progression proof, and
  deterministic env-mutating tests).
