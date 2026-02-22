# vexcoder


Terminal-first coding assistant with streaming responses, tool execution, and ratatui UI.

## Quick Start

```bash
cargo run
```

## API Endpoint Configuration

Set `ANTHROPIC_API_URL` to the protocol-specific endpoint. `VEX_API_PROTOCOL`
can be set explicitly, or omitted and inferred from the URL.

| Protocol | `VEX_API_PROTOCOL` | `ANTHROPIC_API_URL` endpoint |
|---|---|---|
| Anthropic Messages | `anthropic` | `.../v1/messages` |
| OpenAI Chat Completions | `openai` | `.../v1/chat/completions` |

Remote endpoints require `ANTHROPIC_API_KEY`. Localhost endpoints do not.
Structured tool protocol defaults:

- Remote endpoints: enabled (`VEX_STRUCTURED_TOOL_PROTOCOL=on`)
- Local endpoints: disabled by default (text-protocol fallback)
- Override explicitly with `VEX_STRUCTURED_TOOL_PROTOCOL=on|off`

Anthropic example:

```bash
ANTHROPIC_API_URL=https://api.anthropic.com/v1/messages \
VEX_API_PROTOCOL=anthropic \
ANTHROPIC_API_KEY=your_key \
cargo run
```

OpenAI example:

```bash
ANTHROPIC_API_URL=https://api.openai.com/v1/chat/completions \
VEX_API_PROTOCOL=openai \
ANTHROPIC_API_KEY=your_key \
cargo run
```

## Built-in TUI Commands

- `/commands` or `/help`
- `/clear`
- `/history`
- `/repo`
- `/ps`
- `/quit`

## Documentation

This repository uses mdBook + GitHub Pages for documentation.

- Config: `docs/book.toml`
- Pages: `docs/src/`
- Build locally: `mdbook build docs`

ADR files are stored under `TASKS/`, not under `docs/`.

Repository-wide raw URL map (all tracked files):

- `TASKS/completed/REPO-RAW-URL-MAP.md`

## Rust Source Map (`*.rs`)

Tracked Rust files in this repository (`git ls-files '*.rs'`):

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
| `src/state/conversation.rs` | Conversation module entrypoint and re-exports for split conversation submodules. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/state/conversation.rs> |
| `src/state/conversation/core.rs` | Main conversation turn loop, streaming event processing, and model/tool round orchestration. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/state/conversation/core.rs> |
| `src/state/conversation/history.rs` | Message history pruning, truncation, and read-file result summarization helpers. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/state/conversation/history.rs> |
| `src/state/conversation/state.rs` | Conversation state types and `ConversationManager` constructors/accessors. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/state/conversation/state.rs> |
| `src/state/conversation/streaming.rs` | Stream block lifecycle helpers, block promotion, and delta emission utilities. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/state/conversation/streaming.rs> |
| `src/state/conversation/tests.rs` | Conversation module tests covering protocol flow, loop guards, and regression anchors. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/state/conversation/tests.rs> |
| `src/state/conversation/tools.rs` | Tool execution dispatch, approval gating, input parsing, and tool-loop guard helpers. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/state/conversation/tools.rs> |
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

`src/calculator.rs` is untracked locally and excluded from the tracked repo source map.
