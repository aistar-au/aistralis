# TASK: REF-08 — Full Runtime-Core Cutover

**Status:** Completed (merged to `main` on 2026-02-19)  
**Track:** REF (Refactor)  
**Depends on:** REF-07  
**Blocks:** nothing — this is the final REF track task  
**ADR:** ADR-006, ADR-007, ADR-008, ADR-009, ADR-010, ADR-011, ADR-012  
**Scope:** `src/app/mod.rs`, `src/runtime/context.rs`, `src/util/mod.rs` (new),
`src/lib.rs`, `scripts/` (new), `.github/workflows/arch-contracts.yml` (new),
`docs/adr/` (updates)

> **Greenfield policy:** No users, no production traffic, no compatibility window required.
> Remove alternate routing immediately. No warn-mode, no dual-path, no fallback.
> This task is one atomic PR.

---

## Completion Record

REF-08 is completed and archived.

Delivered outcomes:

1. Runtime-core canonical dispatch is the only production turn path.
2. Architecture contracts are enforced in CI (`check_no_alternate_routing`,
   `check_forbidden_imports`, and tests).
3. Runtime parity and safety deltas A-F are documented and verified.
4. Cutover guardrails and deployment gates are recorded in ADR-008 and ADR-012.

Cutover delta docs:

1. `docs/dev/ref-08/DELTA-A-assistant-stream-separation.md`
2. `docs/dev/ref-08/DELTA-B-blockdelta-stream-filter.md`
3. `docs/dev/ref-08/DELTA-C-input-editor-utf8-safety.md`
4. `docs/dev/ref-08/DELTA-D-frontend-mode-aware-poll-contract.md`
5. `docs/dev/ref-08/DELTA-E-typed-interrupt-routing.md`
6. `docs/dev/ref-08/DELTA-F-deterministic-env-tests-and-cancel-progression.md`
7. `docs/dev/ref-08/review-checklist.md`

---

## What This Task Does

After REF-02 → REF-07, the runtime seam exists and the loop is async. One problem
remains: `App` still owns a parallel dispatch path (`message_tx` / `update_rx` worker)
that runs alongside the runtime. Both paths are live simultaneously.

This task cuts the old path completely and makes `Runtime<M>::run` the sole owner
of the turn lifecycle.

**Before:**
```
User input → App::message_tx → background worker → ConversationManager::send_message
                                                  → App::update_rx loop → TuiMode
```

**After:**
```
User input → TuiFrontend::poll_user_input → Runtime<M>::run → TuiMode::on_user_input
                                                             → RuntimeContext::start_turn
                                                             → UiUpdate → TuiMode::on_model_update
```

---

## Step 0 — Inventory Before Touching Anything

Run these before writing a single line:

```bash
# Everything that must be deleted
grep -n "message_tx\|message_rx\|update_rx" src/app/mod.rs

# All direct send_message calls in App (must become ctx.start_turn)
grep -n "send_message" src/app/mod.rs

# ConversationStreamUpdate references (must be removed)
grep -rn "ConversationStreamUpdate" src/

# Confirm util helpers live in runtime today (they move in Step 1)
grep -n "parse_bool_flag\|parse_bool_str\|is_local_endpoint_url" src/runtime/mod.rs
```

Print the line numbers. Those are your delete checklist. Work through it in order.

---

## Step 1 — Extract `src/util` Module

Move the three cross-layer helpers out of `src/runtime/mod.rs` so import rules
are enforceable without circular dependency.

**Create `src/util/mod.rs`:**

```rust
/// Parse "true"/"false"/"1"/"0" from an owned String.
pub fn parse_bool_flag(s: String) -> Option<bool> {
    parse_bool_str(&s)
}

/// Parse "true"/"false"/"1"/"0" from a &str.
pub fn parse_bool_str(s: &str) -> Option<bool> {
    match s.trim().to_lowercase().as_str() {
        "true" | "1" | "yes" => Some(true),
        "false" | "0" | "no" => Some(false),
        _ => None,
    }
}

/// Returns true for localhost, 127.x.x.x, and ::1 URLs (case-insensitive, trims whitespace).
pub fn is_local_endpoint_url(url: &str) -> bool {
    let u = url.trim().to_lowercase();
    u.contains("localhost") || u.contains("127.0.0.") || u.contains("::1")
}
```

**Add `pub mod util;` in `src/lib.rs`.**

**Update imports** — replace `crate::runtime::{parse_bool_flag, …}` with
`crate::util::{parse_bool_flag, …}` in:
- `src/config/mod.rs`
- `src/api/client.rs`
- `src/state/conversation.rs`
- `src/runtime/mod.rs` (remove the definitions, keep the `pub mod` declarations)

**Remove the function bodies from `src/runtime/mod.rs`.** The functions must not
exist in two places.

`cargo check --all-targets` must pass before proceeding.

---

## Step 2 — Implement `RuntimeContext::start_turn` with Full Protocol Parity

The current `start_turn` (REF-04 / REF-07) handles `StreamDelta` and `TurnComplete`.
It must also forward the full event set that `ConversationManager::send_message`
emits, so `TuiMode` receives identical updates regardless of which path was used.

**In `src/runtime/context.rs`, extend the spawn closure:**

```rust
pub fn start_turn(&mut self, input: String) {
    if tokio::runtime::Handle::try_current().is_err() {
        let _ = self.update_tx.send(UiUpdate::Error(
            "runtime error: start_turn requires active Tokio runtime".to_string(),
        ));
        return;
    }

    self.conversation.push_user_message(input.clone());

    let turn_cancel = self.cancel.child_token();
    let tx = self.update_tx.clone();
    let messages = self.conversation.messages_for_api();
    let client = self.conversation.client();

    tokio::spawn(async move {
        let result = client
            .create_stream_with_cancel(&messages, turn_cancel.clone())
            .await;

        match result {
            Ok(mut stream) => {
                use futures::StreamExt;
                let mut block_index: usize = 0;
                while let Some(event) = stream.next().await {
                    if turn_cancel.is_cancelled() {
                        break;
                    }
                    match event {
                        // Map ALL StreamEvent variants to UiUpdate
                        StreamEvent::ContentBlockStart { index, block } => {
                            block_index = index;
                            let _ = tx.send(UiUpdate::StreamBlockStart { index, block });
                        }
                        StreamEvent::ContentBlockDelta { index, delta } => {
                            let _ = tx.send(UiUpdate::StreamBlockDelta { index, delta: delta.clone() });
                            // Also emit the flat delta for modes that only watch StreamDelta
                            let _ = tx.send(UiUpdate::StreamDelta(delta));
                        }
                        StreamEvent::ContentBlockStop { index } => {
                            let _ = tx.send(UiUpdate::StreamBlockComplete { index });
                        }
                        StreamEvent::ToolUse(req) => {
                            let _ = tx.send(UiUpdate::ToolApprovalRequest(req));
                        }
                        StreamEvent::MessageStop => {
                            // Do not emit TurnComplete here — wait for the stream to end
                        }
                        _ => {}
                    }
                }
                let _ = tx.send(UiUpdate::TurnComplete);
            }
            Err(e) => {
                let _ = tx.send(UiUpdate::Error(e.to_string()));
            }
        }
    });
}
```

> **Exactly one terminal event per turn:** `TurnComplete` or `Error`, never both,
> never zero. The `break` on cancellation still emits `TurnComplete` via the
> code path after the `while` loop exits.

**Verify with the existing anchor:**
```bash
cargo test test_ref_04_start_turn_dispatches_message -- --nocapture
```

---

## Step 3 — Add `TuiFrontend` and Wire `App` as Composition Root

`App` stops being a dispatch engine. It becomes the struct that constructs
`RuntimeContext`, `Runtime<TuiMode>`, and `TuiFrontend`, then delegates
entirely to `runtime.run(...)`.

### 3a — Add `TuiFrontend` in `src/app/mod.rs`

```rust
pub struct TuiFrontend {
    terminal: Terminal<CrosstermBackend<std::io::Stdout>>,
    quit: bool,
}

impl TuiFrontend {
    pub fn new(terminal: Terminal<CrosstermBackend<std::io::Stdout>>) -> Self {
        Self { terminal, quit: false }
    }
}

impl FrontendAdapter for TuiFrontend {
    fn poll_user_input(&mut self) -> Option<String> {
        // Non-blocking crossterm event poll.
        // Return Some(string) only when the user presses Enter with a non-empty buffer.
        // The input buffer lives on TuiMode (via HistoryState/InputState); this method
        // reads raw key events and forwards them through the mode's keymap.
        // Overlay-active guard is enforced by TuiMode::on_user_input — not here.
        use crossterm::event::{poll, read, Event, KeyCode, KeyModifiers};
        use std::time::Duration;

        if poll(Duration::from_millis(16)).unwrap_or(false) {
            if let Ok(Event::Key(key)) = read() {
                match key.code {
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        self.quit = true;
                    }
                    // Additional key routing delegated to TuiMode via the return value.
                    // Returning Some(_) here submits; the mode filters overlay state.
                    _ => {}
                }
            }
        }
        None  // Actual submit routing wired in FEAT-13
    }

    fn render<M: RuntimeMode>(&mut self, mode: &M) {
        self.terminal.draw(|frame| {
            draw_tui_frame(frame, mode);  // existing draw helper
        }).ok();
    }

    fn should_quit(&self) -> bool {
        self.quit
    }
}
```

### 3b — Rewrite `App::new` and `App::run`

```rust
pub struct App {
    runtime: Runtime<TuiMode>,
    frontend: TuiFrontend,
    ctx: RuntimeContext,
}

impl App {
    pub fn new(config: Config) -> Result<Self> {
        let client = ApiClient::new(&config)?;
        let executor = ToolExecutor::new(config.workspace.clone());
        let conversation = ConversationManager::new(client, executor);

        let (update_tx, update_rx) = mpsc::unbounded_channel::<UiUpdate>();
        let ctx = RuntimeContext::new(
            conversation,
            update_tx,
            CancellationToken::new(),
        );

        let mode = TuiMode::new();
        let runtime = Runtime::new(mode, update_rx);

        let terminal = setup_terminal()?;
        let frontend = TuiFrontend::new(terminal);

        Ok(Self { runtime, frontend, ctx })
    }

    pub async fn run(&mut self) -> Result<()> {
        self.runtime.run(&mut self.frontend, &mut self.ctx).await;
        Ok(())
    }
}
```

### 3c — Delete alternate routing fields and all associated code

In `src/app/mod.rs`, delete every instance of:

| Symbol | Action |
|--------|--------|
| `message_tx: mpsc::UnboundedSender<_>` | Delete field |
| `message_rx: mpsc::UnboundedReceiver<_>` | Delete field |
| Worker task spawned on `message_rx` | Delete |
| `update_rx.recv()` / `update_rx.try_recv()` loops | Delete |
| Direct `mgr.send_message(…)` calls | Delete |
| `ConversationStreamUpdate` match arms | Delete |

`cargo check --all-targets` must pass after this step.

---

## Step 4 — Remove Stale Helper Paths

Any helpers that existed only to support the deleted routing paths:

```bash
# Find dead code the compiler will warn about after Step 3
cargo check --all-targets 2>&1 | grep "unused\|dead_code\|never used"
```

Delete each. Do not leave `#[allow(dead_code)]` annotations — if it isn't used,
it goes.

---

## Step 5 — Add Architecture Enforcement Scripts

These are gates, not warnings. The codebase is greenfield; there is no grace period.

**`scripts/check_no_alternate_routing.sh`:**

```bash
#!/usr/bin/env bash
set -euo pipefail

PATTERNS=(
    "message_tx"
    "message_rx"
    "send_message("
    "ConversationStreamUpdate"
    "update_rx\.recv"
    "update_rx\.try_recv"
)

FAIL=0
for pattern in "${PATTERNS[@]}"; do
    if grep -rn "$pattern" src/app/; then
        echo "FAIL: forbidden pattern '$pattern' found in src/app/"
        FAIL=1
    fi
done

if [ $FAIL -eq 1 ]; then
    echo ""
    echo "Alternate routing is forbidden. See ADR-007."
    exit 1
fi

echo "check_no_alternate_routing: clean"
```

**`scripts/check_forbidden_imports.sh`:**

```bash
#!/usr/bin/env bash
set -euo pipefail

FORBIDDEN_MODULES=(
    "runtime::context"
    "runtime::mode"
    "runtime::loop"
    "runtime::frontend"
    "runtime::update"
    "runtime::event"
    "crate::app"
)

DIRS_TO_CHECK=("src/state" "src/api" "src/tools")

FAIL=0
for dir in "${DIRS_TO_CHECK[@]}"; do
    for mod in "${FORBIDDEN_MODULES[@]}"; do
        if grep -rn "use.*$mod\|extern.*$mod" "$dir/" 2>/dev/null; then
            echo "FAIL: $dir imports forbidden module $mod"
            FAIL=1
        fi
    done
done

if [ $FAIL -eq 1 ]; then
    echo ""
    echo "Layer violation detected. See ADR-007."
    exit 1
fi

echo "check_forbidden_imports: clean"
```

**Make both executable:**
```bash
chmod +x scripts/check_no_alternate_routing.sh
chmod +x scripts/check_forbidden_imports.sh
```

**`.github/workflows/arch-contracts.yml`:**

```yaml
name: Architecture Contracts

on:
  push:
    branches: [main]
  pull_request:

jobs:
  arch-check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable
        with:
          toolchain: "1.93.1"

      - name: Check no alternate routing
        run: bash scripts/check_no_alternate_routing.sh

      - name: Check forbidden imports
        run: bash scripts/check_forbidden_imports.sh

      - name: Run tests
        run: cargo test --all-targets
```

---

## Step 6 — Add ADR-007

**Create `docs/adr/ADR-007-runtime-canonical-dispatch-no-alt-routing.md`:**

```markdown
# ADR-007: Runtime-core canonical dispatch — no alternate routing

**Date:** 2026-02-19
**Status:** Accepted
**Deciders:** Core maintainer
**Supersedes operationally:** ADR-004 (headless-first seam — now fully realized)
**Related tasks:** TASKS/REF-08-full-runtime-cutover.md

## Decision (normative)

After REF-08:

- MUST: All user input MUST flow only through `Runtime<M>::run` →
  `RuntimeMode::on_user_input` → `RuntimeContext::start_turn`.
- MUST NOT: No code outside `RuntimeContext::start_turn` may call
  `ConversationManager::send_message` in the production path.
- MUST NOT: `src/app` MUST NOT own any `mpsc` channel for conversation dispatch.
- MUST NOT: `src/state`, `src/api`, `src/tools` MUST NOT import runtime dispatch
  interfaces (`runtime::context`, `runtime::mode`, `runtime::loop`,
  `runtime::frontend`, `runtime::update`, `runtime::event`) or `crate::app`.
- MUST: `RuntimeContext::start_turn` MUST emit exactly one terminal event per turn
  (`TurnComplete` or `Error`).
- MUST: `RuntimeContext::start_turn` MUST check for an active Tokio runtime before
  spawning; on failure it emits `UiUpdate::Error` and returns without touching history.

These rules are enforced by `scripts/check_no_alternate_routing.sh` and
`scripts/check_forbidden_imports.sh` in CI.
```

**Update `docs/adr/ADR-004-runtime-seam-headless-first.md` status:**
```markdown
**Status:** Superseded operationally by ADR-006 and ADR-007
```

**Update `docs/adr/README.md` index** to add ADR-007 and reflect ADR-004's status.

**Update `CONTRIBUTING.md`** — add one sentence to the Change Confirmation Rule:
```
Runtime mode additions and naming-policy changes require explicit confirmation
before implementation or documentation. See ADR-007.
```

---

## Acceptance Tests

All of these must be green on the final commit:

```bash
# Existing anchors — must not regress
cargo test test_ref_02_runtime_types_compile -- --nocapture
cargo test test_ref_03_tui_mode_overlay_blocks_input -- --nocapture
cargo test test_ref_04_start_turn_dispatches_message -- --nocapture
cargo test test_ref_05_headless_loop_terminates -- --nocapture
cargo test test_ref_07_async_run_terminates -- --nocapture
cargo test test_ref_07_no_runtime_guard -- --nocapture

# New parity test
cargo test test_ref_08_start_turn_full_protocol_parity -- --nocapture

# Architecture gates
bash scripts/check_no_alternate_routing.sh
bash scripts/check_forbidden_imports.sh

# Full suite
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```

### Add `test_ref_08_start_turn_full_protocol_parity`

In `src/runtime/context.rs` tests:

```rust
#[tokio::test]
async fn test_ref_08_start_turn_full_protocol_parity() {
    // MockApiClient configured to emit: BlockStart, BlockDelta x2, BlockStop, ToolUse, TurnComplete
    // Verify UiUpdate sequence matches expected order with exactly one TurnComplete
    // and no Error.
    let chunks = vec![
        /* configure per mock client API */
    ];
    let (tx, mut rx) = mpsc::unbounded_channel::<UiUpdate>();
    let client = ApiClient::new_mock(Arc::new(MockApiClient::new(chunks)));
    let conversation = ConversationManager::new_mock(client, HashMap::new());
    let mut ctx = RuntimeContext::new(conversation, tx, CancellationToken::new());

    ctx.start_turn("test".to_string());

    let mut events: Vec<&str> = vec![];
    loop {
        match tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
            Ok(Some(UiUpdate::StreamBlockStart { .. })) => events.push("BlockStart"),
            Ok(Some(UiUpdate::StreamBlockDelta { .. })) => events.push("BlockDelta"),
            Ok(Some(UiUpdate::StreamBlockComplete { .. })) => events.push("BlockComplete"),
            Ok(Some(UiUpdate::StreamDelta(_))) => events.push("Delta"),
            Ok(Some(UiUpdate::TurnComplete)) => { events.push("TurnComplete"); break; }
            Ok(Some(UiUpdate::Error(e))) => panic!("unexpected error: {e}"),
            _ => break,
        }
    }

    assert!(events.contains(&"TurnComplete"), "must terminate with TurnComplete");
    assert_eq!(events.iter().filter(|&&e| e == "TurnComplete").count(), 1,
        "exactly one TurnComplete");
}
```

---

## Definition of Done

- [ ] `src/util/mod.rs` exists; helpers removed from `src/runtime/mod.rs`
- [ ] `RuntimeContext::start_turn` emits full event set with exactly one terminal event
- [ ] `TuiFrontend` implements `FrontendAdapter` in `src/app/mod.rs`
- [ ] `App::new` constructs `RuntimeContext` + `Runtime<TuiMode>` + `TuiFrontend`
- [ ] `App::run` calls only `runtime.run(...).await`
- [ ] No `message_tx`, `message_rx`, `update_rx` loop, or direct `send_message` in `src/app/`
- [ ] No `ConversationStreamUpdate` handling in `src/app/`
- [ ] `scripts/check_no_alternate_routing.sh` exits 0
- [ ] `scripts/check_forbidden_imports.sh` exits 0
- [ ] `.github/workflows/arch-contracts.yml` exists and is correct
- [ ] `ADR-007` created; `ADR-004` status updated; `README.md` index updated; `CONTRIBUTING.md` updated
- [ ] All six prior REF anchors still green
- [ ] `test_ref_08_start_turn_full_protocol_parity` passes
- [ ] `cargo test --all-targets` green
- [ ] `cargo clippy --all-targets -- -D warnings` clean

## What NOT to Do

- Do not add CLI flags or new runtime modes
- Do not add `#[allow(dead_code)]` to paper over deleted call sites — delete the dead code
- Do not keep any "transition" comments that reference the old routing path
- Do not split this into multiple PRs — it lands as one atomic cutover
