# Contributing to vexcoder

> **Version:** This workflow applies from `v0.1.0-alpha` onward.  
> **Architecture decisions** live in [`docs/adr/`](docs/adr/README.md).  
> The ADRs explain *why* the project is structured this way. Read them before opening a PR.

---

## 🛠️ The Agentic Workflow (Test-Driven Manifest)

`vexcoder` uses the **Test-Driven Manifest (TDM)** strategy for all bug fixes, features, and refactors. The full rationale is in [ADR-001](docs/adr/ADR-001-tdm-agentic-manifest-strategy.md). The short version:

1. **Identify task** — Check `TASKS/` for open items.
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

See [`docs/dev/manifest-strategy.md`](docs/dev/manifest-strategy.md) for the operational guide.

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
│   ├── adr/                       # Architecture Decision Records (why)
│   │   ├── README.md
│   │   ├── ADR-001-tdm-agentic-manifest-strategy.md
│   │   ├── ADR-002-lexical-path-normalization.md
│   │   ├── ADR-003-dual-protocol-api-auto-detection.md
│   │   ├── ADR-004-runtime-seam-headless-first.md
│   │   └── ADR-005-cfg-test-mock-injection.md
│   └── dev/
│       └── manifest-strategy.md   # TDM operational guide (how)
├── TASKS/                         # Work orders (what + anchor)
│   ├── CRIT-01-protocol.md
│   ├── CORE-01-sse-parser.md
│   ├── SEC-01-path-security.md
│   └── completed/
├── src/
│   ├── api/                       # HTTP client, stream parser, mock
│   ├── app/                       # TUI mode + frontend wiring to runtime core
│   ├── config/                    # Environment variable loading
│   ├── edit_diff/                 # LCS-based diff renderer
│   ├── runtime/                   # Canonical runtime loop, mode traits, updates
│   ├── state/                     # ConversationManager, message history
│   ├── terminal/                  # ratatui/crossterm setup (TUI skeleton)
│   ├── tools/                     # ToolExecutor — filesystem + git
│   ├── types/                     # ApiMessage, Content, StreamEvent
│   └── ui/                        # ratatui render functions
└── tests/                         # Integration tests
```

---

## 🔗 Reference

- [ADR index](docs/adr/README.md) — architectural decisions and their rationale
- [Agentic Repair Strategy](docs/dev/manifest-strategy.md) — TDM workflow deep-dive
- [SECURITY.md](SECURITY.md) — vulnerability reporting
