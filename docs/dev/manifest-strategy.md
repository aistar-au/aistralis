# Agentic Repair: The Test-Driven Manifest (TDM)

This document outlines the architectural routine for maintaining and evolving the `aistar` codebase using the **Test-Driven Manifest (TDM)** strategy. This approach is designed to maximize the efficacy of AI coding agents while strictly preventing regressions.

## The Core Philosophy

Large Language Models (LLMs) perform best when given **narrow focus** and **binary success criteria**. By moving tasks out of a monolithic README and into atomic, test-backed manifest files, we overcome the "context window" limitations of modern AI tools.

---

## The 3-Pillar Workflow

### 1. The Task Manifest (`TASKS/`)
Every significant bug fix or feature starts as a file in the `TASKS/` directory. 
- **File Naming:** `ID-short-description.md` (e.g., `CRIT-01-protocol.md`).
- **Content:** Must define the **Target File** and the **Definition of Done**.
- **Context Management:** Task files should never exceed 2k tokens.

### 2. The Anchor (Failing Test)
Before an agent is allowed to touch production logic, an "Anchor" must be established.
- **The Red Phase:** The Architect (Human) writes a minimal test at the bottom of the target module that reproduces the bug or asserts the missing feature.
- **The Compilation Check:** If the bug is structural, the Anchor may even be a failing `cargo check` (Type-level anchor).

### 3. The Dispatch (The Prompt)
Agents are invoked with a specific "tri-point" context:
1. The **TDM Philosophy** (`CONTRIBUTING.md`).
2. The **Active Task** (`TASKS/Task-ID.md`).
3. The **Anchor Location** (`src/path/to/file.rs`).



---

## Routine for the Architect

As the Lead Architect, your role is to manage the "State Machine" of the repository:

| Phase | Responsibility | Goal |
| :--- | :--- | :--- |
| **Drafting** | Write the `.md` task and the failing Rust test. | Establish "Red" state. |
| **Dispatch** | Feed the task to the agent (Aider/Gemini). | Trigger "Green" attempt. |
| **Verification** | Run `cargo test --all` and review the diff. | Ensure no regressions. |
| **Promotion** | Move task to `TASKS/completed/` and merge. | Update repository history. |

---

## Regression Proofing

Because every fix is tied to a test, we build a **Safety Net** of regression tests. Over time, these tests move from the "Anchor" section into a formal `tests/` directory or a persistent `mod tests` block, ensuring that Agent B never breaks the work of Agent A.

## Prompting Pattern

When delegating to an agent, always use the following template to maintain the TDM loop:

> "Refer to `CONTRIBUTING.md`. I am assigning you **Task [ID]**. 
> 1. Read `TASKS/[ID].md`. 
> 2. Identify the failing test `[test_name]` in `[file_path]`. 
> 3. Modify the code to make the test pass. 
> 4. Verify with `cargo test`."

## Change Confirmation Rule

- Agents must request explicit confirmation before adding or documenting new runtime flags, new operational modes, or new naming-convention policies.
- This rule applies to both implementation and documentation updates.
- Standard bug fixes that do not add flags/modes/policy changes do not require extra confirmation.

---

## Task Naming Convention

| Prefix | Type | Example |
|--------|------|---------|
| `CRIT-XX` | Critical bugs | `CRIT-01-protocol.md` |
| `CORE-XX` | Core runtime behavior | `CORE-01-sse-parser.md` |
| `FEAT-XX` | Feature requests | `FEAT-01-streaming-ui.md` |
| `REF-XX` | Refactoring tasks | `REF-01-error-handling.md` |
| `SEC-XX` | Security hardening | `SEC-01-path-security.md` |
| `DOC-XX` | Documentation tasks | `DOC-01-change-safety-policy.md` |

---

## Task Snapshot

Archived manifests live in `TASKS/completed/`.

Current highlights include:
- Core and critical hardening manifests (`CORE-*`, `CRIT-*`, `SEC-*`).
- Full runtime seam/cutover track:
  `REF-02` through `REF-08` are archived in `TASKS/completed/`.
- REF-08 delta documentation is archived at
  `TASKS/completed/REF-08-deltas/` (working copy under `docs/dev/ref-08/`).

If new work is opened, create manifests in `TASKS/` root first, then promote to
`TASKS/completed/` after verification.

## 🔒 Security Design: Lexical + Canonical Guard
In **SEC-01**, we avoid using `std::fs::canonicalize()` on non-existent target paths, but we do use canonicalization for guard checks on existing paths.
- **Why lexical first:** New file targets may not exist yet; lexical normalization handles `.`/`..` without requiring disk existence.
- **Why canonical guard second:** Existing ancestor paths are canonicalized to block symlink-escape and traversal bypasses.
- **Resulting model:** Resolve and validate candidate paths lexically, then canonicalize an existing guard path to enforce workspace containment.
