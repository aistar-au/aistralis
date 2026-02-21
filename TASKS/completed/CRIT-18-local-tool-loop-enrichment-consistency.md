# Task CRIT-18: Local Tool-Loop Enrichment Consistency

**Target File:** `src/api/client.rs`, `src/state/conversation.rs`, `docs/adr/ADR-015-local-endpoint-text-protocol-default.md`

**ADR:** ADR-015, ADR-014, ADR-007

**Depends on:** CRIT-17 (stream sanitization baseline), runtime-core policy centralization

---

## Issue

Local endpoint sessions still showed brittle tool-loop behavior:

1. Structured tool protocol defaulted on, even when local servers did not reliably
   honor structured tool blocks.
2. Tool rounds parsed via tagged fallback needed explicit verification that
   `tool_result` enrichment is persisted for the next round.

This caused user-visible loops where the model narrated actions without reliably
consuming tool outputs.

---

## Decision

1. Default `VEX_STRUCTURED_TOOL_PROTOCOL` to `false` for local endpoints when the
   env var is unset; keep remote default `true`.
2. Keep explicit env override behavior unchanged.
3. Keep local fallback rounds in text-protocol history form and assert that
   `tool_result` payloads are appended for subsequent rounds.
4. Update fallback loop tests to validate the new text-protocol persistence contract.

---

## Definition of Done

1. Local endpoint default protocol is text-first unless explicitly overridden.
2. Remote endpoint default remains structured.
3. Fallback tool-call round-trips prove enrichment text (`tool_result ...`) is
   persisted for the next model round.
4. `cargo clippy --all-targets -- -D warnings` is green.
5. `cargo test --all-targets` is green.

---

## Anchor Verification

- `test_structured_tool_protocol_defaults_off_for_local_endpoint`
- `test_structured_tool_protocol_defaults_on_for_remote_endpoint`
- `test_text_tagged_tool_call_executes_as_fallback_for_local_endpoint`
- `test_local_text_protocol_tool_round_trip`

**What NOT to do:**
- Do not move loop-reliability behavior into TUI-only code paths.
- Do not force remote endpoints into text-only protocol mode.
- Do not bypass runtime-core tool loop routing.
