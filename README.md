# aistar-rs


Terminal-first coding assistant with streaming responses, tool execution, and ratatui UI.

## Quick Start

```bash
cargo run
```

## API Endpoint Configuration

Set `ANTHROPIC_API_URL` to the protocol-specific endpoint. `AISTAR_API_PROTOCOL`
can be set explicitly, or omitted and inferred from the URL.

| Protocol | `AISTAR_API_PROTOCOL` | `ANTHROPIC_API_URL` endpoint |
|---|---|---|
| Anthropic Messages | `anthropic` | `.../v1/messages` |
| OpenAI Chat Completions | `openai` | `.../v1/chat/completions` |

Remote endpoints require `ANTHROPIC_API_KEY`. Localhost endpoints do not.

Anthropic example:

```bash
ANTHROPIC_API_URL=https://api.anthropic.com/v1/messages \
AISTAR_API_PROTOCOL=anthropic \
ANTHROPIC_API_KEY=your_key \
cargo run
```

OpenAI example:

```bash
ANTHROPIC_API_URL=https://api.openai.com/v1/chat/completions \
AISTAR_API_PROTOCOL=openai \
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
