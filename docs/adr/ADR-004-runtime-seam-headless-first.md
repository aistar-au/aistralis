# ADR-004: Runtime seam refactor — headless-first architecture (REF track)

**Date:** 2026-02-18  
**Status:** Superseded operationally by ADR-006 and ADR-007  
**Deciders:** Core maintainer  
**Related tasks:** REF-02 through REF-06 in `TASKS/` (planned; to be created)  
**Target completion:** v0.2.0

---

## Context

At v0.1.0-alpha, `vexcoder` has a single execution path: the `App` struct in `src/app/mod.rs` runs the conversation loop, reads keyboard input from a TTY, and renders directly to stdout. The TUI skeleton (`src/terminal/mod.rs`, `src/ui/`) uses `ratatui` and `crossterm` but is not wired into the main loop.

Three problems stem from this architecture:

1. **Testability**: `App::run()` cannot be tested without a real TTY. The `#[cfg(test)]` workaround (`VEX_TOOL_CONFIRM=false`) is fragile and only covers the tool-approval branch.

2. **Headless execution**: Running `vex` in CI, in a pipe, or via `COMMAND_TO_AGENT.txt` dispatch requires TTY detection hacks scattered across `src/app/mod.rs`. There is no clean headless mode.

3. **Future TUI**: The `ratatui` scaffolding exists but cannot be activated without gutting `App`. Any attempt to add a proper TUI will conflict with the current stdout renderer.

The dependency graph currently looks like this:

```
App (owns everything)
 ├── ConversationManager (API + tools)
 ├── StreamPrinter (stdout renderer — tightly coupled to App)
 └── Tool approval (keyboard reads inside the App event loop)
```

`ratatui` and `crossterm` are reachable from `src/app/mod.rs`, `src/terminal/mod.rs`, and `src/ui/`. They should not be reachable from `src/runtime/` (the planned module) or `src/state/`.

---

## Decision

Introduce a **runtime seam**: a thin abstraction layer between the conversation loop and the frontend (stdout renderer or TUI). This is a pure refactor — no new user-visible behaviour is added until the seam is established.

### Module ownership after refactor

| Module | Owns | Must not depend on |
| :--- | :--- | :--- |
| `src/runtime/` | Event loop, turn orchestration | `ratatui`, `crossterm`, any terminal I/O |
| `src/state/` | Conversation correctness, message history | All UI crates |
| `src/ui/` | Rendering, input reading | Business logic |
| `src/tools/` | Filesystem and git execution | All UI crates |

### Planned interface (subject to revision during REF-03)

```rust
// src/runtime/events.rs
pub enum RuntimeEvent {
    Keyboard(KeyEvent),
    Mouse(MouseEvent),
    Resize(u16, u16),
    Tick,
    Error(anyhow::Error),
}

// src/runtime/context.rs
pub struct RuntimeContext<'a> {
    pub config: &'a Config,
    pub state_snapshot: &'a ConversationSnapshot,
    pub width: u16,
    pub height: u16,
}

// src/runtime/mod.rs
pub trait RuntimeMode {
    fn handle_input(&mut self, event: RuntimeEvent, ctx: &RuntimeContext) -> Option<Action>;
    fn should_quit(&self) -> bool;
    fn on_tick(&mut self, ctx: &RuntimeContext) -> Option<Action>;
}
```

### Task manifest sequence (REF track)

| Task | Scope | Anchor test |
| :--- | :--- | :--- |
| REF-02 | Define `RuntimeEvent`, `RuntimeContext`, `RuntimeMode` trait stubs | `test_ref_02_runtime_types_compile` |
| REF-03 | Implement `RuntimeMode` trait for the existing stdout mode | `test_ref_03_stdout_mode_behaviour_parity` |
| REF-04 | Define `RuntimeEvent` mapping from `crossterm::event::Event` | `test_ref_04_event_mapping_roundtrip` |
| REF-05 | Generic runtime loop — replaces `App::run()` | `test_ref_05_headless_loop_terminates` |
| REF-06 | Extract TUI mode as second `RuntimeMode` implementor | `test_ref_06_tui_mode_renders_frame` |

### Scope discipline

During the REF track (REF-02 through REF-06):
- No new CLI flags or environment variables.
- No new tools.
- No changes to the Anthropic or OpenAI protocol paths.
- `cargo test --all` must pass after every task.
- Each task touches at most two files.

---

## Rationale

### Why carve the seam before adding the TUI?

Adding the TUI on top of the current `App` structure would require simultaneous refactoring and feature addition. This is the primary cause of regressions in agentic development: the agent conflates two concerns and breaks one while implementing the other. The seam-first approach makes each step verifiable in isolation.

### Why a trait instead of an enum of modes?

An enum of modes would require `src/runtime/loop.rs` to import from `src/ui/`, defeating the dependency boundary. A trait allows each mode to live in its own module with its own dependencies.

### Why `RuntimeContext<'a>` with a lifetime?

The loop owns the mutable `ConversationManager`. The context provides an immutable snapshot to the mode handler. A borrow (`&'a ConversationSnapshot`) avoids the clone cost on every tick. If the lifetime becomes awkward during REF-05, the snapshot field will be changed to an owned clone or `Arc`.

---

## Alternatives considered

### Keep `App` monolithic; duplicate for TUI

Results in two divergent conversation loops that must be kept in sync. The TDM workflow cannot safely maintain two parallel implementations of the same logic.

### Replace `App` with a TUI immediately

Loses the working stdout renderer before the TUI is proven. Users would have no fallback. Behaviour parity cannot be verified without the existing implementation as a reference.

### Actor model with channels (tokio tasks per concern)

More scalable but significantly more complex. The current `Arc<Mutex<ConversationManager>>` + `mpsc` channel approach already provides the necessary concurrency. The runtime seam does not require a deeper architectural change.

---

## Consequences

**Easier:**
- Headless execution (CI, pipe, `COMMAND_TO_AGENT.txt` dispatch) becomes a first-class `RuntimeMode`, testable without a TTY.
- The TUI can be developed and tested independently once REF-06 is complete.
- `ratatui` and `crossterm` are confined to `src/ui/` — they do not appear in `cargo check --target x86_64-unknown-linux-musl` for the `runtime` and `state` modules.

**Harder:**
- `RuntimeContext<'a>` lifetime management adds complexity. If the loop must provide mutable access to state *and* pass it to the mode, the borrow checker will require careful structuring.
- Six sequential tasks (REF-02 through REF-06) with a parity gate at each step takes longer than a single large refactor. This is a deliberate trade-off for safety.

**Constraints imposed on future work:**
- No `ratatui` or `crossterm` imports in `src/runtime/`, `src/state/`, or `src/tools/`. This is enforced by the `build-check` CI job targeting musl (which fails if TUI crates pull in platform-specific deps).
- REF-02 through REF-06 must be executed in order. REF-03 depends on REF-02 types; REF-05 depends on REF-03 and REF-04.
- Behaviour parity between the stdout mode (before) and after REF-06 is verified by `test_ref_06_tui_mode_renders_frame`. Any divergence is a regression.
