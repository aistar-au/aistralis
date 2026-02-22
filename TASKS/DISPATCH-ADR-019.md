# Agent Dispatch: ADR-019 Follow-up Fix Queue (B1 → U1 → U4+D1 → D2 → U2+U3)

Use this with Codex to execute ADR-019 in strict order and keep the checklist/evidence current.

---

## Codex `/plan` prompt (copy/paste)

```text
/plan
Execute ADR-019 from TASKS/ADR-019-adr-018-follow-up-correctness-cutover-cleanup.md.

Hard requirements:
1) Follow exact order:
   B1 -> U1 -> U4 + D1 -> D2 -> U2 + U3
2) Do not start a later item before earlier gates are green.
3) Keep one focused commit per checklist item (U4+D1 may share one commit if both are completed together).
4) After each item:
   - run cargo test --all-targets
   - update TASKS/ADR-019-adr-018-follow-up-correctness-cutover-cleanup.md:
     - check the item box
     - append evidence block with Dispatcher, Commit SHA, files changed,
       insertions/deletions, line references, validation result, and notes
5) Insertion/deletion accounting must come from:
   git show --numstat --format="" <commit_sha>
6) Line references must be concrete path:line for changed behavior.
7) No text-sentinel control events may be introduced.
8) Keep runtime-core contracts consistent with ADR-006/ADR-007/ADR-008.

For this run, start now with B1 only.
Deliverables for this run:
- code + tests for B1
- one commit
- ADR-019 checklist/evidence updated for B1 with +/- counts and line refs
- report exact command outputs summary
```

---

## Per-item dispatcher prompt blocks

### B1 (Unicode-safe streaming deltas)

```text
Refer to CONTRIBUTING.md and TASKS/ADR-019-adr-018-follow-up-correctness-cutover-cleanup.md.
Implement B1 only:
- harden streamed delta slicing to be UTF-8 char-boundary safe
- add regression tests for multi-byte boundaries and partial deltas
Then:
1) run cargo test --all-targets
2) commit
3) run: git show --numstat --format="" <sha>
4) update ADR-019 checklist + evidence block with exact +/- counts and file:line refs
Do not start U1 yet.
```

### U1 (typed events, remove sentinels)

```text
Implement U1 only from ADR-019:
- replace magic scroll/control text sentinels with typed events
- remove collision-prone UserInputEvent::Text control routing
Then run tests, commit, collect numstat, and update ADR-019 evidence for U1.
Do not start U4/D1 yet.
```

### U4 + D1 (production cutover + promotion from test-only)

```text
Implement U4 and D1 from ADR-019:
- ensure production binary path is managed TUI (ADR-018 cutover)
- promote required editor/render logic from test-only to production modules
- preserve single runtime-core dispatch path
Then run tests, commit, collect numstat, and update ADR-019 evidence for U4 and D1.
Do not start D2 yet.
```

### D2 (StreamBlock no-op resolution)

```text
Implement D2 only from ADR-019:
- wire StreamBlock* updates into active render state OR remove dead no-op path
Then run tests, commit, collect numstat, and update ADR-019 evidence for D2.
Do not start U2/U3 yet.
```

### U2 + U3 (cleanup)

```text
Implement U2 and U3 from ADR-019:
- simplify streaming rendering to single-responsibility flow
- remove cfg(test)-dependent TuiMode field layout drift
Then run tests, commit, collect numstat, and update ADR-019 evidence for U2 and U3.
```

---

## Required evidence template (paste into ADR-019 after each item)

```markdown
### [B1|U1|U2|U3|U4|D1|D2] - <short title>
- Dispatcher: <name/id>
- Commit: <sha>
- Files changed:
  - `path/to/file.rs` (+<insertions> -<deletions>)
  - `path/to/other.rs` (+<insertions> -<deletions>)
- Line references:
  - `path/to/file.rs:<line>`
  - `path/to/other.rs:<line>`
- Validation:
  - `cargo test --all-targets` : pass/fail
- Notes:
  - <what was fixed and why>
```
