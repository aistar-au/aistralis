# TASK: REF-05 — Generic `Runtime<M>` loop skeleton

**Status:** Merged
**Phase:** 5 (runtime loop)
**Track:** Runtime seam
**Depends on:** REF-03 green, REF-04 Track B merged (already satisfied)
**Blocks:** REF-06 (TuiFrontend wiring)
**Scope:** `src/runtime/loop.rs` (implementation + anchor unit tests)

---

## Background

REF-02 created a `Runtime<M: RuntimeMode>` struct in `src/runtime/loop.rs` with
`new()` and a `// wired in REF-05` comment. REF-03 implemented `TuiMode`. This
task adds `run()` — the generic event cycle that drives any `RuntimeMode` through
a `FrontendAdapter`.

Attempting to wire `RuntimeContext::start_turn` (REF-04 Track A) before this loop
exists would create a double-dispatch condition: the old `message_tx.send()` path
and the new `ctx.start_turn` path would both be live simultaneously. The loop
skeleton eliminates that risk by making the old path obviously dead before dispatch
is rewired.

No ratatui or crossterm may appear in `src/runtime/`. The loop is frontend-agnostic.

---

## §1 — `FrontendAdapter` trait

If not already present, add to `src/runtime/frontend.rs`:

```rust
use super::mode::RuntimeMode;

pub trait FrontendAdapter {
    /// Return the next user input event, or `None` if none is pending this tick.
    fn poll_user_input(&mut self) -> Option<String>;

    /// Render the current mode state.
    fn render<M: RuntimeMode>(&mut self, mode: &M);

    /// Return `true` when the loop should exit.
    fn should_quit(&self) -> bool;
}
```

---

## §2 — `Runtime<M>::run()`

Replace the stub comment in `src/runtime/loop.rs`:

```rust
use crate::runtime::{frontend::FrontendAdapter, mode::RuntimeMode, UiUpdate};
use super::context::RuntimeContext;
use tokio::sync::mpsc;

pub struct Runtime<M: RuntimeMode> {
    pub mode: M,
    update_rx: mpsc::UnboundedReceiver<UiUpdate>,
}

impl<M: RuntimeMode> Runtime<M> {
    pub fn new(mode: M, update_rx: mpsc::UnboundedReceiver<UiUpdate>) -> Self {
        Self { mode, update_rx }
    }

    pub fn run<F>(
        &mut self,
        frontend: &mut F,
        ctx: &mut RuntimeContext<'_>,
    ) where
        F: FrontendAdapter,
    {
        loop {
            if frontend.should_quit() {
                break;
            }
            if let Some(input) = frontend.poll_user_input() {
                self.mode.on_user_input(input, ctx);
            }
            while let Ok(update) = self.update_rx.try_recv() {
                self.mode.on_model_update(update, ctx);
            }
            frontend.render(&self.mode);
        }
    }
}
```

**Note:** `RuntimeContext<'a>` is borrowed so the loop must receive it as a
parameter rather than constructing it internally. The caller (`App` or a test)
owns the `ConversationManager` and lends it for the duration of the loop.

---

## §3 — Anchor test

The test must terminate without a real TTY or API call. A synchronous
`HeadlessFrontend` quits after a fixed render count.

The test lives as a `#[cfg(test)]` module inside `src/runtime/loop.rs` — not
in `tests/` — so it has full access to `ApiClient::new_mock`, `MockApiClient`,
and `ConversationManager::new_mock` without those needing to be re-exported.

```rust
// Inside src/runtime/loop.rs

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{mock_client::MockApiClient, ApiClient};
    use crate::runtime::{context::RuntimeContext, frontend::FrontendAdapter, UiUpdate};
    use crate::state::ConversationManager;
    use std::collections::{HashMap, VecDeque};
    use std::sync::Arc;
    use tokio::sync::mpsc;

    struct HeadlessFrontend {
        inputs: VecDeque<String>,
        render_count: usize,
        quit_after: usize,
    }

    impl HeadlessFrontend {
        fn new(inputs: Vec<&str>, quit_after: usize) -> Self {
            Self {
                inputs: inputs.into_iter().map(|s| s.to_string()).collect(),
                render_count: 0,
                quit_after,
            }
        }
    }

    impl FrontendAdapter for HeadlessFrontend {
        fn poll_user_input(&mut self) -> Option<String> {
            self.inputs.pop_front()
        }
        fn render<M: RuntimeMode>(&mut self, _mode: &M) {
            self.render_count += 1;
        }
        fn should_quit(&self) -> bool {
            self.render_count >= self.quit_after
        }
    }

    #[test]
    fn test_ref_05_headless_loop_terminates() {
        let mock = Arc::new(MockApiClient::new(vec![]));
        let client = ApiClient::new_mock(mock);
        let mut conversation = ConversationManager::new_mock(client, HashMap::new());
        let mut ctx = RuntimeContext { conversation: &mut conversation };

        let (_tx, update_rx) = mpsc::unbounded_channel::<UiUpdate>();
        let mode = crate::app::TuiMode::new();
        let mut runtime = Runtime::new(mode, update_rx);

        let mut frontend = HeadlessFrontend::new(vec!["hello", "world"], 3);
        runtime.run(&mut frontend, &mut ctx);

        assert_eq!(
            frontend.render_count, 3,
            "loop must render exactly quit_after times before exiting"
        );
    }
}
```

---

## §4 — Verification

```bash
# Anchor (now a unit test inside src/runtime/loop.rs)
cargo test test_ref_05_headless_loop_terminates -- --nocapture

# Prior anchors must stay green
cargo test test_ref_04_runtime_context_constructs -- --nocapture
cargo test test_ref_03_tui_mode_overlay_blocks_input -- --nocapture
cargo test test_ref_02_runtime_types_compile -- --nocapture

# Full suite
cargo test --all

# No ratatui/crossterm in runtime module
grep -r "ratatui\|crossterm" src/runtime/ && echo "FAIL: frontend leaked into runtime" || echo "clean"
```

---

## Definition of done

- [ ] `FrontendAdapter` trait exists in `src/runtime/frontend.rs`
- [ ] `Runtime<M>::run()` implemented with poll → dispatch → drain → render cycle
- [ ] Loop exits when `should_quit()` returns `true`
- [ ] `test_ref_05_headless_loop_terminates` passes without TTY or real API
- [ ] All prior anchors still green
- [ ] `cargo test --all` green
- [ ] No `ratatui` or `crossterm` anywhere in `src/runtime/`
- [ ] No changes to `src/app/mod.rs` or `src/runtime/context.rs`

## What NOT to do

- Do not move the ratatui draw loop out of `App` — that is REF-06
- Do not implement `start_turn` dispatch — that is REF-04 Track A
- Do not add `tokio::time::sleep` or tick intervals; the headless loop is synchronous
- Do not add CLI flags or environment variables
