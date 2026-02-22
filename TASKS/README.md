# Task Dispatch Map

This is the full sequence across all tracks. Docs and TUI are independent
tracks, so they can run in parallel across teams or sessions.

## Active ADR Dispatch Manifests

Uncompleted dispatch ADRs live in `TASKS/` root.

1. `TASKS/ADR-013-tui-completion-deployment-plan.md`
2. `TASKS/ADR-018-managed-tui-scrollback-streaming-cell-overlays.md`
3. `TASKS/ADR-019-adr-018-follow-up-correctness-cutover-cleanup.md`

## Manifests Added In This Wave (6 Files)

1. `TASKS/ADR-013-tui-completion-deployment-plan.md`
2. `TASKS/CORE-12-bounded-transcript.md`
3. `TASKS/CORE-13-dirty-render-guard.md`
4. `TASKS/completed/CORE-14-panic-hook-terminal-restore.md`
5. `TASKS/FEAT-15-scrollback-viewport.md`
6. `TASKS/FEAT-16-idle-interrupt-input-drop-feedback.md`

## Dispatch Immediately (No Dependencies)

**CORE-14** (panic hook) must go first before any TUI work. Raw mode is already
live and a panic during development can leave the terminal broken without it.

**DOC-01** and **DOC-02** are editorial only and fully independent of
everything else.

## Docs Track (Independent Chain)

```text
CORE-02 -> CORE-03 -> CORE-04
```

`CORE-03` needs `book.toml` from `CORE-02` before workflow references are
valid. `CORE-04` needs the build wiring from `CORE-03` before generator wiring.

## TUI Track (Main Chain)

```text
CORE-09
  |
  |- CORE-07 ---- CORE-08
  |                 |
  |            +----+-----+
  |            CORE-10  CORE-13
  |              |
  |            CORE-11
  |              |
  |      +-------+------+--------+
  |     FEAT-10 FEAT-11 FEAT-13 FEAT-14
  |              |
  |            FEAT-12
  |
  |- CORE-12  (parallel from CORE-09)
  |- FEAT-15  (parallel from CORE-09)
  `- FEAT-16  (after CORE-10)
```

## Flat Dispatch Order (With Parallelism)

| Step | Task | Can run in parallel with |
| :--- | :--- | :--- |
| 1 | CORE-09 | CORE-14, docs track |
| 2 | CORE-07 | CORE-12, FEAT-15 |
| 3 | CORE-08 | CORE-12, FEAT-15 |
| 4 | CORE-10 | CORE-13 |
| 5 | CORE-11 | FEAT-16 |
| 6 | FEAT-10 | FEAT-11, FEAT-13, FEAT-14 |
| 7 | FEAT-11 | FEAT-10, FEAT-13, FEAT-14 |
| 8 | FEAT-12 | FEAT-13, FEAT-14 |

`CORE-12` (bounded transcript) and `FEAT-15` (scrollback) both touch
`src/app/mod.rs` history state and can dispatch immediately after `CORE-09`
without conflicting with `CORE-07`/`CORE-08` layout files. `FEAT-16` waits on
`CORE-10` because they share the interrupt routing path.

## Corrected Naming For The Five New Manifests

| Old name | Correct name | Reason |
| :--- | :--- | :--- |
| TUI-01 scrollback | FEAT-15 | User-visible behavior |
| TUI-02 bounded transcript | CORE-12 | Infrastructure, no new UI |
| TUI-03 dirty render guard | CORE-13 | Infrastructure, no new UI |
| TUI-04 panic hook | CORE-14 | Infrastructure, no new UI |
| TUI-05 idle interrupt + drop | FEAT-16 | User-visible behavior |
