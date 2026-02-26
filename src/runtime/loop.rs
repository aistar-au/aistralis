use crate::runtime::{frontend::FrontendAdapter, mode::RuntimeMode, UiUpdate};
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration, Instant};

use super::context::RuntimeContext;

pub struct Runtime<M: RuntimeMode> {
    pub mode: M,
    update_rx: mpsc::UnboundedReceiver<UiUpdate>,
}

const IDLE_RENDER_TICK: Duration = Duration::from_millis(120);
const IDLE_LOOP_BACKOFF: Duration = Duration::from_millis(4);

impl<M: RuntimeMode> Runtime<M> {
    pub fn new(mode: M, update_rx: mpsc::UnboundedReceiver<UiUpdate>) -> Self {
        Self { mode, update_rx }
    }
    /// Execute the runtime loop.
    ///
    /// Must be called within a Tokio runtime context (e.g., `#[tokio::main]`
    /// or `block_on`). The async signature enforces `.await` at compile time
    /// for the loop path.
    pub async fn run<F>(&mut self, frontend: &mut F, ctx: &mut RuntimeContext)
    where
        F: FrontendAdapter<M>,
    {
        let mut last_render_at = Instant::now();
        let mut first_render_pending = true;
        loop {
            if frontend.should_quit() {
                break;
            }

            let mut state_changed = false;
            if let Some(input) = frontend.poll_user_input(&self.mode) {
                state_changed = true;
                self.mode.on_frontend_event(input, ctx);
            }

            while let Ok(update) = self.update_rx.try_recv() {
                state_changed = true;
                self.mode.on_model_update(update, ctx);
            }

            let now = Instant::now();
            let tick_due = now.saturating_duration_since(last_render_at) >= IDLE_RENDER_TICK;
            if first_render_pending || state_changed || tick_due {
                frontend.render(&self.mode);
                last_render_at = now;
                first_render_pending = false;
            } else {
                sleep(IDLE_LOOP_BACKOFF).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{mock_client::MockApiClient, ApiClient};
    use crate::runtime::frontend::UserInputEvent;
    use crate::state::ConversationManager;
    use std::collections::{HashMap, VecDeque};
    use std::sync::Arc;

    struct HeadlessFrontend {
        inputs: VecDeque<String>,
        render_count: usize,
        quit_after_renders: usize,
    }

    impl HeadlessFrontend {
        fn new(inputs: Vec<&str>, quit_after_renders: usize) -> Self {
            Self {
                inputs: inputs.into_iter().map(|s| s.to_string()).collect(),
                render_count: 0,
                quit_after_renders,
            }
        }
    }

    impl FrontendAdapter<crate::app::TuiMode> for HeadlessFrontend {
        fn poll_user_input(&mut self, _mode: &crate::app::TuiMode) -> Option<UserInputEvent> {
            self.inputs.pop_front().map(UserInputEvent::Text)
        }

        fn render(&mut self, _mode: &crate::app::TuiMode) {
            self.render_count += 1;
        }

        fn should_quit(&self) -> bool {
            self.render_count >= self.quit_after_renders
        }
    }

    struct InterruptMode {
        user_input_calls: usize,
        interrupt_calls: usize,
    }

    impl RuntimeMode for InterruptMode {
        fn on_user_input(&mut self, _input: String, _ctx: &mut RuntimeContext) {
            self.user_input_calls += 1;
        }

        fn on_model_update(&mut self, _update: UiUpdate, _ctx: &mut RuntimeContext) {}

        fn on_interrupt(&mut self, _ctx: &mut RuntimeContext) {
            self.interrupt_calls += 1;
        }

        fn on_frontend_event(&mut self, event: UserInputEvent, ctx: &mut RuntimeContext) {
            match event {
                UserInputEvent::Text(input) => self.on_user_input(input, ctx),
                UserInputEvent::Interrupt => self.on_interrupt(ctx),
                UserInputEvent::Scroll { .. } => {}
            }
        }

        fn is_turn_in_progress(&self) -> bool {
            false
        }
    }

    struct InterruptFrontend {
        events: VecDeque<UserInputEvent>,
        render_count: usize,
        quit_after_renders: usize,
    }

    impl InterruptFrontend {
        fn new(events: Vec<UserInputEvent>, quit_after_renders: usize) -> Self {
            Self {
                events: events.into_iter().collect(),
                render_count: 0,
                quit_after_renders,
            }
        }
    }

    impl FrontendAdapter<InterruptMode> for InterruptFrontend {
        fn poll_user_input(&mut self, _mode: &InterruptMode) -> Option<UserInputEvent> {
            self.events.pop_front()
        }

        fn render(&mut self, _mode: &InterruptMode) {
            self.render_count += 1;
        }

        fn should_quit(&self) -> bool {
            self.render_count >= self.quit_after_renders
        }
    }

    struct IdleFrontend {
        render_count: usize,
        start: Instant,
        runtime: Duration,
    }

    impl IdleFrontend {
        fn new(runtime: Duration) -> Self {
            Self {
                render_count: 0,
                start: Instant::now(),
                runtime,
            }
        }
    }

    impl FrontendAdapter<InterruptMode> for IdleFrontend {
        fn poll_user_input(&mut self, _mode: &InterruptMode) -> Option<UserInputEvent> {
            None
        }

        fn render(&mut self, _mode: &InterruptMode) {
            self.render_count += 1;
        }

        fn should_quit(&self) -> bool {
            self.start.elapsed() >= self.runtime
        }
    }

    /// REF-07: renamed to async to match `run()` signature change.
    #[tokio::test]
    async fn test_ref_05_headless_loop_terminates() {
        let mock = Arc::new(MockApiClient::new(vec![]));
        let client = ApiClient::new_mock(mock);
        let conversation = ConversationManager::new_mock(client, HashMap::new());

        let (tx, update_rx) = mpsc::unbounded_channel::<UiUpdate>();
        let mut ctx =
            RuntimeContext::new(conversation, tx, tokio_util::sync::CancellationToken::new());
        let mode = crate::app::TuiMode::new();
        let mut runtime = Runtime::new(mode, update_rx);

        let mut frontend = HeadlessFrontend::new(vec!["hello", "world"], 3);
        runtime.run(&mut frontend, &mut ctx).await;

        assert_eq!(
            frontend.render_count, 3,
            "loop must render exactly quit_after times before exiting"
        );
    }

    #[tokio::test]
    async fn test_ref_07_async_run_terminates() {
        let mock = Arc::new(MockApiClient::new(vec![]));
        let client = ApiClient::new_mock(mock);
        let conversation = ConversationManager::new_mock(client, HashMap::new());

        let (tx, update_rx) = mpsc::unbounded_channel::<UiUpdate>();
        let mut ctx =
            RuntimeContext::new(conversation, tx, tokio_util::sync::CancellationToken::new());
        let mode = crate::app::TuiMode::new();
        let mut runtime = Runtime::new(mode, update_rx);

        let mut frontend = HeadlessFrontend::new(vec!["hello"], 2);
        runtime.run(&mut frontend, &mut ctx).await;

        assert_eq!(frontend.render_count, 2);
    }

    #[tokio::test]
    async fn test_ref_08_interrupt_dispatches_on_interrupt_only() {
        let mock = Arc::new(MockApiClient::new(vec![]));
        let client = ApiClient::new_mock(mock);
        let conversation = ConversationManager::new_mock(client, HashMap::new());

        let (tx, update_rx) = mpsc::unbounded_channel::<UiUpdate>();
        let mut ctx =
            RuntimeContext::new(conversation, tx, tokio_util::sync::CancellationToken::new());
        let mode = InterruptMode {
            user_input_calls: 0,
            interrupt_calls: 0,
        };
        let mut runtime = Runtime::new(mode, update_rx);

        let mut frontend = InterruptFrontend::new(vec![UserInputEvent::Interrupt], 1);
        runtime.run(&mut frontend, &mut ctx).await;

        assert_eq!(runtime.mode.interrupt_calls, 1);
        assert_eq!(runtime.mode.user_input_calls, 0);
    }

    #[tokio::test]
    async fn test_render_is_tick_or_state_driven_when_idle() {
        let mock = Arc::new(MockApiClient::new(vec![]));
        let client = ApiClient::new_mock(mock);
        let conversation = ConversationManager::new_mock(client, HashMap::new());

        let (tx, update_rx) = mpsc::unbounded_channel::<UiUpdate>();
        let mut ctx =
            RuntimeContext::new(conversation, tx, tokio_util::sync::CancellationToken::new());
        let mode = InterruptMode {
            user_input_calls: 0,
            interrupt_calls: 0,
        };
        let mut runtime = Runtime::new(mode, update_rx);

        let mut frontend = IdleFrontend::new(Duration::from_millis(180));
        runtime.run(&mut frontend, &mut ctx).await;

        assert!(
            frontend.render_count <= 3,
            "idle render count should remain bounded by tick-driven scheduling"
        );
    }
}
