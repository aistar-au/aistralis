# Contributing to vexcoder

> **Version:** This workflow applies from `v0.1.0-alpha` onward.  
> **Architecture decisions** live in [`TASKS/`](TASKS/ADR-README.md).  
> **Dispatch ADRs not yet completed** live in [`TASKS/`](TASKS/TASKS-DISPATCH-MAP.md) as `TASKS/ADR-XXX-*.md`.  
> The ADRs explain *why* the project is structured this way. Read them before opening a PR.

---

## 🛠️ The Agentic Workflow (Test-Driven Manifest)

`vexcoder` uses the **Test-Driven Manifest (TDM)** strategy for all bug fixes, features, and refactors. The full rationale is in [ADR-001](TASKS/completed/ADR-001-tdm-agentic-manifest-strategy.md). The short version:

1. **Identify task** — Check `TASKS/` for open items.
   - This includes active dispatch ADR manifests (`TASKS/ADR-XXX-*.md`).
2. **Anchor test** — Every task has exactly one failing Rust test before work begins. No anchor, no dispatch.
3. **Module isolation** — Work is confined to the `Target File` named in the task manifest (± one helper file).
4. **Verification** — Success is `cargo test <anchor_name>` passing, plus `cargo test --all-targets` showing no regressions.

Runtime mode additions and naming-policy changes require explicit confirmation before implementation or documentation. See ADR-007.
Canonical production dispatch is runtime-core only: `Runtime<M>::run` → `RuntimeMode::on_user_input` → `RuntimeContext::start_turn`.
Alternate app-owned dispatch channels are forbidden in production paths.
Runtime-core ratatui TUI behavior must conform to ADR-009, ADR-010, and ADR-011 before merge.
Runtime-core TUI deployment is gated by ADR-012; no deploy if any ADR-012 item is unmet.
Architecture gates enforcing ADR-007 must remain green:
`bash scripts/check_no_alternate_routing.sh`
`bash scripts/check_forbidden_imports.sh`
Tests that mutate process environment variables must hold `crate::test_support::ENV_LOCK`; `cargo test --all-targets` must pass without `--test-threads=1`.

See [`TASKS/manifest-strategy.md`](TASKS/manifest-strategy.md) for the operational guide.

---

## 🧾 Planning and Audit-Only Requests

Planning-only and audit-only requests are strictly no-touch by default:
no file create, edit, rename, move, or delete is allowed during a planning/audit-only pass.

If the user later asks to implement changes in the same session, switch to edit mode only
after explicit user confirmation.

Use the same explicit-confirmation standard already required for runtime mode additions and
naming-policy changes.

---

## 📚 Docs Deployment Standard (GitHub Pages + mdBook)

Docs deployment changes must follow this baseline:

1. GitHub Pages preflight:
   - Repository Pages source is set to **GitHub Actions**.
   - Repository and branch policy permit the docs workflow to run on the protected integration path
     (normally `main` via pull request merge).
2. Workflow permissions minimums:
   - `pages: write`
   - `id-token: write`
3. Canonical docs structure requirements:
   - `docs/book.toml`
   - `docs/src/SUMMARY.md`

Keep docs deployment guidance scoped to documentation publishing only.
Do not mix runtime behavior changes into deployment-standard edits.

---

## 📋 Task Naming Convention

| Prefix | Type | Example |
| :--- | :--- | :--- |
| `CRIT-XX` | Critical bug | `CRIT-02-serde-fix.md` |
| `FEAT-XX` | Feature | `FEAT-01-streaming-ui.md` |
| `REF-XX` | Refactor | `REF-02-runtime-contract.md` |
| `SEC-XX` | Security | `SEC-01-path-security.md` |
| `CORE-XX` | Core infrastructure | `CORE-01-sse-parser.md` |
| `DOC-XX` | Documentation | `DOC-01-api-docs.md` |

Completed tasks move to `TASKS/completed/` — do not delete them.

---

## 🧩 Rust Module File Naming (Rust 2018+)

Use path-based module entry files across `src/`.

| Situation | Required path |
| :--- | :--- |
| Top-level module entry | `src/<module>.rs` |
| Child module | `src/<module>/<child>.rs` |

Do not introduce new `src/*/mod.rs` files unless an external tool or macro
requires that layout.

---

## 🗺️ Runtime-core Status

REF-08 full cutover is complete and merged (2026-02-19).
Canonical dispatch and layering rules are now governed by ADR-007 and ADR-008.
Historical REF manifests remain archived under `TASKS/completed/`.

---

## 🚀 Quick Start

```bash
# 1. Install Rust (stable toolchain required)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"

# 2. Verify the environment
cargo test --all-targets

# 3. Pick a task from TASKS/, read its manifest, identify the anchor test

# 4. Implement, then verify
cargo test test_crit_XX_anchor_name -- --nocapture

# 5. Confirm no regressions
cargo test --all-targets
bash scripts/check_no_alternate_routing.sh
bash scripts/check_forbidden_imports.sh
```

---

## 📁 Project Structure

```
vexcoder/
├── CONTRIBUTING.md                # Workflow guide + source map
├── README.md                      # Runtime and quickstart
├── docs/                          # mdBook docs for GitHub Pages
├── TASKS/                         # ADRs and task manifests (open + completed)
├── src/                           # Rust crate source
│   └── bin/vex.rs                 # Binary entrypoint
└── tests/                         # Integration tests
```

---

## 🦀 Tracked Rust Source Map (`*.rs`)

| File | Short description (with raw URL) |
| :--- | :--- |
| `src/lib.rs` | Crate root exporting runtime/app/api/state/tools/ui modules. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/lib.rs> |
| `src/bin/vex.rs` | Production binary entrypoint and managed TUI startup loop. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/bin/vex.rs> |
| `src/api.rs` | API module entry and re-exports. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/api.rs> |
| `src/api/client.rs` | HTTP client, protocol selection, request/stream setup, tool schemas. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/api/client.rs> |
| `src/api/logging.rs` | Shared API debug/error logger and env-based log path handling. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/api/logging.rs> |
| `src/api/mock_client.rs` | Mock streaming client used by tests. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/api/mock_client.rs> |
| `src/api/stream.rs` | Stream/SSE event parsing helpers used by API layer. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/api/stream.rs> |
| `src/app.rs` | TUI mode state machine: input, overlays, history, and UI event handling. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/app.rs> |
| `src/config.rs` | Config loading/validation from environment variables. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/config.rs> |
| `src/edit_diff.rs` | Edit preview diff/hunk formatting utilities. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/edit_diff.rs> |
| `src/runtime.rs` | Runtime module entry and re-exports. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime.rs> |
| `src/runtime/context.rs` | Async turn execution context and conversation update forwarding. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/context.rs> |
| `src/runtime/frontend.rs` | Frontend adapter contracts and runtime-facing input event types. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/frontend.rs> |
| `src/runtime/loop.rs` | Runtime event loop orchestration between mode, frontend, and context. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/loop.rs> |
| `src/runtime/mode.rs` | Runtime mode trait defining input/update hooks. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/mode.rs> |
| `src/runtime/policy.rs` | Output sanitization and tool-evidence policy helpers. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/policy.rs> |
| `src/runtime/update.rs` | `UiUpdate` message types emitted from runtime to frontend. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/update.rs> |
| `src/state.rs` | State module entry and re-exports. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/state.rs> |
| `src/state/conversation.rs` | Core conversation loop: streaming parse, tool execution, approvals, pruning. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/state/conversation.rs> |
| `src/state/stream_block.rs` | Structured stream block models and tool status enum. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/state/stream_block.rs> |
| `src/terminal.rs` | Terminal raw-mode lifecycle and panic-safe restore guard. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/terminal.rs> |
| `src/test_support.rs` | Shared test synchronization helpers (e.g., env lock). Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/test_support.rs> |
| `src/tool_preview.rs` | Tool approval preview rendering and read-file snapshot summaries. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/tool_preview.rs> |
| `src/tools.rs` | Tools module entry and re-exports. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/tools.rs> |
| `src/tools/operator.rs` | Sandboxed file/git tool operator with path safety and literal search. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/tools/operator.rs> |
| `src/types.rs` | Types module entry and re-exports. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/types.rs> |
| `src/types/api_types.rs` | API request/response content and streaming event structs/enums. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/types/api_types.rs> |
| `src/ui.rs` | UI module entry and re-exports. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/ui.rs> |
| `src/ui/input_metrics.rs` | Input editor row/width metrics for viewport-safe rendering. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/ui/input_metrics.rs> |
| `src/ui/layout.rs` | Ratatui pane layout splitting and geometry helpers. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/ui/layout.rs> |
| `src/ui/render.rs` | Ratatui render functions for status, history, input, and overlays. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/ui/render.rs> |
| `src/util.rs` | Shared utility functions (bool/env parsing and endpoint helpers). Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/util.rs> |
| `tests/integration_test.rs` | Integration tests for config validation behavior. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/tests/integration_test.rs> |
| `tests/stream_parser_tests.rs` | Stream parser protocol and fragmentation tests. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/tests/stream_parser_tests.rs> |
| `tests/tool_operator_tests.rs` | Tool operator behavior/security tests for file and git actions. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/tests/tool_operator_tests.rs> |

`src/calculator.rs` is currently untracked and intentionally excluded from the tracked repo map above.

---

## 🔗 Reference

- [ADR index](TASKS/ADR-README.md) — architectural decisions and their rationale
- [Agentic Repair Strategy](TASKS/manifest-strategy.md) — TDM workflow deep-dive
