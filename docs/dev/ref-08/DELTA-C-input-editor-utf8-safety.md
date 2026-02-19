# REF-08 DELTA-C: UTF-8 safe InputEditor

## Problem

Cursor, backspace, and delete operations can split multi-byte UTF-8 characters
if handled as raw byte offsets.

## Target

`src/app/mod.rs`

## Decision

1. Use boundary-safe helpers in `InputEditor`:
   1. `clamp_cursor_to_boundary_left`
   2. `prev_char_boundary`
   3. `next_char_boundary`
2. Apply helpers to:
   1. left/right/home/end cursor movement
   2. backspace/delete ranges
   3. insertion cursor clamping and restore paths
3. Keep undo/redo/history/multiline behavior unchanged.

## Required test

`test_input_editor_unicode_cursor_backspace_delete_safe`

## Acceptance

All edit operations remain boundary-safe for UTF-8 text.
