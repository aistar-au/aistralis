use crate::state::ConversationManager;

/// Borrowed per-tick context passed into every `RuntimeMode` callback.
///
/// The lifetime `'a` is the borrow of `ConversationManager` for one event loop
/// tick. A future task (REF-04 Track A) will evaluate whether this should
/// become an owned shape; for now the borrowed form is correct and sufficient.
///
/// `start_turn` is still a no-op stub while REF-04 Track A wiring is pending.
/// The prerequisite dispatch surface from
/// `TASKS/REF-04-pre-conversation-dispatch-surface.md` is already merged.
pub struct RuntimeContext<'a> {
    pub conversation: &'a mut ConversationManager,
}

impl<'a> RuntimeContext<'a> {
    /// Initiate a user turn.
    ///
    /// # REF-04 Track A pending — currently a no-op
    ///
    /// Full implementation is pending REF-04 Track A follow-up wiring.
    /// The prerequisite surface from `TASKS/REF-04-pre-conversation-dispatch-surface.md`
    /// is now available:
    ///
    /// - `ConversationManager::push_user_message(&mut self, input: String)`
    /// - `ConversationManager::messages_for_api(&self) -> Vec<ApiMessage>`
    /// - `ConversationManager::client(&self) -> Arc<ApiClient>` (provided by REF-04-pre)
    /// - `ApiClient::create_stream_with_cancel(&self, msgs, token: CancellationToken)`
    ///
    /// The anchor test `test_ref_04_start_turn_dispatches` remains `#[ignore]`
    /// until Track A dispatch wiring is implemented.
    pub fn start_turn(&mut self, _input: String) {
        // REF-04 Track A TODO: wire dispatch using the exposed conversation/api surface.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{mock_client::MockApiClient, ApiClient};
    use crate::state::ConversationManager;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn make_conversation() -> ConversationManager {
        let mock = Arc::new(MockApiClient::new(vec![]));
        let client = ApiClient::new_mock(mock);
        ConversationManager::new_mock(client, HashMap::new())
    }

    /// REF-04 anchor — start_turn dispatches a real turn.
    ///
    /// IGNORED: pending REF-04 Track A dispatch wiring in `start_turn`.
    ///
    /// Required before un-ignoring:
    ///   - ConversationManager::push_user_message
    ///   - ConversationManager::messages_for_api
    ///   - ConversationManager::client() -> Arc<ApiClient>
    ///   - ApiClient::create_stream_with_cancel (CancellationToken variant)
    #[test]
    #[ignore = "REF-04 Track A pending: start_turn dispatch not wired yet"]
    fn test_ref_04_start_turn_dispatches() {
        let mut conversation = make_conversation();
        let mut ctx = RuntimeContext {
            conversation: &mut conversation,
        };
        ctx.start_turn("hello".to_string());
        // Replace with real assertions once wired:
        //   assert!(matches!(update_rx.try_recv().unwrap(), UiUpdate::TurnComplete));
        todo!("wire assertions when REF-04 Track A dispatch wiring lands")
    }

    /// Smoke test — RuntimeContext constructs and start_turn is callable without panicking.
    /// Must stay green at all times.
    #[test]
    fn test_ref_04_runtime_context_constructs() {
        let mut conversation = make_conversation();
        let mut ctx = RuntimeContext {
            conversation: &mut conversation,
        };
        // start_turn is a no-op stub (REF-04 gap). Calling it must not panic.
        ctx.start_turn("probe".to_string());
    }
}
