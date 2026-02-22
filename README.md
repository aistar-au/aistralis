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

Source maps:

- App/raw links for the Rust application code: `CONTRIBUTING.md`
- Full repository raw URL map: `TASKS/completed/REPO-RAW-URL-MAP.md`
