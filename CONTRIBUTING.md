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
├── CONTRIBUTING.md                # This file — TDM law
├── docs/
│   ├── book.toml                  # mdBook configuration (Pages source)
│   └── src/                       # mdBook content only
├── TASKS/                         # Dispatch manifests and work orders (what + anchor)
│   ├── ADR-README.md              # ADR index and status tracking
│   ├── ADR-013-tui-completion-deployment-plan.md
│   ├── ADR-018-managed-tui-scrollback-streaming-cell-overlays.md
│   ├── ADR-019-adr-018-follow-up-correctness-cutover-cleanup.md
│   ├── manifest-strategy.md       # TDM operational guide (how)
│   ├── CRIT-01-protocol.md
│   ├── CORE-01-sse-parser.md
│   ├── SEC-01-path-security.md
│   └── completed/
├── src/
│   ├── api.rs                     # Top-level entry for api module
│   ├── api/                       # HTTP client, stream parser, mock
│   ├── app.rs                     # Top-level entry for app module
│   ├── app/                       # TUI mode + frontend wiring to runtime core
│   ├── config.rs                  # Top-level entry for config module
│   ├── config/                    # Environment variable loading
│   ├── edit_diff/                 # LCS-based diff renderer
│   ├── runtime.rs                 # Top-level entry for runtime module
│   ├── runtime/                   # Canonical runtime loop, mode traits, updates
│   ├── state.rs                   # Top-level entry for state module
│   ├── state/                     # ConversationManager, message history
│   ├── terminal.rs                # Top-level entry for terminal module
│   ├── terminal/                  # ratatui/crossterm setup (TUI skeleton)
│   ├── tools.rs                   # Top-level entry for tools module
│   ├── tools/                     # ToolExecutor — filesystem + git
│   ├── types.rs                   # Top-level entry for types module
│   ├── types/                     # ApiMessage, Content, StreamEvent
│   ├── ui.rs                      # Top-level entry for ui module
│   ├── ui/                        # ratatui render functions
│   └── util.rs                    # Top-level entry for util helpers
└── tests/                         # Integration tests
```

---

## 🔗 Reference

- [ADR index](TASKS/ADR-README.md) — architectural decisions and their rationale
- [Agentic Repair Strategy](TASKS/manifest-strategy.md) — TDM workflow deep-dive
- [SECURITY.md](SECURITY.md) — vulnerability reporting
