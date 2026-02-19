use crate::runtime::{frontend::FrontendAdapter, mode::RuntimeMode, UiUpdate};
use tokio::sync::mpsc;

use super::context::RuntimeContext;

pub struct Runtime<M: RuntimeMode> {
    pub mode: M,
    #[allow(dead_code)]
    update_rx: mpsc::UnboundedReceiver<UiUpdate>,
}

impl<M: RuntimeMode> Runtime<M> {
    pub fn new(mode: M, update_rx: mpsc::UnboundedReceiver<UiUpdate>) -> Self {
        Self { mode, update_rx }
    }
    pub fn run<F>(&mut self, frontend: &mut F, ctx: &mut RuntimeContext<'_>)
    where
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{mock_client::MockApiClient, ApiClient};
    use crate::state::ConversationManager;
    use std::collections::{HashMap, VecDeque};
    use std::sync::Arc;

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

        fn render<N: RuntimeMode>(&mut self, _mode: &N) {
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
        let mut ctx = RuntimeContext {
            conversation: &mut conversation,
        };

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
