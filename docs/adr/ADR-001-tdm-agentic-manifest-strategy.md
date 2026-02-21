# ADR-001: Test-Driven Manifest (TDM) as primary agentic development methodology

**Date:** 2026-02-18  
**Status:** Accepted  
**Deciders:** Core maintainer  
**Related tasks:** All `TASKS/` files inherit this decision. See `docs/dev/manifest-strategy.md` for the operational guide.

---

## Context

`vexcoder` is itself a coding agent, and its own development is conducted with heavy use of LLM coding agents (Aider, Claude, Gemini, etc.). This creates a unique meta-problem:

**How do you maintain engineering discipline — no regressions, clear ownership, auditable changes — when the implementor is a stateless language model that forgets everything between sessions?**

Standard practices break down in this environment:

- A long-form spec in a README is too large for an agent's effective context window. The agent drifts, implements the wrong thing, or conflates separate concerns.
- Free-form "fix this bug" prompts give agents no binary success criterion, so they declare success when the code merely compiles.
- Agent A fixing CRIT-01 has no mechanism to prevent it from breaking CRIT-02 without explicit regression anchors.
- Architectural drift: agents instructed to "refactor X" will simultaneously add features, rename things, and change APIs if not constrained.

The conventional ADR + issue tracker workflow assumes a stateful human contributor who accumulates context over time. It does not translate to a zero-memory agent that must reconstruct context from files alone.

---

## Decision

Adopt the **Test-Driven Manifest (TDM)** as the canonical workflow for all bug fixes, features, and refactors:

1. **Every task lives in a single file** in `TASKS/` no larger than ~2 KB (approximately 500 tokens). This is the *maximum effective context payload* for a single agentic dispatch.

2. **Every task defines exactly one anchor test** — a failing Rust test that encodes the binary success criterion. The task is complete when and only when `cargo test <anchor_name>` passes.

3. **Tasks are atomic**: one task, one target file, one anchor. A task that requires touching three modules is split into three tasks with an explicit dependency chain.

4. **Agents are dispatched via `COMMAND_TO_AGENT.txt`**, a file that provides the three-point context: TDM philosophy (`CONTRIBUTING.md`), the active task (`TASKS/ID.md`), and the anchor location. This file is ephemeral and overwritten per dispatch.

5. **The human architect owns the red phase**: writing the failing test and the task manifest. Agents own the green phase: making the test pass without breaking existing anchors.

6. **Completed tasks move to `TASKS/completed/`** rather than being deleted, forming an auditable repair history.

---

## Rationale

### Why manifest files over a standard issue tracker?

GitHub Issues and Jira are web interfaces not accessible to a CLI agent running in a terminal. File-based task manifests are universally accessible. An agent reading `TASKS/CRIT-01-protocol.md` has everything it needs in one `cat` command.

### Why size-constrained manifests?

LLM performance degrades significantly when the relevant context is buried in a large document. A 500-token manifest with a single anchor keeps the agent's attention focused on exactly one problem. This is not a human UX decision — it is a deliberate engineering constraint shaped by how transformer attention works in practice.

### Why anchor tests instead of acceptance criteria prose?

Prose acceptance criteria are ambiguous. An anchor test is a Rust function that either passes or fails — there is no interpretation required. It is also directly executable by the agent as part of its verification loop (`cargo test test_crit_01_protocol_flow`).

### Why split tasks that touch multiple modules?

An agent given broad scope will take broad action. Constraining the task to one file bounds the blast radius of any mistake. It also makes code review tractable: a diff touching only `src/state/conversation.rs` is easy to audit; a diff touching eight files is not.

---

## Alternatives considered

### Standard GitHub Issues + PR workflow (without TDM)

Works well for human contributors with persistent memory. Fails for agents because the issue description is not accessible from the CLI, the success criterion is prose, and there is no mechanism preventing inter-task regressions.

### Test files only, no task manifests

Anchor tests alone do not give an agent enough context to understand *what* to implement. The manifest provides the background (what is broken and why) that the agent needs to write correct code, not just code that passes the test.

### Single large AGENTS.md / CLAUDE.md file

Popularised by llama.cpp and other AI-native projects. Works well for stable conventions but degrades for per-task context because the file grows unbounded. `vexcoder` uses `CONTRIBUTING.md` for stable workflow conventions and `TASKS/` for per-task ephemeral context — separating the two concerns.

### Monorepo-style ADR for every decision

Standard ADR practice (as documented in this directory) is used for *architectural* decisions. TDM task manifests are *work orders* — they are distinct in scope, format, and lifecycle. Neither replaces the other.

---

## Consequences

**Easier:**
- Any capable LLM can be dispatched to any task without project-specific onboarding.
- Regression protection accumulates automatically as the anchor test suite grows.
- The architect can reason about progress by counting passing vs. failing anchors.
- Code review scope is bounded by the task's `Target File` constraint.

**Harder:**
- Cross-cutting refactors that touch many files must be decomposed into sequential tasks, which takes more upfront planning.
- The human architect must write the failing test (the red phase) before dispatching an agent. This is intentional but requires discipline.
- Removing completed tasks from `TASKS/` is tempting but destroys audit history.

**Constraints imposed on future work:**
- New bug reports must produce a task manifest + anchor test before any agent is dispatched. "Fix this" prompts with no manifest are out of scope for the TDM loop.
- Task manifests must be kept under ~2 KB. If a task description grows beyond that, it is a signal to decompose the task.
- The `TASKS/completed/` directory must not be purged as part of cleanup automation.

---

## Compliance notes for agents

When executing any task in this repository:

1. Read `CONTRIBUTING.md` first. It is the TDM law, not a suggestion.
2. Your success criterion is `cargo test <anchor_name>`, not the absence of compiler errors.
3. Do not modify files outside the `Target File` specified in the task manifest unless the task explicitly lists additional files.
4. Do not add new CLI flags, modes, or environment variables unless the task manifest explicitly calls for them.
5. Run `cargo test --all` before declaring the task complete to verify no regressions.
