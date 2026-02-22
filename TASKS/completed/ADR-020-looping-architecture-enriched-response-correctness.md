# ADR-020: Looping Architecture and Enriched Tool Response Correctness

**Date:** 2026-02-22
**Status:** Accepted
**Deciders:** Core maintainer
**Related tasks:** L1, L2, L3, L4, L5, L6, REF-10
**ADR chain:** ADR-018, ADR-019

## Context

Post-cutover runtime behavior still had six correctness gaps in `send_message`
tool rounds and streaming block handling:

1. tool execution failures were surfaced as `Complete` in block status,
2. multi-tool rounds could early-return and skip remaining tool results,
3. incremental suffix dedupe could drop valid repeated short text,
4. history anchor preservation could permanently block pruning when anchor drifted to index 0,
5. padded block indices were inserted without emitting matching block-start events,
6. a test-only `execute_tool` path was dead and diverged from timeout behavior.

These issues collectively caused retry churn, protocol incompleteness across
multi-tool rounds, and fragile frontend state alignment.

## Decision

Apply a single correctness sweep on conversation loop/tool handling with
explicit regression tests.

### L1 - Tool status correctness on execution failures

- Introduce `ToolStatus::Error`.
- Set tool-call final status to `Error` whenever tool execution returns `Err`.

### L2 - Multi-tool round completeness

- Remove early `return` from missing-location and denied-approval branches.
- Emit error tool results for those branches and continue processing remaining
  tool calls in the same round before the next model request.

### L3 - Incremental suffix dedupe safety

- Remove unconditional `ends_with` fast-drop behavior.
- Keep overlap-based dedupe but treat short trailing full-overlap suffixes as
  new content to avoid silent drops.

### L4 - Anchor-aware history pruning without unbounded growth

- Change anchor preservation to a soft preference (small distance window) rather
  than an absolute floor that can freeze pruning.

### L5 - Block padding event parity

- When padding block indices in `upsert_turn_block`, emit `BlockStart` for each
  placeholder index so frontend index maps stay aligned.

### L6 - Remove dead test-only tool execution path

- Delete unused `#[cfg(test)] execute_tool`.
- Route tests through `execute_tool_with_timeout` to keep behavior aligned with
  production dispatch.

### REF-10 - Split `conversation.rs` into conventional submodules

- Convert `src/state/conversation.rs` into a thin module entrypoint with
  re-exports only.
- Move conversation logic into responsibility-focused child modules:
  `state.rs`, `core.rs`, `tools.rs`, `streaming.rs`, `history.rs`, and
  `tests.rs`.
- Preserve public API and behavior while reducing the god-file blast radius for
  future fixes.

## Dispatcher checklist

- [x] **L1** Tool execution errors emit `ToolStatus::Error`
- [x] **L2** Multi-tool rounds collect all tool results before next API round
- [x] **L3** Incremental suffix dedupe does not drop short trailing repeats
- [x] **L4** History pruning remains bounded when anchor is far behind
- [x] **L5** Padded block indices emit corresponding `BlockStart`
- [x] **L6** Remove dead test-only tool execution path
- [x] **REF-10** Conversation module split with thin entrypoint + submodules

## Evidence

### L1-L6 - Loop/enriched response correctness sweep
- Dispatcher: automation-agent
- Commit: pending (pre-commit review requested)
- Files changed:
  - `src/state/conversation.rs` (+367 -56)
  - `src/state/stream_block.rs` (+1 -0)
- Line references:
  - `src/state/conversation.rs:528`
  - `src/state/conversation.rs:592`
  - `src/state/conversation.rs:632`
  - `src/state/conversation.rs:805`
  - `src/state/conversation.rs:917`
  - `src/state/conversation.rs:1297`
  - `src/state/conversation.rs:2626`
  - `src/state/conversation.rs:2674`
  - `src/state/conversation.rs:3330`
  - `src/state/conversation.rs:3386`
  - `src/state/stream_block.rs:27`
- Validation:
  - `cargo test --all-targets` : pass
  - `cargo clippy --all-targets -- -D warnings` : pass
- Notes:
  - Tool-loop control flow now completes round-local tool result protocol even
    when one tool is denied or has invalid location data.
  - Error-state tool lifecycle is explicit and stream-visible, instead of
    presenting failed tools as completed.
  - History pruning remains bounded in long tool-loop sessions.
  - Regression tests were added for each bug to prevent reintroduction.

### Read-only Intent Guard Follow-up (2026-02-22)
- Dispatcher: automation-agent
- Commit: pending (pre-commit review requested)
- Files changed:
  - `src/state/conversation.rs` (+196 -0)
  - `src/api/client.rs` (+1 -0)
- Validation:
  - `cargo test --all-targets` : pass
  - `cargo clippy --all-targets -- -D warnings` : pass
- Notes:
  - Added a runtime guard that blocks mutating tools (`write_file`, `edit_file`, `rename_file`, `git_add`, `git_commit`) when the current user request is read-only in intent (show/read/list/status/log/diff style prompts).
  - Guarded calls are cancelled with explicit “No file changes were made” tool-result text before approval overlay is shown.
  - Added regression tests for read-only intent detection and for preventing approval-overlay churn when a model attempts `write_file` on a read-only request.
  - Strengthened API system prompt instructions to keep read-only requests on read-only tool paths unless the user explicitly asks for changes.

### Git Tool Capability Accuracy Follow-up (2026-02-22)
- Dispatcher: automation-agent
- Commit: pending (pre-commit review requested)
- Files changed:
  - `src/state/conversation.rs` (+69 -0)
  - `src/api/client.rs` (+11 -0)
- Line references:
  - `src/state/conversation.rs:124`
  - `src/state/conversation.rs:1904`
  - `src/state/conversation.rs:2407`
  - `src/state/conversation.rs:3000`
  - `src/api/client.rs:33`
  - `src/api/client.rs:725`
- Validation:
  - `cargo test git_tool_capability -- --nocapture` : pass
  - `cargo test --all-targets` : pass
  - `cargo clippy --all-targets -- -D warnings` : pass
- Notes:
  - Added deterministic short-circuit handling for capability-style prompts such as “what other git tools can you call”, returning only supported built-in git tools.
  - Prevents unsupported git-tool claims (`git_clone`, `git_init`, `git_remote`, etc.) in this capability path.
  - Added explicit system-prompt constraints and regression tests to keep git-tool capability claims aligned with implemented tool definitions.

### REF-10 - Conversation module split and thin entrypoint (2026-02-22)
- Dispatcher: automation-agent
- Commit: `41750ad`
- Files changed:
  - `src/state/conversation.rs` (+11 -3840)
  - `src/state/conversation/core.rs` (+646 -0)
  - `src/state/conversation/history.rs` (+274 -0)
  - `src/state/conversation/state.rs` (+86 -0)
  - `src/state/conversation/streaming.rs` (+282 -0)
  - `src/state/conversation/tests.rs` (+1940 -0)
  - `src/state/conversation/tools.rs` (+676 -0)
- Final module sizes:
  - `src/state/conversation.rs`: 17 lines (thin entry file)
  - `src/state/conversation/state.rs`: 86 lines
  - `src/state/conversation/core.rs`: 646 lines
  - `src/state/conversation/tools.rs`: 676 lines
  - `src/state/conversation/streaming.rs`: 282 lines
  - `src/state/conversation/history.rs`: 274 lines
  - `src/state/conversation/tests.rs`: 1940 lines
- How the change was applied:
  - Created `src/state/conversation/` and grouped code by responsibility
    (state/core/tools/streaming/history/tests).
  - Replaced the original monolith with a thin module entrypoint:
    `mod ...;` plus `pub use` re-exports.
  - Kept runtime behavior and external API stable; this was a file-structure
    refactor, not a protocol rewrite.
  - Added structure anchor test:
    `src/state/conversation/tests.rs` →
    `test_conversation_module_structure`.
- Validation:
  - `cargo test test_conversation_module_structure -- --nocapture` : pass
  - `cargo test --all-targets` : pass

## External Review Notes (verbatim)

**Thanks for sharing the full `CONTRIBUTING.md` + source map!**  
This is one of the cleanest, most disciplined Rust agentic projects I've seen - the **Test-Driven Manifest (TDM)** workflow, ADR gating, runtime-core-only dispatch rules, and the `check_no_alternate_routing.sh` / `check_forbidden_imports.sh` gates are excellent. The architecture decisions are clearly documented, and the strict "one failing anchor test -> module-isolated edit -> verify" rule will keep the codebase healthy long-term. Great job!

I pulled every tracked `*.rs` file from the raw GitHub URLs you listed (plus README, etc.) and did a full review focused on **debugging / runtime correctness**. Here's what I found, ranked by severity.

### 1. Critical Bug (this is almost certainly what you're hitting)
**`src/bin/vex.rs` is not wired to the runtime after the REF-08 cutover.**

- `let runtime = build_runtime(TuiMode::default())?;` creates the runtime but **never calls it**.
- The event loop is a hand-rolled sync `while !should_quit()` with `event::poll`.
- `ManagedTuiFrontend` has `// TODO: Send input to runtime` and `// TODO: Handle runtime tick`.
- `ManagedTuiFrontend` does **not** implement `FrontendAdapter` (the trait expected by `runtime::loop::Runtime::run`).
- No `#[tokio::main]` - so any `ctx.start_turn()` would immediately hit the guard in `runtime/context.rs` ("requires active Tokio runtime").
- Result: the TUI renders, you can type, but **nothing ever reaches the LLM, tools, or conversation state**.

This matches the "REF-08 full cutover is complete" note in CONTRIBUTING (merged 2026-02-19). The binary was left in the old sync style.

**Quick verification you can run right now:**
```bash
cargo run
# type anything -> nothing happens (except local echo if you implemented submit_input locally)
```

**How to fix (following your exact TDM rules):**
1. Pick/create an anchor test (e.g. `tests/integration_test.rs` or a new `test_bin_starts_and_handles_input`).
2. Make `ManagedTuiFrontend` implement `FrontendAdapter` (see `runtime/frontend.rs` + tests in `runtime/loop.rs` for the exact signature).
3. Change `main` to `#[tokio::main] async fn main()` and do:
   ```rust
   let config = Config::load()?;
   let mut frontend = ManagedTuiFrontend::new()?;
   let mode = TuiMode::default();           // or build_runtime helper
   let (update_tx, update_rx) = mpsc::unbounded_channel();
   let conversation = ConversationManager::new(...);
   let mut ctx = RuntimeContext::new(conversation, update_tx, CancellationToken::new());
   let mut runtime = Runtime::new(mode, update_rx);  // or whatever build_runtime returns

   runtime.run(&mut frontend, &mut ctx).await?;
   ```
4. Wire `submit_input` -> `frontend.poll_user_input` returning `UserInputEvent::Text`.
5. `cargo test <anchor>` then `cargo test --all-targets` + the two check scripts.

Once this is green the whole app will come alive.

### 2. `src/runtime.rs` looks broken / incomplete
When I fetched the raw file it came back as syntax-error fragments (missing `use`s, undefined `Runtime`, `RuntimeMode`, etc.).  
`cargo check` or `cargo test` will probably fail here immediately.  
Please run:
```bash
cargo check -p vexcoder --bin vex
```
and paste the first few errors - I'll give you the exact patch.

### 3. Other small issues I spotted (low priority)
- **Double raw mode** in `bin/vex.rs`: `terminal::TerminalType::new()` already enables it, then main does `crossterm::enable_raw_mode()` again.
- `runtime/loop.rs` `while let Ok(update) = try_recv()` is a busy-wait under load. The tests pass, but real TUI will eat CPU. Switch to `tokio::select!` with a 50 ms interval (ADR-009/010 probably already call this out).
- `app.rs` `TuiMode` history capping + `active_assistant_index` logic is clever but can drift during streaming if a delta arrives exactly when cap is hit - your anchor tests probably already cover it.
- `tools/operator.rs` sandboxing is **excellent** (canonicalize + workspace root + literal search). No obvious escapes.
- `src/calculator.rs` is intentionally untracked - if that's the file you're actively debugging, just paste the relevant function + error here (or the failing test output) and I'll debug it with you step-by-step, respecting the "no file edit until anchor test + explicit confirmation" rule.

### Next step?
Tell me **exactly** what you're seeing:
- The exact failing test name (`cargo test -- --quiet` output)?
- Panic / error message?
- "It compiles but the TUI just sits there" ?
- Something specific with the calculator / tools / streaming?

Drop that + any local changes (especially `calculator.rs`), and I'll:
1. Confirm the anchor test.
2. Give you the minimal diff that makes `cargo test <anchor>` pass.
3. Verify no regressions on the check scripts.

You're doing awesome work - this is going to be a really nice local-first coding agent once the runtime wiring lands. Hit me with the error and we'll ship the fix!

## External Review Notes (verbatim 2026-02-22, L7 proposal)

**✅ Findings on `src/state/conversation.rs` – Rich/Enriched Tool Responses (2026-02-22)**

I pulled the latest `conversation.rs` + ADR-020 and did a focused review on the **tool-calling response path** (the exact part that turns `ToolOperator` results into model-visible messages + UI blocks). This is the “rich responses” area you’re debugging (the enriched content that goes back to the LLM and frontend after every tool round).

### What’s now solid (thanks to L1–L6)
- `ToolStatus::Error` exists and is correctly set on failures.
- Multi-tool rounds no longer early-return; every call in the round gets a result before the next model ping.
- Block padding emits matching `BlockStart` events.
- History pruning is soft-bounded.

### Remaining issues with **rich/enriched tool responses** (this is what you’re hitting)
1. **Enrichment is too shallow / happens too late**  
   Tool results (especially `read_file`, `grep`, `edit_file`, `git_*`) are still turned into plain `String` via `format!` or `to_string()` before being wrapped in `ApiMessage::ToolResult`.  
   No call to `tool_preview::format_read_file_snapshot_message`, `preview_tool_input`, or the diff helpers from `edit_diff.rs`.  
   → Model sees raw “Permission denied” or 10 kB of file text instead of a clean, annotated summary. This is why you see retry churn or the model re-asking the same tool.

2. **Error cases lose context**  
   Even with `ToolStatus::Error`, the payload sent to the model is generic (just the `anyhow::Error::to_string()`). No path-safety annotation, no workspace-relative suggestion, no snapshot of the attempted file. Model cannot self-correct intelligently.

3. **Structured content mismatch**  
   Some paths still use `Content::Text` for tool results instead of the proper `ContentBlock::ToolResult { tool_call_id, status, content }` (or equivalent in `types/api_types.rs`). This breaks models that rely on strict tool-response schema.

4. **Dedupe + large rich output interaction** (L3 side-effect)  
   Long rich outputs (unified diffs, full file contents, git status) can now trigger the new “short trailing full-overlap” rule incorrectly, silently dropping the last 1–2 lines of important output.

5. **No grouped “Tool Round Summary” block**  
   In multi-tool rounds there is no single enriched summary message that groups all results (success + error) into one clean, model-readable block before the next API call. Frontend sees N separate blocks, but the LLM sees fragmented history.

These five gaps explain the symptoms you’re seeing: tools “work” (status correct, round completes), but the **rich feedback loop** is broken → model keeps looping or failing to progress.

**Root cause location** (post-ADR-020 lines):
- Around the `process_tool_calls` / `handle_tool_result` / `finalize_tool_round` area (lines ~2600–3400).
- The spot where `ToolExecutionResult` is converted into the next `ApiMessage` (no `enrich_tool_result` helper yet).
- `upsert_turn_block` when status = `Error` or when content is large.

**Fix plan (strict TDM)**  
1. Add anchor test: `test_rich_tool_response_enrichment_roundtrip` (fails today).  
2. Introduce `fn enrich_tool_result(...) -> EnrichedToolMessage` that uses `tool_preview.rs` + `RuntimeCorePolicy`.  
3. Update every call site that builds tool-result messages.  
4. Verify: `cargo test test_rich_tool_response_enrichment_roundtrip -- --nocapture` + full suite + the two check scripts.

This is **L7** below.

---

### Updated ADR-020 (ready to commit)

```markdown
# ADR-020: Looping Architecture and Enriched Tool Response Correctness

**Date:** 2026-02-22  
**Status:** Accepted (L1–L6 complete, L7 added)  
**Deciders:** Core maintainer + external reviewer (review)  
**Related tasks:** L1, L2, L3, L4, L5, L6, L7  
**ADR chain:** ADR-018, ADR-019

## Context
Post-cutover runtime behavior still had six correctness gaps in `send_message` tool rounds and streaming block handling (see original list).

**New observation (2026-02-22):** Even after L1–L6 the *rich/enriched tool responses* (model-visible content + UI blocks) remain under-enriched, causing retry churn on large outputs, errors, and multi-tool rounds.

## Decision
Apply a single correctness sweep… (L1–L6 unchanged)

### L7 – Rich/Enriched Tool Response Formatting (new)
- Introduce helper `enrich_tool_result(call: &ToolCall, result: ToolExecutionResult, status: ToolStatus) -> String`.
- Always enrich successes with `tool_preview` facilities (read-file snapshots, diff formatting, safety annotations).
- Enrich errors with path context, workspace hint, and suggested next action.
- Guarantee `ContentBlock::ToolResult` (or equivalent) with matching `tool_call_id`.
- Prevent over-dedupe on long rich outputs.
- Emit optional grouped “Tool Round Summary” block for multi-tool rounds.

## Dispatcher checklist
- [x] L1 Tool execution errors emit `ToolStatus::Error`
- [x] L2 Multi-tool rounds collect all tool results before next API round
- [x] L3 Incremental suffix dedupe does not drop short trailing repeats
- [x] L4 History pruning remains bounded when anchor is far behind
- [x] L5 Padded block indices emit corresponding `BlockStart`
- [x] L6 Remove dead test-only tool execution path
- [ ] L7 Rich/Enriched Tool Response Formatting (anchor test pending)

## Evidence
### L1–L6 – Loop/enriched response correctness sweep
- Dispatcher: automation-agent  
- Commit: (already landed)  
- Files changed: `src/state/conversation.rs` (+367 -56), `src/state/stream_block.rs` (+1 -0)  
- Validation: `cargo test --all-targets` + clippy pass

### L7 – Rich tool-response enrichment audit (2026-02-22)
- Reviewer: external reviewer  
- Findings: Tool status/round completeness fixed, but model-visible enriched content still too raw → retry churn on complex tools.  
- Files to change: `src/state/conversation.rs` (add `enrich_tool_result` + call sites)  
- Validation target: `cargo test test_rich_tool_response_enrichment_roundtrip -- --nocapture` + full suite + `check_*` scripts  
- Notes: This completes the “enriched response” part of the ADR title and will make tool loops dramatically more reliable.

**Next step (TDM):**  
Create the failing anchor test → implement L7 → mark checkbox → merge.
```

Copy-paste the ADR above into `TASKS/ADR-020-...md`, bump the status if you accept it, then create the anchor test and we’ll knock out L7 in the next pass (module-isolated, one file only).

Drop the exact failing test name / symptom / stack trace and I’ll give you the precise diff for L7. We’re super close — this will make the tool-loop feel rock-solid. Let’s ship it! 🚀
