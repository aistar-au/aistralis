use crate::runtime::UiUpdate;
use crate::state::ConversationManager;
use futures::StreamExt;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Capability surface passed to `RuntimeMode` methods.
///
/// Owns `ConversationManager` (not a borrow) so that REF-05's runtime loop
/// can hold it without a lifetime parameter. See ADR-006 §2.
pub struct RuntimeContext {
    pub(crate) conversation: ConversationManager,
    pub(crate) update_tx: mpsc::UnboundedSender<UiUpdate>,
    pub(crate) cancel: CancellationToken,
}

impl RuntimeContext {
    pub fn new(
        conversation: ConversationManager,
        update_tx: mpsc::UnboundedSender<UiUpdate>,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            conversation,
            update_tx,
            cancel,
        }
    }

    pub fn start_turn(&mut self, input: String) {
        self.conversation.push_user_message(input);

        let turn_cancel = self.cancel.child_token();
        let tx = self.update_tx.clone();
        let messages = self.conversation.messages_for_api();
        let client = self.conversation.client();

        tokio::spawn(async move {
            let result = client.create_stream_with_cancel(&messages, turn_cancel.clone()).await;

            match result {
                Ok(mut stream) => {
                    while let Some(chunk_result) = stream.next().await {
                        if turn_cancel.is_cancelled() {
                            break;
                        }

                        match chunk_result {
                            Ok(chunk) => {
                                let text = String::from_utf8_lossy(&chunk).to_string();
                                let text = text.trim().to_string();
                                if !text.is_empty() {
                                    let _ = tx.send(UiUpdate::StreamDelta(text));
                                }
                            }
                            Err(e) => {
                                let _ = tx.send(UiUpdate::Error(e.to_string()));
                                return;
                            }
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

    pub fn cancel_turn(&mut self) {
        self.cancel.cancel();
        self.cancel = CancellationToken::new();
    }
}

#[cfg(test)]
mod tests {
    use super::RuntimeContext;
    use crate::api::{mock_client::MockApiClient, ApiClient};
    use crate::runtime::UiUpdate;
    use crate::state::ConversationManager;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::mpsc;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn test_ref_04_start_turn_dispatches_message() {
        let (tx, mut rx) = mpsc::unbounded_channel::<UiUpdate>();

        let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![vec![
            "Hello".to_string(),
            " world".to_string(),
        ]])));
        let conversation = ConversationManager::new_mock(client, HashMap::new());

        let mut ctx = RuntimeContext::new(conversation, tx, CancellationToken::new());

        ctx.start_turn("test input".to_string());

        let mut saw_delta = false;
        let mut saw_complete = false;
        loop {
            match tokio::time::timeout(std::time::Duration::from_millis(500), rx.recv()).await {
                Ok(Some(UiUpdate::StreamDelta(_))) => saw_delta = true,
                Ok(Some(UiUpdate::TurnComplete)) => {
                    saw_complete = true;
                    break;
                }
                Ok(Some(UiUpdate::Error(e))) => panic!("unexpected error: {e}"),
                Ok(None) | Err(_) => break,
                _ => {}
            }
        }

        assert!(saw_delta, "expected at least one StreamDelta");
        assert!(saw_complete, "expected TurnComplete");
    }
}
