# ADR-008: Runtime cutover parity guardrails

**Date:** 2026-02-19
**Status:** Accepted
**Deciders:** Core maintainer
**Related tasks:** REF-08 delta set (A-F)
**Supersedes operationally:** none (complements ADR-007)

## Context

ADR-007 enforced the canonical dispatch path. Final REF-08 review identified
additional parity/safety rules that must stay true after the cutover:

1. assistant stream text must not merge into user prompt lines,
2. tool partial JSON must not leak into user-facing `StreamDelta`,
3. input editor must be UTF-8 boundary-safe,
4. frontend poll contract must be mode-aware and typed,
5. interrupt dispatch must be typed (not a magic string),
6. post-cancel flow must prove next-turn progression,
7. env-mutating tests must remain deterministic under parallel test execution.

## Decision (normative)

After REF-08:

1. MUST: Assistant streaming text MUST write only to assistant output slots and
   MUST NOT mutate user-prefixed (`> `) lines.
2. MUST: `ConversationStreamUpdate::BlockDelta` for non-textual blocks
   (`ToolCall`, `ToolResult`) MUST NOT be mirrored into `UiUpdate::StreamDelta`.
3. MUST: Input cursor and delete/backspace operations MUST be UTF-8
   boundary-safe.
4. MUST: Frontend poll contract MUST be mode-aware and typed:
   `poll_user_input(&mut self, mode: &M) -> Option<UserInputEvent>`.
5. MUST: Interrupt routing MUST use `UserInputEvent::Interrupt` and MUST NOT use
   a text sentinel.
6. MUST: `cancel_turn()` token replacement behavior MUST be validated by a test
   that proves a subsequent `start_turn()` emits updates.
7. MUST: Tests that mutate process env vars MUST hold
   `crate::test_support::ENV_LOCK`; CI gate remains parallel
   `cargo test --all-targets`.

## Rationale

These rules prevent output corruption, protocol leakage, UTF-8 input breakage,
and flaky test behavior while keeping runtime/frontend contracts explicit.

## Consequences

1. Runtime context tracks per-block textual classification.
2. Frontend/runtime coupling becomes intentionally typed.
3. Input editor keeps explicit character-boundary logic.
4. Test suite gains a shared env lock requirement for deterministic parallel runs.

## Compliance checks

1. `cargo test --all-targets`
2. `bash scripts/check_no_alternate_routing.sh`
3. `bash scripts/check_forbidden_imports.sh`
4. REF-08 targeted tests listed in `docs/dev/ref-08/review-checklist.md`
