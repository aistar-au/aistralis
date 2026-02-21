# Task CRIT-17: TUI Stream Leak Sanitization

**Target File:** `src/api/client.rs`, `src/app/mod.rs`, `src/state/conversation.rs`

**ADR:** ADR-013 (TUI completion/deployment), ADR-009 §1 (user-visible interaction contract), ADR-010 §2–§3 (viewport/transcript behavior)

**Depends on:** CORE-09/CORE-10/CORE-11/FEAT-16 baseline behavior (typed interrupt routing, overlay/input guards, structured TUI history flow)

---

## Issue

Two protocol-level leaks were still visible in the TUI history pane:

1. **Debug payload leak into interactive terminal output**
   `VEX_DEBUG_PAYLOAD=1` printed full request JSON to `stderr` while the ratatui
   UI was active, which contaminated the visible transcript/scrollback area and made
   the three-pane experience unreadable during streaming.

2. **Tagged fallback tool markup leak into assistant history**
   Local fallback tool syntax (for example `<function=...>` / `<parameter=...>`)
   could appear in streamed assistant text and persisted history, surfacing protocol
   internals to users instead of clean assistant output.

This violated the UI contract implied by ADR-013 and the user-visible interaction
discipline in ADR-009.

---

## Decision

1. **Route debug payload logging away from live TUI stderr noise**
   - Keep `VEX_DEBUG_PAYLOAD` behavior enabled.
   - When `stderr` is a terminal, append debug payload output to a file path
     (`VEX_DEBUG_PAYLOAD_PATH`, default `/tmp/vex-debug-payload.log`) instead
     of printing multi-line JSON into the active UI.
   - Preserve direct `stderr` logging when output is non-interactive (non-TTY).

2. **Sanitize tagged tool markup in streamed UI history**
   - In `TuiMode::on_model_update(UiUpdate::StreamDelta(...))`, sanitize the active
     assistant line after each append to remove complete tagged tool blocks and hide
     incomplete tag suffix fragments.

3. **Sanitize tagged tool markup before assistant history persistence**
   - In `ConversationManager::send_message`, when local tagged fallback parsing is
     used, store/return sanitized assistant text for history-facing content while
     preserving tool execution behavior.

4. **Do not weaken fallback tool execution**
   - Keep tagged fallback parsing/execution active.
   - Keep structured tool block emission behavior unchanged for runtime semantics.

---

## Definition of Done

1. `VEX_DEBUG_PAYLOAD=1` no longer floods interactive TUI panes with raw JSON.
2. `<function=...>` / `<parameter=...>` syntax is not shown in assistant history
   lines during local fallback streaming.
3. Sanitization removes incomplete tag suffix fragments during incremental deltas.
4. Existing fallback execution and approval/overlay behavior remain intact.
5. `cargo clippy --all-targets -- -D warnings` is green.
6. `cargo test --all-targets` is green.

---

## Anchor Verification

- `test_stream_delta_strips_tagged_tool_markup_from_history`
- `test_stream_delta_hides_incomplete_tool_tag_suffix`
- `test_strip_tagged_tool_markup_removes_function_blocks`
- `test_strip_tagged_tool_markup_drops_incomplete_suffix`
- `test_text_tagged_tool_call_executes_as_fallback_for_local_endpoint`
- `test_text_tagged_tool_call_emits_structured_tool_blocks_for_fallback`

**What NOT to do:**
- Do not disable fallback parsing/execution for local models.
- Do not remove debug payload support entirely.
- Do not change pane geometry or overlay z-order for this fix.
- Do not add alternate routing paths outside runtime-core dispatch.
