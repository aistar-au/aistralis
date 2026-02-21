# ADR-003: Dual-protocol API client with URL-inferred protocol selection

**Date:** 2026-02-18  
**Status:** Accepted  
**Deciders:** Core maintainer  
**Related tasks:** None (original design decision; may spawn a FEAT task for additional backends)  
**Implemented in:** `src/api/client.rs` — `ApiProtocol`, `infer_api_protocol()`, `adapt_to_openai_chat_completions_url()`

---

## Context

`vexcoder` targets two distinct user groups:

1. **Remote API users** who connect to `api.anthropic.com` using the Anthropic Messages API (`/v1/messages`). This group uses Claude models and expects streaming SSE with `content_block_start` / `content_block_delta` events and native `tool_use` blocks.

2. **Local model users** who run llama.cpp server, Ollama, or LM Studio locally. These tools expose an OpenAI-compatible `/v1/chat/completions` endpoint. This group cannot use the Anthropic protocol but wants the same agentic loop with local models.

Requiring users to configure a protocol enum explicitly would create friction. Most users know their endpoint URL but do not necessarily know which wire protocol it speaks.

Additionally, tool call representation differs between protocols: Anthropic uses structured `tool_use` content blocks; OpenAI uses `tool_calls` arrays in assistant deltas. The stream parser and message history builder must handle both.

A tagged-text fallback (`<function=name><parameter=key>value</parameter></function>`) also exists for local models that do not support either native tool protocol reliably.

---

## Decision

Implement a single `ApiClient` that internally selects between `ApiProtocol::AnthropicMessages` and `ApiProtocol::OpenAiChatCompletions` based on the endpoint URL, with a manual override via `VEX_API_PROTOCOL`.

**Protocol inference rules** (`infer_api_protocol()`):
- URL contains `/chat/completions` → OpenAI
- URL ends with `/v1` → OpenAI (base path convention)
- Anything else → Anthropic Messages (default)

**URL adaptation** (`adapt_to_openai_chat_completions_url()`):
- `/v1/messages` → `/v1/chat/completions`
- `/v1` → `/v1/chat/completions`
- Already correct → unchanged

**Stream parser** (`src/api/stream.rs`): attempts Anthropic SSE parse first; on failure attempts OpenAI chunk parse. OpenAI tool calls are translated into the same `StreamEvent` enum used by the Anthropic path, so `ConversationManager` is protocol-agnostic above the stream layer.

**Tagged-text fallback**: if neither protocol produces tool use blocks, `parse_tagged_tool_calls()` scans the assistant text for `<function=name>` syntax. This provides compatibility with models that emit tool calls as formatted text rather than structured JSON.

---

## Rationale

URL-based inference requires no configuration change for the common case. A user switching from `api.anthropic.com/v1/messages` to `http://localhost:8080/v1/chat/completions` gets the correct protocol automatically. The override variable exists for edge cases (e.g., a proxy that serves the Anthropic protocol on a non-standard URL).

Translating OpenAI events to the Anthropic `StreamEvent` enum — rather than having two parallel code paths in `ConversationManager` — keeps the conversation logic in one place. The translation cost is minimal and contained to `stream.rs`.

The tagged-text fallback preserves compatibility with older or constrained local models without requiring the operator to configure anything. It is purely additive.

---

## Alternatives considered

### Separate `AnthropicClient` and `OpenAiClient` types

Cleaner at the type level but forces `ConversationManager` to accept a trait object, introducing dynamic dispatch and lifetime complexity. The unified `ApiClient` with an internal enum achieves the same separation with less indirection.

### User-configured protocol enum in config file

More explicit but creates friction. The most common migration path (Anthropic → local model) requires the user to edit two fields instead of one.

### OpenAI protocol only, with an Anthropic adapter

Would lose native Anthropic features (extended thinking, `betas` headers, native tool_choice) that do not map cleanly to the OpenAI schema.

---

## Consequences

**Easier:**
- Zero-config local model support. Point `ANTHROPIC_API_URL` at any OpenAI-compatible server and it works.
- `ConversationManager` is protocol-agnostic; no protocol logic leaks into the conversation layer.
- The tagged fallback means even models that ignore the tools schema can participate in the agentic loop.

**Harder:**
- The stream parser is more complex: it attempts two parse strategies per SSE event. Parse errors from the Anthropic path are silently retried as OpenAI before being logged.
- Testing requires mock streams for both protocols (see `src/api/mock_client.rs`).
- Adding a third protocol (e.g., Google Gemini) requires extending the enum, the inference logic, the URL adapter, and the stream parser. The abstraction is extensible but not free.

**Constraints imposed on future work:**
- Protocol selection must remain automatic (URL-inferred) for the common case. Do not make `VEX_API_PROTOCOL` required.
- New protocol-specific features (e.g., Anthropic extended thinking) must degrade gracefully when the active protocol is OpenAI.
- All protocol paths must be covered by integration tests using `MockApiClient`. Adding a new protocol path without mock coverage is not acceptable.
