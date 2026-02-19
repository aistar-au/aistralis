# ADR-006: Runtime mode contracts — `RuntimeMode`, `RuntimeContext`, `RuntimeEvent`, `FrontendAdapter`

**Date:** 2026-02-18  
**Status:** Accepted  
**Deciders:** Core maintainer  
**Related tasks:** `TASKS/REF-02-runtime-mode-contract.md`, `TASKS/REF-03-tui-mode-implement.md`, `TASKS/REF-04-runtime-context-start-turn.md`, `TASKS/REF-05-runtime-loop.md`, `TASKS/REF-06-tui-frontend-adapter.md`  
**Amendment:** 2026-02-19 signature sections synchronized with accepted ADR-008 typed poll/interrupt contracts (`UserInputEvent`, `on_interrupt`)  
**Supersedes:** Nothing. Extends `ADR-004` with concrete type contracts.

---

## Context

ADR-004 established that a **runtime seam** must exist between the conversation loop and the frontend (TUI or headless) before any new UI mode can be added. It left the exact contract shapes to the REF task track.

REF-02 created compilable stubs and intentionally used a minimal borrowed
`RuntimeContext<'a>` shape. REF-03 wired `TuiMode` as the first
`RuntimeMode` implementation against that stub. This ADR locks the
**stage-by-stage contract sequence** and the end-state so implementers know
which shape is canonical at each REF step.

The core problem this solves: `App` in `src/app/mod.rs` currently owns everything — the event loop, conversation dispatch, tool approval, and all rendering. The TUI skeleton exists in `src/ui/` but is not the active renderer. Making the architecture extensible requires splitting ownership along three lines:

```
Before:  App (owns everything)
After:   Runtime<M: RuntimeMode>  ←  conversation loop, event routing
         M (TuiMode, BatchMode…)  ←  UI state + input decisions
         FrontendAdapter           ←  raw I/O, rendering
```

---

## Decision

### 1. `RuntimeMode` trait

Located in `src/runtime/mode.rs`. The **only** integration point between the runtime loop and any UI implementation.

```rust
pub trait RuntimeMode {
    /// Called when the frontend has confirmed a complete user input string.
    fn on_user_input(&mut self, input: String, ctx: &mut RuntimeContext);

    /// Called for every update emitted by the model/tool layer.
    fn on_model_update(&mut self, update: UiUpdate, ctx: &mut RuntimeContext);

    /// Called when the frontend emits an interrupt event.
    /// Default is a no-op so non-interrupting modes can opt out.
    fn on_interrupt(&mut self, _ctx: &mut RuntimeContext) {}

    /// Frontends poll this to guard input submission (e.g. disable Enter while streaming).
    fn is_turn_in_progress(&self) -> bool;
}
```

Design constraints:
- The trait must not mention `ratatui`, `crossterm`, or any terminal I/O type.
- `RuntimeContext` is passed by `&mut` so modes can drive side-effects (start turns, cancel, etc.) without holding a long-lived reference.
- Return types are `()` — modes communicate back through `ctx`, not through return values.

### 2. `RuntimeContext`

Located in `src/runtime/context.rs`. The **capability surface** a `RuntimeMode` can use to interact with the model/tool layer. Modes never reach directly into `ConversationManager` or `ApiClient`.

Canonical shape by task stage:

- **REF-02 and REF-03 (stub/call-site phase):**

```rust
pub struct RuntimeContext<'a> {
    pub conversation: &'a mut ConversationManager,
}

impl<'a> RuntimeContext<'a> {
    pub fn start_turn(&mut self, input: String);
    pub fn cancel_turn(&mut self);
}
```

- **REF-04 onward (dispatch/cancellation phase; canonical end-state):**

```rust
pub struct RuntimeContext {
    pub(crate) conversation: ConversationManager,
    pub(crate) update_tx: mpsc::UnboundedSender<UiUpdate>,
    pub(crate) cancel: CancellationToken,
}

impl RuntimeContext {
    /// Begin a new conversation turn. Spawns the async API task.
    pub fn start_turn(&mut self, input: String);

    /// Signal the in-flight turn to stop emitting updates.
    /// Does not kill the task; the task drains cleanly then sends TurnComplete.
    pub fn cancel_turn(&mut self);
}
```

Design constraints:
- `RuntimeContext` must not hold a `Terminal` or any UI type.
- REF-02/03 intentionally keep `RuntimeContext<'a>` minimal (borrowed conversation only) to keep stubs compile-only.
- `start_turn` is the **only** path through which API calls are initiated after REF-04. All existing `message_tx.send(content)` call sites in `App` migrate to `ctx.start_turn(input)` in REF-05.
- `cancel_turn` sets a cancellation signal; it does not `abort()` the Tokio task, preserving clean shutdown.

### 3. `UiUpdate` enum

Located in `src/runtime/update.rs`. Replaces the local `UiUpdate` definition previously in `src/app/mod.rs`.

```rust
pub enum UiUpdate {
    StreamDelta(String),
    StreamBlockStart { index: usize, block: StreamBlock },
    StreamBlockDelta { index: usize, delta: String },
    StreamBlockComplete { index: usize },
    ToolApprovalRequest(ToolApprovalRequest),
    TurnComplete,
    Error(String),
}
```

Design constraints:
- This type lives in `runtime`, not `app`, so `RuntimeMode`, `RuntimeContext`, and `Runtime<M>` can all reference it without a circular dependency.
- `src/app/mod.rs` imports it as `use crate::runtime::UiUpdate`.

### 4. `RuntimeEvent` enum

Located in `src/runtime/event.rs`. Represents the set of events the runtime loop dispatches internally (distinct from `UiUpdate` which flows to modes).

```rust
pub enum RuntimeEvent {
    TurnStarted { id: u64 },
    StreamDelta { text: String },
    ToolApprovalRequest(ToolApprovalRequest),
    TurnComplete,
    Error(String),
}
```

`RuntimeEvent` is reserved for future internal routing (e.g. REF-06 multi-mode dispatch). For REF-04 and REF-05, `UiUpdate` is the live wire; `RuntimeEvent` is a stub.

### 5. `FrontendAdapter` trait

Located in `src/runtime/frontend.rs`. Decouples raw I/O from the runtime loop.

```rust
pub enum UserInputEvent {
    Text(String),
    Interrupt,
}

pub trait FrontendAdapter<M: RuntimeMode> {
    fn poll_user_input(&mut self, mode: &M) -> Option<UserInputEvent>;
    fn render(&mut self, mode: &M);
    fn should_quit(&self) -> bool;
}
```

`TuiFrontend` (REF-06) wraps `ratatui` and `crossterm` behind this interface. A future `BatchFrontend` reads from stdin and never calls `render`. Neither requires changes to `Runtime<M>`.

### 6. `Runtime<M: RuntimeMode>` loop struct

Located in `src/runtime/loop.rs`. The host that owns the mode and drives the event cycle.

```rust
pub struct Runtime<M: RuntimeMode> {
    pub mode: M,
    update_rx: mpsc::UnboundedReceiver<UiUpdate>,
}
```

The `run()` method (REF-05) implements the generic loop:
1. Poll the `FrontendAdapter` for typed user input.
2. Route `UserInputEvent::Text(...)` to `mode.on_user_input(...)`.
3. Route `UserInputEvent::Interrupt` to `mode.on_interrupt(...)`.
4. Drain `update_rx` → `mode.on_model_update(...)`.
5. Ask the `FrontendAdapter` to render.
6. Repeat until `frontend.should_quit()`.

---

## Rationale

### Why `UiUpdate` in `runtime` rather than `app`?

The previous placement in `app` created a cycle: `runtime::mode` needed `UiUpdate` to express the `on_model_update` signature, but `UiUpdate` was defined in `app`. Moving it to `runtime::update` cuts the cycle cleanly. `app` depends on `runtime`; `runtime` depends on `state` and `types`; neither reaches back up.

### Why `RuntimeContext` holds `update_tx` after REF-04?

`start_turn` spawns a Tokio task. That task needs a sender to forward `UiUpdate`s back to the mode. Passing `update_tx` through `RuntimeContext` avoids the task capturing a raw `Arc<Mutex<...>>` of the whole `ConversationManager`, which would create a long-lived lock contention path.

### Why keep `TuiMode` in `src/app/mod.rs` rather than a new file?

CORE-09 Decision Record (2026-02-18): *"Do not add a new global `src/state.rs`; keep UI state local to `App` in `src/app/mod.rs` to avoid collision with the existing `src/state/` runtime namespace."* By extension, `TuiMode` stays in `app/mod.rs` because it is UI state. `RuntimeMode` is the interface; `TuiMode` is the implementation that lives where UI state lives.

### Why not a trait object (`Box<dyn RuntimeMode>`) in `Runtime`?

`RuntimeMode` methods take `&mut self` and `&mut RuntimeContext`. A `Box<dyn RuntimeMode>` would require the trait to be object-safe, which is currently satisfied, but any future method returning `impl Trait` would break it. Using a generic `<M: RuntimeMode>` keeps the door open and avoids allocation on every event.

---

## Alternatives considered

### Keep everything in `App`, add `--mode` flag when needed

Deferred by ADR-004. Adding a flag without the seam requires duplicating or wrapping the entire event loop. The blast radius of a mistake during that rewrite is the full application.

### Use `tokio::sync::watch` instead of `mpsc` for `update_rx`

`watch` is single-value: the receiver only sees the latest value, not every update. Streaming deltas require every event to be delivered in order; `mpsc::UnboundedReceiver` is the correct primitive.

### Put `RuntimeContext` in `src/state/`

`RuntimeContext` is staged: REF-02/03 use `&mut ConversationManager`; REF-04+
uses owned `ConversationManager` plus sender/cancellation plumbing. Placing it
in `state/` would still invert dependency direction by forcing state-layer code
to depend on runtime-layer update signaling.

---

## Consequences

**Easier:**
- Adding a new mode is a new struct + `impl RuntimeMode`. Zero changes to `Runtime<M>`, `RuntimeContext`, or `ConversationManager`.
- Headless/batch execution becomes a first-class `FrontendAdapter` with a minimal `poll_user_input` and a no-op `render`.
- `ratatui` and `crossterm` are confined to `src/ui/` and `src/app/mod.rs`. They do not appear in `src/runtime/` or `src/state/`.

**Harder:**
- REF-02/03 use `RuntimeContext<'a>`, so interim code must satisfy borrow
  lifetimes until REF-04 migrates to the owned context shape.
- The REF track is six sequential tasks. Jumping steps causes compilation failures because later tasks depend on earlier types.

**Constraints imposed on future work:**
- `RuntimeContext::start_turn` is the **sole** dispatch path after REF-05. `App::message_tx.send()` call sites become unreachable.
- No `ratatui` or `crossterm` imports in `src/runtime/`. The CI `grep` check from REF-02 must stay green.
- Any new `RuntimeMode` must handle `ToolApprovalRequest` — even if only to auto-deny — because the conversation layer will block waiting for a response.
- The `FrontendAdapter::render` signature takes `&M` (immutable reference to the mode). Modes must not require `&mut self` to expose their render state. Store render-ready snapshots or use interior mutability sparingly.

---

## Task sequence and anchors

| Task | Scope | Anchor test | Gate |
| :--- | :--- | :--- | :--- |
| REF-02 | Stub types: compile check | `test_ref_02_runtime_types_compile` | Must be green before REF-03 |
| REF-03 | `TuiMode` + `RuntimeMode` impl | `test_ref_03_tui_mode_overlay_blocks_input` | Must be green before REF-04 |
| REF-04 | Wire `ctx.start_turn` → API dispatch | `test_ref_04_start_turn_dispatches_message` | Must be green before REF-05 |
| REF-05 | Generic `Runtime<M>` loop replaces `App::run()` | `test_ref_05_headless_loop_terminates` | Must be green before REF-06 |
| REF-06 | `TuiFrontend` implements `FrontendAdapter` | `test_ref_06_tui_frontend_renders_frame` | Closes REF track |

## Compliance notes for agents

1. Do not implement `run()` on `Runtime<M>` until REF-05. The stub in `loop.rs` is intentional.
2. Do not move the `ratatui` draw loop out of `App` until REF-06. That is REF-06's job.
3. Do not add CLI flags, environment variables, or new tool definitions during the REF track.
4. `cargo test --all-targets` must pass after every task. Anchor tests from completed tasks must stay green.
5. The only files in scope per task are listed in that task's manifest. Do not edit adjacent files to make a test compile.
