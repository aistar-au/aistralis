# REF-02: Define core runtime contracts as stub types

**Status:** Active  
**Date:** 2026-02-18  
**ADR:** [`docs/adr/ADR-006-runtime-mode-contracts.md`](../docs/adr/ADR-006-runtime-mode-contracts.md)  
**Depends on:** Nothing (first in REF track)  
**Blocks:** REF-03

---

## Context

ADR-006 locks four contracts that decouple the TUI from the conversation loop:
`RuntimeMode`, `RuntimeContext`, `RuntimeEvent`, `FrontendAdapter`, and the
`Runtime<M>` loop struct. These must exist as compilable types before any
implementation begins.

**QA finding addressed:** `src/runtime.rs` already exists as a flat file
containing `parse_bool_flag`, `parse_bool_str`, and `is_local_endpoint_url`.
`src/app/mod.rs` imports `use crate::runtime::parse_bool_flag`. These helpers
must be preserved. The solution is a standard Rust module conversion:
rename `src/runtime.rs` to `src/runtime/mod.rs`. Rust resolves `pub mod runtime`
in `lib.rs` to either form identically — no changes to `lib.rs` or `app/mod.rs`
are required.

This task adds stubs only. No logic, no wiring, no changes to `src/app/mod.rs`.

---

## Target files

| Operation | File |
| :--- | :--- |
| **Rename** (move, do not delete content) | `src/runtime.rs` → `src/runtime/mod.rs` |
| **Create** | `src/runtime/mode.rs` |
| **Create** | `src/runtime/context.rs` |
| **Create** | `src/runtime/event.rs` |
| **Create** | `src/runtime/frontend.rs` |
| **Create** | `src/runtime/loop.rs` |

Do **not** touch `src/lib.rs`, `src/app/mod.rs`, or any file outside `src/runtime/`.

---

## Step 1 — Convert flat file to module directory

```bash
mkdir src/runtime
mv src/runtime.rs src/runtime/mod.rs
```

After this `cargo check` must still pass. The existing helpers and their tests
remain in `src/runtime/mod.rs` and stay importable as `crate::runtime::*`.
Do not modify them.

Then add module declarations at the very top of `src/runtime/mod.rs`,
before the existing helper functions:

```rust
// NEW — add at top of src/runtime/mod.rs
pub mod context;
pub mod event;
pub mod frontend;
pub mod mode;
pub mod r#loop;  // `loop` is a reserved keyword; raw identifier required

// existing helpers follow unchanged
pub fn parse_bool_flag(...) { ... }
```

---

## Step 2 — Create stub files

### `src/runtime/mode.rs`

```rust
use crate::types::UiUpdate;
use super::context::RuntimeContext;

pub trait RuntimeMode {
    fn on_user_input(&mut self, input: String, ctx: &mut RuntimeContext);
    fn on_model_update(&mut self, update: UiUpdate, ctx: &mut RuntimeContext);
    fn is_turn_in_progress(&self) -> bool;
}
```

### `src/runtime/context.rs`

```rust
use crate::state::conversation::ConversationManager;
use crate::types::UiUpdate;
use tokio::sync::mpsc;

pub struct RuntimeContext<'a> {
    pub conversation: &'a mut ConversationManager,
}

impl<'a> RuntimeContext<'a> {
    pub fn start_turn(&mut self, _input: String, _tx: mpsc::UnboundedSender<UiUpdate>) {
        // wired in REF-04
    }
    pub fn cancel_turn(&mut self) {
        // wired in REF-04
    }
}
```

### `src/runtime/event.rs`

```rust
// Adjust import path if ToolApprovalRequest lives elsewhere in your crate.
use crate::types::ToolApprovalRequest;

pub enum RuntimeEvent {
    TurnStarted { id: u64 },
    StreamDelta { text: String },
    ToolApprovalRequest(ToolApprovalRequest),
    TurnComplete,
    Error(String),
}
```

### `src/runtime/frontend.rs`

```rust
use super::mode::RuntimeMode;

pub trait FrontendAdapter {
    fn poll_user_input(&mut self) -> Option<String>;
    fn render<M: RuntimeMode>(&mut self, mode: &M);
    fn should_quit(&self) -> bool;
}
```

### `src/runtime/loop.rs`

```rust
use crate::types::UiUpdate;
use tokio::sync::mpsc;
use super::{context::RuntimeContext, frontend::FrontendAdapter, mode::RuntimeMode};

pub struct Runtime<M: RuntimeMode> {
    pub mode: M,
    update_rx: mpsc::UnboundedReceiver<UiUpdate>,
}

impl<M: RuntimeMode> Runtime<M> {
    pub fn new(mode: M, update_rx: mpsc::UnboundedReceiver<UiUpdate>) -> Self {
        Self { mode, update_rx }
    }
    // run() wired in REF-05
}
```

---

## Step 3 — Add anchor test

Append inside the **existing** `#[cfg(test)] mod tests` block in
`src/runtime/mod.rs`. Do not create a second `mod tests`.

```rust
#[test]
fn test_ref_02_runtime_types_compile() {
    use crate::runtime::{
        context::RuntimeContext,
        event::RuntimeEvent,
        frontend::FrontendAdapter,
        mode::RuntimeMode,
    };
    // Zero-cost existence check — if the module tree compiles, this passes.
    let _ = std::mem::size_of::<RuntimeEvent>();
}
```

---

## Verification matrix

Run in order. Every item must be green before closing this task.

```bash
# 1. Catches any import breaks from the module rename
cargo check --all-targets

# 2. Anchor test
cargo test test_ref_02_runtime_types_compile -- --nocapture

# 3. Existing runtime helper tests must stay green
cargo test test_parse_bool_helpers -- --nocapture
cargo test test_is_local_endpoint_url_normalizes_case_and_space -- --nocapture

# 4. Full regression suite
cargo test --all

# 5. No ratatui/crossterm in src/runtime/ (use grep if musl target unavailable)
grep -r "ratatui\|crossterm" src/runtime/ && echo "FAIL: UI crates in runtime" || echo "clean"
```

Also confirm with git that the only changed files are inside `src/runtime/`:

```bash
git diff --name-only
# Expected output — nothing outside src/runtime/:
# src/runtime/mod.rs
# src/runtime/mode.rs
# src/runtime/context.rs
# src/runtime/event.rs
# src/runtime/frontend.rs
# src/runtime/loop.rs
```

---

## Definition of done

- [ ] `src/runtime/` directory exists; `src/runtime.rs` is gone.
- [ ] `src/runtime/mod.rs` contains the original helpers unchanged plus the five new `pub mod` declarations at the top.
- [ ] `src/runtime/{mode,context,event,frontend,loop}.rs` created as stubs.
- [ ] `cargo check --all-targets` passes — proves `use crate::runtime::parse_bool_flag` in `app/mod.rs` still resolves.
- [ ] `test_ref_02_runtime_types_compile` passes.
- [ ] `test_parse_bool_helpers` passes.
- [ ] `cargo test --all` is green.
- [ ] No `ratatui` or `crossterm` in `src/runtime/`.
- [ ] `git diff src/lib.rs src/app/mod.rs` shows no changes to those files.
