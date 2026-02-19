# Contributing to aistar

> **Version:** This workflow applies from `v0.1.0-alpha` onward.  
> **Architecture decisions** live in [`docs/adr/`](docs/adr/README.md).  
> The ADRs explain *why* the project is structured this way. Read them before opening a PR.

---

## 🛠️ The Agentic Workflow (Test-Driven Manifest)

`aistar` uses the **Test-Driven Manifest (TDM)** strategy for all bug fixes, features, and refactors. The full rationale is in [ADR-001](docs/adr/ADR-001-tdm-agentic-manifest-strategy.md). The short version:

1. **Identify task** — Check `TASKS/` for open items.
2. **Anchor test** — Every task has exactly one failing Rust test before work begins. No anchor, no dispatch.
3. **Module isolation** — Work is confined to the `Target File` named in the task manifest (± one helper file).
4. **Verification** — Success is `cargo test <anchor_name>` passing, plus `cargo test --all` showing no regressions.

Runtime mode additions and naming-policy changes require explicit confirmation before implementation or documentation. See ADR-007.
Runtime-core ratatui TUI behavior must conform to ADR-009, ADR-010, and ADR-011 before merge.
Tests that mutate process environment variables must hold `crate::test_support::ENV_LOCK`; `cargo test --all-targets` must pass without `--test-threads=1`.

See [`docs/dev/manifest-strategy.md`](docs/dev/manifest-strategy.md) for the operational guide.

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

Completed tasks move to `TASKS/COMPLETED/` — do not delete them.

---

## 🗺️ Active Roadmap

### REF track — Runtime seam (headless-first refactor)

> **Architectural decision:** [ADR-004](docs/adr/ADR-004-runtime-seam-headless-first.md)  
> **Goal:** Decouple the conversation loop from the terminal renderer so headless execution and a proper TUI become independent concerns.  
> **Scope rule during this track:** No new CLI flags, tools, or protocol changes.

| Task | Target | Status |
| :--- | :--- | :--- |
| REF-02 | Define `RuntimeEvent`, `RuntimeContext`, `RuntimeMode` trait | Planned |
| REF-03 | Implement `RuntimeMode` for existing stdout renderer | Planned |
| REF-04 | Map `crossterm::event::Event` → `RuntimeEvent` | Planned |
| REF-05 | Generic runtime loop replacing `App::run()` | Planned |
| REF-06 | Extract TUI mode as a second `RuntimeMode` implementor | Planned |

Each REF task has its own manifest in `TASKS/`. Do not work on REF-03 before REF-02's anchor test passes.

---

## 🚀 Quick Start

```bash
# 1. Install Rust (stable toolchain required)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"

# 2. Verify the environment
cargo test --all

# 3. Pick a task from TASKS/, read its manifest, identify the anchor test

# 4. Implement, then verify
cargo test test_crit_XX_anchor_name -- --nocapture

# 5. Confirm no regressions
cargo test --all
```

---

## 📁 Project Structure

```
aistar/
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
│   └── COMPLETED/
├── src/
│   ├── api/                       # HTTP client, stream parser, mock
│   ├── app/                       # Stdout renderer + App event loop
│   ├── config/                    # Environment variable loading
│   ├── edit_diff/                 # LCS-based diff renderer
│   ├── runtime/                   # (planned REF track) RuntimeMode trait
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
