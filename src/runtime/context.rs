use crate::state::ConversationManager;

/// Borrowed per-tick context passed into every `RuntimeMode` callback.
///
/// The lifetime `'a` is the borrow of `ConversationManager` for one event loop
/// tick. A future task (REF-04 Track A) will evaluate whether this should
/// become an owned shape; for now the borrowed form is correct and sufficient.
///
/// `start_turn` is a no-op stub until `TASKS/REF-04-pre-conversation-dispatch-surface.md`
/// merges and exposes the required methods on `ConversationManager`.
pub struct RuntimeContext<'a> {
    pub conversation: &'a mut ConversationManager,
}

impl<'a> RuntimeContext<'a> {
    /// Initiate a user turn.
    ///
    /// # REF-04 gap — currently a no-op
    ///
    /// Full implementation is blocked on `TASKS/REF-04-pre-conversation-dispatch-surface.md`.
    /// The following must exist before this method can be wired:
    ///
    /// - `ConversationManager::push_user_message(&mut self, input: String)`
    /// - `ConversationManager::messages_for_api(&self) -> Vec<ApiMessage>`
    /// - `ConversationManager::client(&self) -> Arc<ApiClient>` (requires Arc refactor)
    /// - `ApiClient::create_stream_with_cancel(&self, msgs, token: CancellationToken)`
    ///
    /// The anchor test `test_ref_04_start_turn_dispatches` is marked `#[ignore]`
    /// and must be un-ignored only after REF-04-pre merges **and** Track A wiring
    /// is complete.
    pub fn start_turn(&mut self, _input: String) {
        // REF-04: gap — dispatch surface not yet exposed on ConversationManager
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
    /// IGNORED: blocked on TASKS/REF-04-pre-conversation-dispatch-surface.md.
    /// Un-ignore after REF-04-pre merges AND start_turn is wired in Track A.
    ///
    /// Required before un-ignoring:
    ///   - ConversationManager::push_user_message
    ///   - ConversationManager::messages_for_api
    ///   - ConversationManager::client() -> Arc<ApiClient>
    ///   - ApiClient::create_stream_with_cancel (CancellationToken variant)
    #[test]
    #[ignore = "REF-04-pre gap: ConversationManager dispatch surface not yet exposed"]
    fn test_ref_04_start_turn_dispatches() {
        let mut conversation = make_conversation();
        let mut ctx = RuntimeContext { conversation: &mut conversation };
        ctx.start_turn("hello".to_string());
        // Replace with real assertions once wired:
        //   assert!(matches!(update_rx.try_recv().unwrap(), UiUpdate::TurnComplete));
        todo!("wire assertions when REF-04-pre lands")
    }

    /// Smoke test — RuntimeContext constructs and start_turn is callable without panicking.
    /// Must stay green at all times.
    #[test]
    fn test_ref_04_runtime_context_constructs() {
        let mut conversation = make_conversation();
        let mut ctx = RuntimeContext { conversation: &mut conversation };
        // start_turn is a no-op stub (REF-04 gap). Calling it must not panic.
        ctx.start_turn("probe".to_string());
    }
}
