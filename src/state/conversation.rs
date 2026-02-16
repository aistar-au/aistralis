use crate::api::{stream::StreamParser, ApiClient};
use crate::tools::ToolExecutor;
use crate::types::{ApiMessage, Content, ContentBlock, StreamEvent};
use anyhow::Result;
use futures::StreamExt;
#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

pub struct ConversationManager {
    client: ApiClient,
    tool_executor: ToolExecutor,
    api_messages: Vec<ApiMessage>,
    #[cfg(test)]
    mock_tool_executor_responses: Option<Arc<Mutex<HashMap<String, String>>>>,
}

impl ConversationManager {
    pub fn new(client: ApiClient, executor: ToolExecutor) -> Self {
        Self {
            client,
            tool_executor: executor,
            api_messages: Vec::new(),
            #[cfg(test)]
            mock_tool_executor_responses: None,
        }
    }

    #[cfg(test)]
    pub fn new_mock(client: ApiClient, tool_executor_responses: HashMap<String, String>) -> Self {
        Self {
            client,
            tool_executor: ToolExecutor::new(std::path::PathBuf::from("/tmp")), // Dummy executor
            api_messages: Vec::new(),
            mock_tool_executor_responses: Some(Arc::new(Mutex::new(tool_executor_responses))),
        }
    }

    pub async fn send_message(
        &mut self,
        content: String,
        stream_delta_tx: Option<&mpsc::UnboundedSender<String>>,
    ) -> Result<String> {
        self.api_messages.push(ApiMessage {
            role: "user".to_string(),
            content: Content::Text(content),
        });

        loop {
            let mut stream = self.client.create_stream(&self.api_messages).await?;
            let mut parser = StreamParser::new();
            let mut assistant_text = String::new();
            let mut tool_use_blocks = Vec::new();
            let mut tool_input_buffers: Vec<Option<String>> = Vec::new();

            while let Some(chunk_result) = stream.next().await {
                let chunk = chunk_result?;
                let events = parser.process(&chunk)?;

                for event in events {
                    match event {
                        StreamEvent::ContentBlockStart {
                            index,
                            content_block,
                        } => {
                            if let ContentBlock::ToolUse { .. } = content_block {
                                while tool_use_blocks.len() <= index {
                                    tool_use_blocks.push(None);
                                    tool_input_buffers.push(None);
                                }
                                tool_use_blocks[index] = Some(content_block);
                                tool_input_buffers[index] = Some(String::new());
                            }
                        }
                        StreamEvent::ContentBlockDelta { index, delta } => {
                            if let Some(text) = delta.text {
                                assistant_text.push_str(&text);
                                if let Some(tx) = stream_delta_tx {
                                    let _ = tx.send(text);
                                }
                            }

                            if let Some(partial_json) = delta.partial_json {
                                let maybe_buffer = tool_input_buffers.get_mut(index);
                                if let Some(Some(buffer)) = maybe_buffer {
                                    buffer.push_str(&partial_json);
                                }
                            }
                        }
                        StreamEvent::ContentBlockStop { index } => {
                            let maybe_json = tool_input_buffers.get_mut(index);
                            let maybe_tool = tool_use_blocks.get_mut(index);

                            if let (
                                Some(Some(json_str)),
                                Some(Some(ContentBlock::ToolUse { input, .. })),
                            ) = (maybe_json, maybe_tool)
                            {
                                if !json_str.is_empty() {
                                    if let Ok(parsed_input) = serde_json::from_str(json_str) {
                                        *input = parsed_input;
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }

            let tool_use_blocks: Vec<ContentBlock> =
                tool_use_blocks.into_iter().flatten().collect();

            let mut assistant_content_blocks = Vec::new();
            if !assistant_text.is_empty() {
                assistant_content_blocks.push(ContentBlock::Text {
                    text: assistant_text.clone(),
                });
            }
            assistant_content_blocks.extend(tool_use_blocks.clone());

            self.api_messages.push(ApiMessage {
                role: "assistant".to_string(),
                content: Content::Blocks(assistant_content_blocks),
            });

            if tool_use_blocks.is_empty() {
                return Ok(assistant_text);
            }

            let mut tool_result_blocks = Vec::new();
            for block in tool_use_blocks {
                if let ContentBlock::ToolUse { id, name, input } = block {
                    let result = self.execute_tool(&name, &input).await;
                    tool_result_blocks.push(ContentBlock::ToolResult {
                        tool_use_id: id,
                        content: result.as_ref().map_or_else(
                            |e| format!("Error executing tool: {e}"),
                            ToString::to_string,
                        ),
                        is_error: result.is_err(),
                    });
                }
            }

            self.api_messages.push(ApiMessage {
                role: "user".to_string(),
                content: Content::Blocks(tool_result_blocks),
            });
        }
    }

    async fn execute_tool(&self, name: &str, input: &serde_json::Value) -> Result<String> {
        let get_str = |key: &str| input.get(key).and_then(|v| v.as_str()).unwrap_or("");
        let get_bool =
            |key: &str, default: bool| input.get(key).and_then(|v| v.as_bool()).unwrap_or(default);
        let get_usize = |key: &str, default: usize| {
            input
                .get(key)
                .and_then(|v| v.as_u64())
                .map(|v| v as usize)
                .unwrap_or(default)
        };

        #[cfg(test)]
        {
            if let Some(responses_arc) = &self.mock_tool_executor_responses {
                let responses = responses_arc.lock().unwrap();
                if name == "read_file" {
                    let path = get_str("path");
                    if let Some(content) = responses.get(path) {
                        return Ok(content.clone());
                    } else {
                        return Err(anyhow::anyhow!(
                            "Mock tool 'read_file' not configured for path: {}",
                            path
                        ));
                    }
                }
            }
        }
        match name {
            "read_file" => self.tool_executor.read_file(get_str("path")),
            "write_file" => self
                .tool_executor
                .write_file(get_str("path"), get_str("content"))
                .map(|_| format!("Successfully wrote to {}", get_str("path"))),
            "edit_file" => self
                .tool_executor
                .edit_file(get_str("path"), get_str("old_str"), get_str("new_str"))
                .map(|_| format!("Successfully edited {}", get_str("path"))),
            "git_status" => self.tool_executor.git_status(
                get_bool("short", true),
                input.get("path").and_then(|v| v.as_str()),
            ),
            "git_diff" => self.tool_executor.git_diff(
                get_bool("cached", false),
                input.get("path").and_then(|v| v.as_str()),
            ),
            "git_log" => self.tool_executor.git_log(get_usize("max_count", 10)),
            "git_show" => self.tool_executor.git_show(get_str("revision")),
            "git_add" => self.tool_executor.git_add(get_str("path")),
            "git_commit" => self.tool_executor.git_commit(get_str("message")),
            _ => Ok(format!("Unknown tool: {name}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::ApiClient;
    use serde_json::json;

    #[tokio::test]
    async fn test_crit_01_protocol_flow() -> Result<()> {
        // ANCHOR: This test verifies the multi-turn conversation protocol.
        // It will PASS if the protocol is correctly implemented.
        //
        // The test should:
        // 1. Create a ConversationManager with a mock client
        // 2. Send a message that triggers tool use
        // 3. Verify the tool is executed
        // 4. Verify the final response incorporates tool results

        // Mock responses for the API client
        let first_response_sse = vec![
            r#"event: message_start
data: {"type": "message_start", "message": {"id": "msg_mock_01", "type": "message", "role": "assistant", "model": "mock-model", "content": [], "stop_reason": null, "stop_sequence": null, "usage": {"input_tokens": 10, "output_tokens": 1}}}"#.to_string(),
            r#"event: content_block_start
data: {"type": "content_block_start", "index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
            r#"event: content_block_delta
data: {"type": "content_block_delta", "index":0,"delta":{"type":"text_delta","text":"Okay, I can help with that. "}}"#.to_string(),
            r#"event: content_block_start
data: {"type": "content_block_start", "index":1,"content_block":{"type":"tool_use","id":"toolu_mock_01", "name":"read_file","input":{}}}"#.to_string(),
            r#"event: content_block_delta
data: {"type": "content_block_delta", "index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\": \"file.txt\"}"}}"#.to_string(),
            r#"event: content_block_stop
data: {"type": "content_block_stop", "index":1}"#.to_string(),
            r#"event: message_delta
data: {"type": "message_delta", "delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":6}}"#.to_string(),
            r#"event: message_stop
data: {"type": "message_stop"}"#.to_string(),
        ];

        let second_response_sse = vec![
            r#"event: message_start
data: {"type": "message_start", "message": {"id": "msg_mock_02", "type": "message", "role": "assistant", "model": "mock-model", "content": [], "stop_reason": null, "stop_sequence": null, "usage": {"input_tokens": 10, "output_tokens": 1}}}"#.to_string(),
            r#"event: content_block_start
data: {"type": "content_block_start", "index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
            r#"event: content_block_delta
data: {"type": "content_block_delta", "index":0,"delta":{"type":"text_delta","text":"The content of file.txt is 'Hello from file.txt'"}}"#.to_string(),
            r#"event: message_delta
data: {"type": "message_delta", "delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":10}}"#.to_string(),
            r#"event: message_stop
data: {"type": "message_stop"}"#.to_string(),
        ];

        let mock_api_client =
            ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
                first_response_sse,
                second_response_sse,
            ])));

        let mut mock_tool_responses = HashMap::new();
        mock_tool_responses.insert("file.txt".to_string(), "Hello from file.txt".to_string());

        let mut manager = ConversationManager::new_mock(mock_api_client, mock_tool_responses);

        let final_text = manager
            .send_message("What is in file.txt?".into(), None)
            .await?;

        assert!(final_text.contains("The content of file.txt is 'Hello from file.txt'"));

        // Verify the message history order
        let messages = &manager.api_messages;
        assert_eq!(messages.len(), 4);

        // Initial user message
        assert_eq!(messages[0].role, "user");
        if let Content::Text(text) = &messages[0].content {
            assert!(text.contains("What is in file.txt?"));
        }

        // Assistant message with tool_use
        assert_eq!(messages[1].role, "assistant");
        if let Content::Blocks(blocks) = &messages[1].content {
            assert_eq!(blocks.len(), 2);
            if let ContentBlock::Text { text } = &blocks[0] {
                assert!(text.contains("Okay, I can help with that."));
            }
            if let ContentBlock::ToolUse { id: _, name, input } = &blocks[1] {
                assert_eq!(name, "read_file");
                assert_eq!(input, &json!({ "path": "file.txt" }));
            }
        }

        // User message with tool_result
        assert_eq!(messages[2].role, "user");
        if let Content::Blocks(blocks) = &messages[2].content {
            assert_eq!(blocks.len(), 1);
            if let ContentBlock::ToolResult {
                tool_use_id: _,
                content,
                is_error,
            } = &blocks[0]
            {
                assert!(content.contains("Hello from file.txt"));
                assert!(!is_error);
            }
        }

        // Final assistant message
        assert_eq!(messages[3].role, "assistant");
        if let Content::Blocks(blocks) = &messages[3].content {
            assert_eq!(blocks.len(), 1);
            if let ContentBlock::Text { text } = &blocks[0] {
                assert!(text.contains("The content of file.txt is 'Hello from file.txt'"));
            }
        }

        Ok(())
    }
}
