# REF-08 DELTA-F: Deterministic env tests and cancel progression proof

## Problems

1. Env-mutating tests can race under parallel execution.
2. Token replacement checks after `cancel_turn()` are insufficient if they do not
   prove that the next turn can still emit updates.

## Targets

1. `src/test_support.rs`
2. env-mutating tests in `src/runtime/context.rs`, `src/state/conversation.rs`,
   and `src/api/client.rs`
3. `src/runtime/context.rs` post-cancel progression test
4. `CONTRIBUTING.md`

## Decision

1. Add process-wide test lock:
   `crate::test_support::ENV_LOCK: Mutex<()>`.
2. Require every env-mutating test to hold the lock for full test duration.
3. Keep parallel `cargo test --all-targets` as the default gate (no
   `--test-threads=1` requirement).
4. In post-cancel test, assert a subsequent turn emits runtime updates
   (`StreamDelta` or `TurnComplete`) after `cancel_turn()`.

## Required tests

1. `test_ref_08_cancel_turn_resets_root_token_for_next_turn`
2. `test_ref_08_tool_approval_forwarding_no_hang`
3. parallel `cargo test --all-targets`

## Acceptance

1. Env-mutating tests are deterministic in parallel.
2. A next turn proves forward progress after cancel token replacement.
