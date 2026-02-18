# Contributing to aistar

## 🛠️ The Agentic Workflow (TDD Manifest)

We use a Test-Driven Manifest strategy for all bug fixes and features:

1. **Identify Task:** Check `TASKS/` for open items (`TASKS/completed/` is archive only).
2. **Anchor Test:** Every task must have a failing regression test in the codebase before work begins.
3. **Module Isolation:** Work should be confined to the file specified in the task manifest.
4. **Verification:** Success is defined as `cargo test` passing for the anchor.

See `docs/dev/manifest-strategy.md` for the full technical breakdown.

## 📋 Task Naming Convention

| Prefix | Type | Example |
|--------|------|---------|
| `CRIT-XX` | Critical bugs | `CRIT-02-serde-fix.md` |
| `CORE-XX` | Core runtime behavior | `CORE-01-sse-parser.md` |
| `FEAT-XX` | Feature requests | `FEAT-01-streaming-ui.md` |
| `REF-XX` | Refactoring tasks | `REF-01-error-handling.md` |
| `SEC-XX` | Security hardening | `SEC-01-path-security.md` |
| `DOC-XX` | Documentation tasks | `DOC-01-change-safety-policy.md` |

## 📦 Task Lifecycle

1. Create or pick an active task file in `TASKS/`.
2. Implement the change and run the task's anchor test.
3. Verify the task's definition of done is fully satisfied.
4. Move the task file to `TASKS/completed/` only after verification.
5. Keep `TASKS/` root reserved for active work.
6. Only manifests in `TASKS/` root are executable work; `TASKS/completed/` entries are historical references only.

## 🧭 File Naming Convention

- Rust source files/modules: `snake_case` (example: `src/tools/executor.rs`)
- Markdown docs (outside `TASKS/`): lowercase `kebab-case` (example: `docs/dev/manifest-strategy.md`)
- Task manifests in `TASKS/`: uppercase prefix + numeric ID + kebab description (example: `CRIT-01-protocol.md`)
- Avoid uppercase ad-hoc filenames (for example `COMMAND_TO_AGENT.txt`)
- Local scratch files should use a `scratch-*` prefix and stay untracked

## ✅ Change Confirmation Policy

- Do not introduce new CLI flags, runtime modes, or naming-convention policy changes without explicit user confirmation first.
- If a new flag or mode is proposed, ask for confirmation before implementation and before documenting it.
- Exceptions: bug fixes that do not change user-facing flags/modes or naming policy.

## 🔎 Repo-Wide Findings (February 18, 2026)

The following review items were tracked and addressed in this cycle:

1. **High** — Plain-text tagged tool fallback could execute unintended tools from assistant text (`src/state/conversation.rs`).
2. **High** — `search_files` fallback could follow symlinked paths outside workspace when `rg` is unavailable (`src/tools/executor.rs`).
3. **High** — TUI block-delta routing could append tool JSON deltas into assistant message text (`src/app/mod.rs`).
4. **Medium** — `Ctrl+Z` was bound to editor undo while in raw mode, overriding expected terminal suspend behavior (`src/app/mod.rs`).
5. **Medium** — Cursor/wrap logic used character counts instead of terminal display-width for wide glyphs (`src/app/mod.rs`, `src/ui/render.rs`).
6. **Medium** — Unknown tool names were treated as successful results instead of errors (`src/state/conversation.rs`).
7. **Medium** — `CONTRIBUTING.md` had stale architecture wording about crate wiring (`CONTRIBUTING.md`, `src/main.rs`, `src/lib.rs`).
8. **Medium** — Ratatui terminal lifecycle used alternate-screen behavior that hid copyable output from normal zsh scrollback (`src/terminal/mod.rs`).
9. **Low** — Progressive output could render separator-only lines (`====`, `----`) as noise instead of visual feedback (`src/app/mod.rs`).
10. **Medium** — Tool-preview dispatch logic was split across app/state layers with drift risk (`src/tool_preview.rs`, `src/app/mod.rs`, `src/state/conversation.rs`).
11. **Medium** — Ratatui panel borders (`┌ ─ ┐`) could create heavy terminal line noise (`src/ui/render.rs`).
12. **High** — Read/search loop guard could false-positive on repeated tool names even when queries changed, causing premature aborts (`src/state/conversation.rs`).
13. **High** — Consecutive read/search ceiling (`MAX_CONSECUTIVE_READ_ONLY_TOOL_ROUNDS`) could still false-positive during legitimate investigation runs and abort with `Detected too many consecutive read/search-only tool rounds` (`src/state/conversation.rs`).
14. **High** — Tool timeout wrapper could be ineffective when tool execution blocks inline; timeout must wrap a spawned task and return promptly on expiry (`src/state/conversation.rs`).
15. **High** — History pruning could retain `tool_result` user messages without their preceding assistant `tool_use`, causing protocol-invalid turns near message limits (`src/state/conversation.rs`).
16. **Medium** — Raw prompt editor mode could be enabled with terminal stdin but non-terminal stdout, leaking control sequences into redirected output (`src/app/mod.rs`).
17. **Medium** — `ReadFileSnapshotCache` stored full file content strings per path, causing unbounded memory growth proportional to file size and read frequency; `read_file_path` baked a presentation fallback (`"<missing>"`) into a shared utility layer, coupling domain and display concerns (`src/tool_preview.rs`, `src/app/mod.rs`, `src/state/conversation.rs`).

Related housekeeping completed:
- Removed stale, misleading document `aistar-v0.1.0-fully-corrected.md`.
- Reduced duplicate security-test maintenance by keeping traversal coverage in integration tests and removing redundant unit overlap.
- Ensure runtime modules (`src/runtime.rs`, `src/tool_preview.rs`) are treated as first-class tracked source files.
- **REF-01: Hash-based `ReadFileSnapshotCache` + `Option`-based `read_file_path`** (`src/tool_preview.rs`, `src/app/mod.rs`, `src/state/conversation.rs`) — replaced full-string cache entries with fixed-size `(u64 hash, usize chars, usize lines)` tuples; changed `read_file_path` to return `Option<String>` with fallback text owned by call sites. Steps must follow this order — each step is a compile gate for the next:

  **Step 1 — `src/tool_preview.rs` (do this first; steps 2 and 3 will not compile until this lands)**
  - `ReadFileSnapshotSummary::Changed`: rename fields `chars`→`after_chars`, `lines`→`after_lines` in the enum definition.
  - `ReadFileSnapshotCache.entries`: change storage from `HashMap<String, String>` to `HashMap<String, (u64, usize, usize)>`.
  - Add `fn hash_content(content: &str) -> u64` using `DefaultHasher` (non-deterministic across restarts; safe because the cache is per-process in-memory only).
  - Rewrite `ReadFileSnapshotCache::summarize()` to use hash tuples inline; remove the delegation to `summarize_read_file_snapshot`.
  - Remove `pub fn summarize_read_file_snapshot(cache: &mut HashMap<String, String>, ...)` — this function is deleted entirely; it had no callers outside `summarize()`.
  - `pub fn read_file_path(input: &Value) -> String` → `pub fn read_file_path(input: &Value) -> Option<String>`; remove the baked `"<missing>"` fallback.
  - Update both `Changed` arms in `format_read_file_snapshot_message` to bind `after_chars`/`after_lines`. **This arm is the undocumented compile break** — `format_read_file_snapshot_message` pattern-matches the enum directly and will fail to compile when the field names change if not updated in the same commit.
  - Update tests: replace `test_summarize_read_file_snapshot_states` (tests deleted function) with `test_snapshot_cache_states` (tests the struct method); add Changed→Unchanged fourth step; fix `test_read_file_path_extraction` for `Option` return; fix `Changed` field names in `test_format_read_file_snapshot_message_styles`.

  **Step 2 — `src/app/mod.rs` and `src/state/conversation.rs` (parallel; both depend on Step 1, not on each other)**
  - `src/app/mod.rs` line ~794: `read_file_path(&input)` → `read_file_path(&input).unwrap_or_else(|| "<missing>".to_string())`. No other changes — `EditorAction::Changed` is an unrelated enum and is not touched.
  - `src/state/conversation.rs` line ~628: `read_file_path(input)` → `read_file_path(input).unwrap_or_else(|| "<missing>".to_string())`. No pattern-match changes needed; the result is passed directly into `format_read_file_snapshot_message`.

  Run `cargo test --all-targets` and `cargo clippy --all-targets -- -W warnings` after Step 2 to confirm clean build.

  REF-01 status (verified on February 18, 2026):
  - [x] Step 1 `src/tool_preview.rs` complete.
  - [x] Step 2 `src/app/mod.rs` complete.
  - [x] Step 2 `src/state/conversation.rs` complete.

Explicit rendering scope (to avoid rework loops):
- `====` / `----` filtering applies only to assistant progressive response lines in `format_progressive_response_line` (`src/app/mod.rs`).
- Ratatui frame-border lines (`┌ ─ ┐`) were a separate issue in `src/ui/render.rs`; borders were removed from conversation/prompt panels and replaced with dimmed prompt-surface styling.

Explicit loop-guard scope (to avoid rework loops):
- The removed `"repeated read/search-only tool pattern"` check was name-only and could block legitimate iterative search.
- The follow-up false positive (`Detected too many consecutive read/search-only tool rounds`) is tracked as a separate bug from the old name-only matcher.
- Infinite-loop protection for read/search now relies on exact repeated signature detection plus global `AISTAR_MAX_TOOL_ROUNDS` bounds, rather than a hard consecutive read-only ceiling.

## 🧱 Bug-Fix Prioritization and TUI Migration Order

Use this order when planning major stabilization + ratatui integration work.

### Principle

- Fix blockers, crashers, and API-invalid states before big UI wiring.
- If reports show Bug #1 (tool approval) or Bug #5 (tool definitions) are still broken, move them to the very front.

### Phase 0 — Restore Core Functionality (tiny, highest leverage)

1. **Bug #1: Tool approval deadlock / silent cancellation (Critical)**
   - In streaming `App::run()`, handle `UiUpdate::ToolApprovalRequest(req)` explicitly.
   - Every path must send exactly one bool on `req.response_tx`.
   - Never swallow approval requests in catch-all arms.
2. **Bug #5: Missing tool definitions (capability blocker)**
   - Add missing tool schemas in all tool-definition producers (`tool_definitions()` and OpenAI-compat variants if present).
   - Tool definition names must match `execute_tool` dispatch names exactly.

Acceptance after Phase 0:
- Tools are both available and approvable.

Phase 0 status (verified on February 18, 2026):
- [x] Bug #1 tool approval handling complete.
- [x] Bug #5 tool definition completeness complete.
- [x] Bug #1 follow-up: `CancelNewTask` now auto-denies later approval prompts in the same turn.

### Phase 1 — Correctness Crashers and API-Invalid States

1. **Bug #2: UTF-8 panic in `append_incremental_suffix`**
   - Never slice on arbitrary byte indices.
   - Overlap math must use character-boundary-safe indices (for example `char_indices`).
2. **Bug #3: `prune_message_history` ordering**
   - After prune, first retained message must be `role == "user"` or list is empty.
   - Avoid orphaning tool/result structures.

Acceptance after Phase 1:
- No UTF-8 boundary panics in incremental text paths.
- History sent to API remains ordering-valid.

Phase 1 status (verified on February 18, 2026):
- [x] Bug #3 prune ordering correctness complete.

### Phase 2 — Ratatui Integration (wiring + architecture)

1. **Bug #4 / 6.1:** Declare `mod terminal; mod ui;` in crate roots so TUI code is compiled.
2. **6.2:** Add persistent TUI state to `App` (`terminal`, `input_buffer`, `cursor_position`, `messages`, `scroll_offset`, and `pending_tool_approval`).
3. **6.3:** `terminal::setup()` in `App::new()` and `terminal::restore()` in `Drop` (best-effort).
4. **6.7:** Add layout/import plumbing before loop rewrite.
5. **6.4:** Replace blocking loop with draw/event/update loop.
   - Draw frame.
   - Poll/read input.
   - Drain `update_rx` with non-blocking receives.
   - Exit on `should_quit`.
6. **6.5:** Key handler for insert/edit/move/send/quit/scroll.
7. **6.6:** UI update handler for `StreamDelta`, `TurnComplete`, `Error`.
8. **6.8:** Tool approval modal state (input locked to approval while pending).

Acceptance after Phase 2:
- No `stdin().read_line()` in ratatui runtime path.
- Streaming response is rendered as one growing assistant message.
- Tool approval always responds exactly once.

Phase 2 status (verified on February 18, 2026):
- [x] 2.1 Module wiring complete (`lib.rs` declares `terminal`/`ui`; `main.rs` consumes library modules directly).
- [x] 2.2 Persistent TUI state on `App` complete.
- [x] 2.3 Terminal setup/restore lifecycle complete (`App::new` + `Drop`).
- [x] 2.4/2.5/2.7 Draw/event/update ratatui loop complete.
- [x] 2.6 Key handling complete (edit/history/paste, Esc, Ctrl+C, send, scroll).
- [x] 2.8 Modal tool approval state complete (single-response guarantee while pending).

### Phase 3 — Optional Enhancements

- **6.9:** slash commands, history UX polish, multiline input improvements, status line, repo awareness widgets.

Phase 3 status (verified on February 18, 2026):
- [x] Slash commands in TUI (`/commands`, `/clear`, `/history`, `/repo`, `/ps`, `/quit`).
- [x] Input history navigation and inspection (`Up/Down`, `/history`).
- [x] Multi-line input in TUI (`Shift+Enter` or `Ctrl+J` inserts newline).
- [x] Live status line in TUI (mode, approval state, input mode, history, repo state).
- [x] Repo awareness widget integrated into status and `/repo`.

## 🧱 Phase 4 — Docs Deployment Contract

Use this phase after Phase 3 when implementing docs publishing.

### Principle

- Lock deployment prerequisites and naming rules before adding workflow files.
- Keep scope explicit to avoid another “fixed wrong thing” loop.

1. **DOC-01: Change-Safety Policy for Planning/Audit Requests**
   - Add explicit no-touch behavior: when user asks for plan/audit only, do not create/edit/delete files.
   - Require explicit confirmation before entering edit mode in the same session.
2. **DOC-02: GitHub Pages + mdBook Deployment Standard**
   - Document Pages preflight requirements (`GitHub Actions` source, permissions, branch policy).
   - Document required workflow permissions: `pages: write`, `id-token: write`.
   - Document docs structure requirements (`docs/book.toml`, `docs/src/SUMMARY.md`).
3. **CORE-02: mdBook Scaffold**
   - Add base docs pages and summary navigation.
   - Set project site path for Pages (`/aistar-rs/`) unless repo/domain decision says otherwise.
4. **CORE-03: Docs Workflow with Pinned Versions**
   - Add `.github/workflows/docs-build-and-deploy.yml`.
   - Build on PR; deploy only on `main` non-PR runs.
   - Pin toolchain/tool versions:
     - Rust `1.93.1`
     - mdBook `0.5.0`
     - terraform-docs `0.20.0`
     - helm-docs `1.14.2`
     - Node.js LTS `24.13.0` (optional, for OpenAPI tooling)
5. **CORE-04: IaC Auto-Doc Generation Pipeline**
   - Add generator script to run before `mdbook build`.
   - Generate Terraform/Helm/OpenAPI markdown pages into docs source tree.
   - Fail loudly on generator/tool errors; avoid silent empty pages.

Acceptance after Phase 4:
- Pages workflow builds docs on PR and deploys on `main` with minimal required permissions.
- `mdbook` site builds from canonical docs structure with pinned dependency versions.
- IaC docs are generated automatically and included in published output.

Phase 4 status (planned on February 18, 2026):
- [ ] DOC-01 change-safety policy documented.
- [ ] DOC-02 deployment standard documented.
- [ ] CORE-02 mdBook scaffold complete.
- [ ] CORE-03 docs workflow complete.
- [ ] CORE-04 IaC generator pipeline complete.

Explicit docs-deploy scope (to avoid rework loops):
- This phase covers docs publishing and generator wiring only; no unrelated application behavior changes.
- Custom domain setup is optional and deferred unless explicitly requested.
- Base path decision (`/aistar-rs/` vs `/`) must be locked before workflow merge.

## 🧱 Phase 5 — 3-Pane + Overlay TUI Roadmap (CONTRIBUTING-Style, Priority-First)

### Summary

- Adds a stable 3-pane terminal contract: header, history, input.
- Adds a modal overlay system: tool approval, command confirmation, patch approval, error modal.
- Gap-closure plan, not a rewrite: `run_tui`, base rendering, and tool-approval modal already exist.
- Focuses on layout/state contracts, overlay routing, and missing behaviors (diff overlay, focus policy, deterministic tests).
- Legacy mapping for archived/superseded manifests:
  - `CORE-05-ui-event-display-contract.md` is superseded by `CORE-10` + `FEAT-11`.
  - `CORE-06-transcript-determinism-regression-gates.md` is superseded by `FEAT-12` + Phase 5 regression gates.
  - Superseded manifests must live only in `TASKS/completed/` and must not be reintroduced in `TASKS/` root.

### Principle

- Fix core interaction correctness before adding UI enhancements.
- Keep TUI decisions explicit and scoped to avoid rework loops.
- Preserve protocol/runtime behavior; UI layer must not change API semantics.
- Module wiring (`terminal`/`ui`) is already complete; no additional crate-root exposure changes are required in this phase.

Decision Record (February 18, 2026):
- Do not add a new global `src/state.rs`; keep UI state in `src/app/mod.rs` because `src/state/` is the existing runtime module namespace.

### Track 0 — Core Correctness Prerequisites (Critical, must land first)

1. **CRIT-16: Raw editor gating with redirected stdout**
   - Ensure stream prompt raw mode is enabled only when both stdin and stdout are TTY in non-TUI path.
   - Align `stream_input_mode` checks with `App::new()` terminal gating.
   - File: `src/app/mod.rs`
2. **CRIT-15: Prune safety for tool_use/tool_result pairing**
   - Prevent prune from retaining orphaned user `tool_result` blocks without preceding assistant `tool_use`.
   - Keep first retained message user-role only when pairing constraints remain valid.
   - File: `src/state/conversation.rs`
3. **CRIT-14: Effective tool timeout**
   - Run blocking tool execution in cancellable spawned context so timeout can actually expire.
   - Return deterministic timeout error and keep turn state consistent.
   - File: `src/state/conversation.rs`

Acceptance after Track 0:

- Redirected stdout no longer gets interactive control sequences from raw prompt mode.
- Pruned history remains protocol-valid with tool pairs.
- Tool timeout is enforceable under blocking tool execution.

Track 0 status (verified on February 18, 2026):
- [x] CRIT-16 raw editor TTY gating complete — `sticky_prompt_enabled()` checks both `stdin` and `stdout` `is_terminal()` before enabling raw mode; all `enable_input_raw_mode()` call sites are gated.
- [x] CRIT-15 prune tool_use/tool_result pairing complete — `prune_message_history` walks forward until it finds a `role == "user"` message that is not a `tool_result` block before anchoring.
- [x] CRIT-14 effective tool timeout complete — `execute_tool_with_timeout` runs blocking execution inside `tokio::task::spawn_blocking`, wrapped by `tokio::time::timeout`; task is aborted on expiry.

### Track 1 — 3-Pane Layout Contract (Critical)

1. **CORE-07: Extract layout manager**
   - Add `src/ui/layout.rs` with a single canonical splitter:
     - Row 1: header/status (`Length(1)`)
     - Middle: history (`Min(1)`)
     - Bottom: input (`Length(dynamic_input_height)`)
   - Export helper used by `draw_tui_frame`.
   - Decision: keep dynamic input height (existing behavior), but contract the pane ordering and ownership.
2. **CORE-08: Unify frame composition order**
   - Always render in this order:
     1. Header
     2. History
     3. Input
     4. Overlay (if active)
   - Keep overlay draw last for z-order guarantee.
   - Files: `src/app/mod.rs`, `src/ui/render.rs`

Acceptance after Track 1:

- Layout split logic is centralized.
- Overlay no longer affects pane geometry or causes implicit reflow.

### Track 2 — Persistent State + Overlay Router (Critical)

1. **CORE-09: Formalize app UI state slices (without new src/state.rs)**
   - Decision: keep state in `App`/`app/mod.rs` to avoid conflict with existing `src/state/` runtime module tree.
   - Introduce clear internal structs/types:
     - `HistoryState` (messages + scroll)
     - `InputState` (buffer, cursor, multiline/history navigation metadata)
     - `OverlayState`/`OverlayKind` (tool permission, command confirm, patch approve, error)
   - Migrate existing ad-hoc fields into these grouped state holders.
2. **CORE-10: Event router hard split**
   - `Overlay::None` => input keymap/editing path.
   - `Overlay::Some(_)` => overlay keymap only (`y`/`n`/`enter`/`esc`, plus modal-local scroll keys where relevant).
   - Enforce focus policy: input cursor hidden while overlay active.
3. **CORE-11: UiUpdate-to-overlay mapping**
   - Map `ToolApprovalRequest` to modal with captured response sender and preview.
   - Map `UiUpdate::Error` to overlay or history according to explicit policy:
     - Default decision: tool/approval errors modal, stream/runtime errors history line.
   - Add explicit “exactly once response” guard in overlay dismissal path.

Acceptance after Track 2:

- Input cannot submit while overlay active.
- Overlay decision always resolves exactly once.
- State model is explicit and testable.

### Track 3 — Rendering Completeness (High)

1. **FEAT-10: Header/status renderer contract**
   - Keep top row reserved and stable.
   - Show mode, approval state, history depth, repo status as current compact status line.
2. **FEAT-11: Overlay renderer family**
   - Expand from tool-approval-only to shared modal renderer:
     - Confirm command
     - Approve patch
     - Tool permission
     - Error
   - Use centered modal + Clear + bounded body text + footer shortcuts.
3. **FEAT-12: Diff overlay viewer**
   - Add scrollable diff in modal with line colorization (+/-/context).
   - Overlay-local Up/Down/PageUp/PageDown.
   - `y`/`n` approval preserved.

Acceptance after Track 3:

- All overlay classes render through one shared contract.
- Diff modal is readable and scrollable at small terminal sizes.

### Track 4 — UX Enhancements (Medium)

1. **FEAT-13: Multiline input contract**
   - `Shift+Enter` / `Ctrl+J` insert newline.
   - Enter submits (single-shot) outside overlay.
   - Input pane expands up to max rows then internally scrolls.
2. **FEAT-14: Prompt history improvements**
   - Stable history traversal with stash/restore behavior.
   - No history mutation while overlay active.

Acceptance after Track 4:

- Multiline and history behavior remain deterministic with overlays and streaming turns.

Phase 5 status (updated February 18, 2026):
- [x] Track 0 complete (`CRIT-16`, `CRIT-15`, `CRIT-14`)
- [ ] Track 1 complete (`CORE-07`, `CORE-08`)
- [ ] Track 2 complete (`CORE-09`, `CORE-10`, `CORE-11`)
- [ ] Track 3 complete (`FEAT-10`, `FEAT-11`, `FEAT-12`)
- [ ] Track 4 complete (`FEAT-13`, `FEAT-14`)

Acceptance after Phase 5:
- 3-pane rendering order is stable and centralized.
- Overlay routing is modal-correct, blocks normal input while active, and resolves decisions exactly once.
- Streaming history and pinned input remain deterministic under tool/error/approval states.
- No Phase 5 work changes external CLI/API protocol semantics.

### Public Interfaces / Types Changes

1. Add `src/ui/layout.rs` public helper(s):
   - `split_main(area, input_height) -> [Rect; 3]` (or equivalent typed struct)
2. Add explicit overlay enum family in `src/app/mod.rs`:
   - `OverlayKind::{ToolPermission, ConfirmCommand, ApprovePatch, Error}`
3. Add grouped UI state structs in `src/app/mod.rs`:
   - `HistoryState`, `InputState`, `OverlayState`
4. No external CLI/API protocol changes.
5. No new env vars in this phase (explicitly out of scope).

### Test Plan and Scenarios

1. Unit tests
   - Layout splitter always returns 3 panes with fixed order.
   - Input cursor movement remains UTF-8 boundary safe.
   - Overlay router blocks input submit while active.
   - Overlay decision sends exactly one response.
2. Integration tests
   - Tool approval event opens modal and `y`/`n` resolves sender.
   - Turn streaming continues rendering history while input pane remains pinned.
   - Redirected stdout path contains no raw prompt control-sequence leakage.
   - Prune under high message count preserves valid `tool_use`/`tool_result` pairing.
3. Snapshot/UI tests
   - Golden frame snapshots for:
     - Base 3-pane
     - Tool permission modal
     - Diff approval modal
     - Error modal

### Explicit Scope Notes (to avoid “fixed wrong thing” loops)

1. This phase targets TUI composition and routing, not model/tool protocol redesign.
2. Do not introduce new CLI flags or runtime modes unless explicitly approved.
3. Deviation from external roadmap: do not add a new global `src/state.rs`; keep UI state local to app because `src/state/` is already the runtime module namespace.
4. Overlay visual polish is secondary to routing correctness and one-shot approval guarantees.

### Assumptions and Defaults

1. Keep existing ratatui path as primary interactive UI path.
2. Keep current key conventions (`y`/`n`, `1`/`2`/`3`, `Esc`) unless user requests a simplification.
3. Keep status line compact (single row) and avoid additional panes.
4. Prioritize reviewer-found correctness regressions (timeouts, prune validity, stdout TTY gating) before new UI features.

### Cursor Correctness Requirement

- Do not treat cursor position in `String` as arbitrary bytes.
- Use char-index/grapheme-aware cursoring, or clamp byte indices to valid char boundaries.
- This rule applies to both streaming editors and TUI editors to avoid reintroducing UTF-8 boundary bugs.
- Implementation choice: **Option B**. Cursor stays as byte index internally, but every edit/move path clamps to the nearest valid `is_char_boundary` before slicing or mutation.

### Condensed Required Sequence

1. Phase 5 / Track 0 critical gate: `CRIT-16` -> `CRIT-15` -> `CRIT-14`.
2. Phase 5 / Track 1 layout contract: `CORE-07`, `CORE-08`.
3. Phase 5 / Track 2 state + overlay routing: `CORE-09`, `CORE-10`, `CORE-11`.
4. Phase 5 / Track 3 rendering completeness: `FEAT-10`, `FEAT-11`, `FEAT-12`.
5. Phase 5 / Track 4 UX enhancements: `FEAT-13`, `FEAT-14`.

## 🚀 Quick Start

```bash
# 1. Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"

# 2. Run tests to verify environment
cargo test

# 3. Pick an active task from TASKS/ (not TASKS/completed/)
# Read the task file and implement the fix

# 4. Run the specific anchor test
cargo test <anchor_test_name> -- --nocapture

# 5. Iterate until the test passes and definition-of-done is satisfied
# 6. Move the completed task manifest into TASKS/completed/
```

## 📁 Project Structure

```
aistar/
├── CONTRIBUTING.md          # This file
├── TASKS/                   # Active task manifests (root only)
│   ├── <PREFIX>-XX-*.md    # Individual active task files
│   └── completed/          # Verified completed task manifests
│       └── <PREFIX>-XX-*.md
├── docs/
│   └── dev/
│       └── manifest-strategy.md  # TDD Manifest deep-dive
├── src/
│   ├── api/                # API client code
│   ├── app/                # Application state
│   ├── state/              # Conversation management
│   ├── terminal/           # Terminal setup
│   ├── tools/              # Tool execution
│   ├── types/              # Type definitions
│   └── ui/                 # UI rendering
└── tests/                  # Integration tests
```

## 🔗 Useful Links

- [Agentic Repair Strategy](docs/dev/manifest-strategy.md)
