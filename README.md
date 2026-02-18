# aistar-rs
<img width="973" height="721" alt="image" src="https://github.com/user-attachments/assets/260cda51-aa28-45ec-9955-0c351742043b" />

Terminal-first coding assistant with streaming responses, tool execution, and ratatui UI.

## Quick Start

```bash
cargo run
```

Local endpoint example:

```bash
ANTHROPIC_API_URL=http://localhost:8000/v1/messages \
ANTHROPIC_MODEL=local/llama.cpp \
cargo run
```

## Required Runtime Configuration

| Variable | Required | Default | Notes |
|---|---|---|---|
| `ANTHROPIC_API_URL` | No | `https://api.anthropic.com/v1/messages` | Must be `http://` or `https://`. |
| `ANTHROPIC_API_KEY` | Remote only | none | Required when URL is not local (`localhost`, `127.0.0.1`, `0.0.0.0`). |
| `ANTHROPIC_MODEL` | No | `claude-sonnet-4-5-20250929` | For local endpoints, `local/*` is allowed. |
| `ANTHROPIC_VERSION` | No | `2023-06-01` | Sent on Anthropic protocol requests. |

## Runtime Controls

### API / Protocol

| Variable | Default | Behavior |
|---|---|---|
| `AISTAR_API_PROTOCOL` | inferred from URL | `anthropic` or `openai` protocol override. |
| `AISTAR_STRUCTURED_TOOL_PROTOCOL` | `true` | Enables structured tool schema payloads. |
| `AISTAR_MAX_TOKENS` | local: `1024`, remote: `4096` | Response token limit, clamped `128..8192`. |
| `AISTAR_DEBUG_PAYLOAD` | `false` | Prints outgoing API payload JSON to stderr. |

### Conversation / Tool Loop

| Variable | Default | Behavior |
|---|---|---|
| `AISTAR_MAX_ASSISTANT_HISTORY_CHARS` | local: `1200`, remote: `3000` | Assistant history truncation budget (`200..20000`). |
| `AISTAR_MAX_TOOL_RESULT_HISTORY_CHARS` | local: `2500`, remote: `6000` | Tool-result truncation budget (`200..40000`). |
| `AISTAR_MAX_API_MESSAGES` | local: `14`, remote: `32` | Message count budget before pruning (`4..128`). |
| `AISTAR_TOOL_TIMEOUT_SECS` | local: `20`, remote: `60` | Per-tool execution timeout (`2..300`). |
| `AISTAR_MAX_TOOL_ROUNDS` | local: `12`, remote: `24` | Turn tool-call round cap (`2..64`). |
| `AISTAR_TOOL_CONFIRM` | local: `false`, remote: `true` | Requires interactive tool approval when enabled. |
| `AISTAR_STREAM_LOCAL_TOOL_EVENTS` | `false` | Streams local tool status events into output. |
| `AISTAR_STREAM_SERVER_EVENTS` | `true` | Streams server-side event markers. |
| `AISTAR_USE_STRUCTURED_BLOCKS` | `true` | Enables structured block rendering path. |

### Terminal / Rendering

| Variable | Default | Behavior |
|---|---|---|
| `AISTAR_FORCE_COLOR` | `false` | Forces color output even when detection would disable it. |
| `NO_COLOR` | unset | Disables color output when set. |
| `AISTAR_DISABLE_CURSOR` | `false` | Disables streaming cursor animation. |
| `AISTAR_DISABLE_FRAME_BATCHING` | `false` | Disables frame token batching. |
| `AISTAR_DISABLE_PROGRESSIVE_EFFECTS` | `false` | Disables progressive line-style effects. |
| `AISTAR_FRAME_INTERVAL_MS` | `16` | Frame pacing interval (`4..250`). |
| `AISTAR_THINKING_WRAP_WIDTH` | auto | Explicit thinking-wrap width (`40..300`). |

## Built-in TUI Commands

- `/commands` or `/help`
- `/clear`
- `/history`
- `/repo`
- `/ps`
- `/quit`
