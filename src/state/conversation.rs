use crate::api::{stream::StreamParser, ApiClient};
use crate::tools::ToolExecutor;
use crate::types::{ApiMessage, Content, ContentBlock, StreamEvent};
use anyhow::Result;
use futures::StreamExt;
use std::collections::HashMap;

pub struct ConversationManager {
    client: ApiClient,
    tool_executor: ToolExecutor,
    api_messages: Vec<ApiMessage>,
}

impl ConversationManager {
    pub fn new(client: ApiClient, executor: ToolExecutor) -> Self {
        Self {
            client,
            tool_executor: executor,
            api_messages: Vec::new(),
        }
    }

    pub async fn send_message(&mut self, content: String) -> Result<String> {
        self.api_messages.push(ApiMessage {
            role: "user".to_string(),
            content: Content::Text(content),
        });

        loop {
            let mut stream = self.client.create_stream(&self.api_messages).await?;
            let mut parser = StreamParser::new();
            let mut assistant_text = String::new();
            let mut tool_use_blocks = Vec::new();

            while let Some(chunk_result) = stream.next().await {
                let chunk = chunk_result?;
                let events = parser.process(&chunk)?;

                for event in events {
                    match event {
                        StreamEvent::ContentBlockStart { content_block, .. } => {
                            if let ContentBlock::ToolUse { .. } = &content_block {
                                tool_use_blocks.push(content_block);
                            }
                        }
                        StreamEvent::ContentBlockDelta { delta, .. } => {
                            if let Some(text) = delta.text {
                                assistant_text.push_str(&text);
                            }
                        }
                        _ => {}
                    }
                }
            }

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
        let input_map: HashMap<String, serde_json::Value> = serde_json::from_value(input.clone())?;

        match name {
            "read_file" => {
                let path = input_map.get("path").and_then(|v| v.as_str()).unwrap_or("");
                self.tool_executor.read_file(path)
            }
            "write_file" => {
                let path = input_map.get("path").and_then(|v| v.as_str()).unwrap_or("");
                let content = input_map
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                self.tool_executor.write_file(path, content)?;
                Ok(format!("Successfully wrote to {path}"))
            }
            "edit_file" => {
                let path = input_map.get("path").and_then(|v| v.as_str()).unwrap_or("");
                let old_str = input_map
                    .get("old_str")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let new_str = input_map
                    .get("new_str")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                self.tool_executor.edit_file(path, old_str, new_str)?;
                Ok(format!("Successfully edited {path}"))
            }
            _ => Ok(format!("Unknown tool: {name}")),
        }
    }
}
