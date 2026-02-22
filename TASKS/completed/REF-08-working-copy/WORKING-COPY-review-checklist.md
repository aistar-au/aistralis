# REF-08 Review Checklist

**Status:** Completed and verified (2026-02-19)  
**ADR mapping:** ADR-008 compliance checks; ADR-012 required verification

## Scope

1. `src/app/mod.rs`
2. `src/runtime/context.rs`
3. `src/runtime/frontend.rs`
4. `src/runtime/loop.rs`
5. `src/runtime/mode.rs`
6. `src/test_support.rs`
7. REF-08 docs under `TASKS/completed/REF-08-working-copy/`

## Verify behavior

1. User line and assistant stream line are separated.
2. Tool JSON does not appear in `StreamDelta`.
3. Unicode edit operations are UTF-8 boundary-safe.
4. Frontend poll signature is mode-aware and typed (`UserInputEvent`).
5. Interrupt uses typed event path, not magic text sentinel.
6. After `cancel_turn()`, a subsequent turn can still emit updates.
7. Env-mutating tests are serialized with `ENV_LOCK`.

## Commands

1. `cargo test test_ref_03_tui_mode_overlay_blocks_input -- --nocapture`
2. `cargo test test_ref_08_stream_delta_appends_to_assistant_placeholder_not_user_line -- --nocapture`
3. `cargo test test_ref_08_block_delta_partial_json_not_mirrored_to_stream_delta -- --nocapture`
4. `cargo test test_input_editor_unicode_cursor_backspace_delete_safe -- --nocapture`
5. `cargo test test_ref_08_interrupt_dispatches_on_interrupt_only -- --nocapture`
6. `cargo test test_interrupt_is_typed_event_not_magic_string_collision -- --nocapture`
7. `cargo test test_ref_08_cancel_turn_resets_root_token_for_next_turn -- --nocapture`
8. `cargo test --all-targets`
9. `bash scripts/check_no_alternate_routing.sh`
10. `bash scripts/check_forbidden_imports.sh`

## Validation result

All checks above were validated as part of REF-08 completion with no
architecture contract violations.
