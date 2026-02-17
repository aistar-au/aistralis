use super::stream_block::{StreamBlock, ToolStatus};
use crate::api::{stream::StreamParser, ApiClient};
use crate::tools::ToolExecutor;
use crate::types::{ApiMessage, Content, ContentBlock, StreamEvent};
use anyhow::bail;
use anyhow::Result;
use futures::StreamExt;
#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};

pub enum ConversationStreamUpdate {
    Delta(String),
    BlockStart { index: usize, block: StreamBlock },
    BlockDelta { index: usize, delta: String },
    BlockComplete { index: usize },
    ToolApprovalRequest(ToolApprovalRequest),
}

pub struct ToolApprovalRequest {
    pub tool_name: String,
    pub input_preview: String,
    pub response_tx: oneshot::Sender<bool>,
}

const LOCAL_DEFAULT_MAX_ASSISTANT_HISTORY_CHARS: usize = 1_200;
const LOCAL_DEFAULT_MAX_TOOL_RESULT_HISTORY_CHARS: usize = 2_500;
const LOCAL_DEFAULT_MAX_API_MESSAGES: usize = 14;
const REMOTE_DEFAULT_MAX_ASSISTANT_HISTORY_CHARS: usize = 3_000;
const REMOTE_DEFAULT_MAX_TOOL_RESULT_HISTORY_CHARS: usize = 6_000;
const REMOTE_DEFAULT_MAX_API_MESSAGES: usize = 32;

#[derive(Clone, Copy)]
struct HistoryLimits {
    max_assistant_history_chars: usize,
    max_tool_result_history_chars: usize,
    max_api_messages: usize,
}

pub struct ConversationManager {
    client: ApiClient,
    tool_executor: ToolExecutor,
    api_messages: Vec<ApiMessage>,
    current_turn_blocks: Vec<StreamBlock>,
    #[cfg(test)]
    mock_tool_executor_responses: Option<Arc<Mutex<HashMap<String, String>>>>,
}

impl ConversationManager {
    pub fn new(client: ApiClient, executor: ToolExecutor) -> Self {
        Self {
            client,
            tool_executor: executor,
            api_messages: Vec::new(),
            current_turn_blocks: Vec::new(),
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
            current_turn_blocks: Vec::new(),
            mock_tool_executor_responses: Some(Arc::new(Mutex::new(tool_executor_responses))),
        }
    }

    pub async fn send_message(
        &mut self,
        content: String,
        stream_delta_tx: Option<&mpsc::UnboundedSender<ConversationStreamUpdate>>,
    ) -> Result<String> {
        self.current_turn_blocks.clear();
        self.api_messages.push(ApiMessage {
            role: "user".to_string(),
            content: Content::Text(content),
        });

        let use_structured_tool_protocol = self.client.supports_structured_tool_protocol();
        let use_structured_blocks = structured_blocks_enabled();
        let limits = resolve_history_limits(self.client.is_local_endpoint());
        let stream_server_events = stream_server_events_enabled();
        let stream_local_tool_events = stream_local_tool_events_enabled();
        let require_tool_approval = tool_approval_enabled();
        let mut rounds = 0usize;
        loop {
            self.current_turn_blocks.clear();
            self.prune_message_history(limits.max_api_messages);
            rounds += 1;
            if rounds > 24 {
                bail!("Exceeded max tool rounds (24). Possible tool-calling loop.");
            }

            let mut stream = self.client.create_stream(&self.api_messages).await?;
            let mut parser = StreamParser::new();
            let mut assistant_text = String::new();
            let mut tool_use_blocks = Vec::new();
            let mut tool_input_buffers: Vec<Option<String>> = Vec::new();
            let mut tool_input_event_emitted: Vec<bool> = Vec::new();

            while let Some(chunk_result) = stream.next().await {
                let chunk = chunk_result?;
                let events = parser.process(&chunk)?;

                for event in events {
                    match event {
                        StreamEvent::MessageStart { .. } => {
                            if !use_structured_blocks && stream_server_events {
                                emit_text_update(
                                    stream_delta_tx,
                                    "\n* Event: message_start\n".to_string(),
                                );
                            }
                        }
                        StreamEvent::ContentBlockStart {
                            index,
                            content_block,
                        } => {
                            if use_structured_blocks {
                                match &content_block {
                                    ContentBlock::Text { .. } => {
                                        self.upsert_turn_block(
                                            index,
                                            StreamBlock::Thinking {
                                                content: String::new(),
                                                collapsed: false,
                                            },
                                            stream_delta_tx,
                                        );
                                    }
                                    ContentBlock::ToolUse { id, name, input } => {
                                        self.upsert_turn_block(
                                            index,
                                            StreamBlock::ToolCall {
                                                id: id.clone(),
                                                name: name.clone(),
                                                input: input.clone(),
                                                status: ToolStatus::Pending,
                                            },
                                            stream_delta_tx,
                                        );
                                    }
                                    ContentBlock::ToolResult { .. } => {}
                                }
                            } else if stream_server_events {
                                let event_label = match &content_block {
                                    ContentBlock::Text { .. } => "\n* Thinking\n".to_string(),
                                    ContentBlock::ToolUse { name, .. } => {
                                        format!("\n* Tool: {name}\n")
                                    }
                                    ContentBlock::ToolResult { .. } => {
                                        format!("\n* Event: tool_result_block#{index}\n")
                                    }
                                };
                                emit_text_update(stream_delta_tx, event_label);
                            }

                            let tool_name =
                                if let ContentBlock::ToolUse { name, .. } = &content_block {
                                    Some(name.clone())
                                } else {
                                    None
                                };
                            if tool_name.is_some() {
                                while tool_use_blocks.len() <= index {
                                    tool_use_blocks.push(None);
                                    tool_input_buffers.push(None);
                                    tool_input_event_emitted.push(false);
                                }
                                tool_use_blocks[index] = Some(content_block);
                                tool_input_buffers[index] = Some(String::new());
                            }
                        }
                        StreamEvent::ContentBlockDelta { index, delta } => {
                            if let Some(text) = delta.text {
                                if use_structured_blocks {
                                    let appended =
                                        self.append_text_delta(index, &text, stream_delta_tx);
                                    assistant_text.push_str(&appended);
                                } else {
                                    assistant_text.push_str(&text);
                                    emit_text_update(stream_delta_tx, text);
                                }
                            }

                            if let Some(partial_json) = delta.partial_json {
                                let maybe_buffer = tool_input_buffers.get_mut(index);
                                if let Some(Some(buffer)) = maybe_buffer {
                                    buffer.push_str(&partial_json);

                                    if use_structured_blocks {
                                        if let Ok(parsed_input) =
                                            serde_json::from_str::<serde_json::Value>(buffer)
                                        {
                                            if let Some(StreamBlock::ToolCall { input, .. }) =
                                                self.current_turn_blocks.get_mut(index)
                                            {
                                                *input = parsed_input;
                                            }
                                        }
                                        emit_stream_update(
                                            stream_delta_tx,
                                            ConversationStreamUpdate::BlockDelta {
                                                index,
                                                delta: partial_json.clone(),
                                            },
                                        );
                                    }
                                }
                                if !use_structured_blocks && stream_server_events {
                                    let should_emit = tool_input_event_emitted
                                        .get(index)
                                        .map(|emitted| !*emitted)
                                        .unwrap_or(false);
                                    if should_emit {
                                        emit_text_update(
                                            stream_delta_tx,
                                            format!("\n* Event: input_json#{index}\n"),
                                        );
                                        if let Some(flag) = tool_input_event_emitted.get_mut(index)
                                        {
                                            *flag = true;
                                        }
                                    }
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

                                        if let Some(StreamBlock::ToolCall {
                                            input: block_input,
                                            ..
                                        }) = self.current_turn_blocks.get_mut(index)
                                        {
                                            *block_input = input.clone();
                                        }
                                    }
                                }
                            }
                            if use_structured_blocks {
                                emit_stream_update(
                                    stream_delta_tx,
                                    ConversationStreamUpdate::BlockComplete { index },
                                );
                            }
                        }
                        StreamEvent::MessageDelta { delta } => {
                            if !use_structured_blocks && stream_server_events {
                                let stop_reason =
                                    delta.stop_reason.unwrap_or_else(|| "none".to_string());
                                emit_text_update(
                                    stream_delta_tx,
                                    format!("\n* Event: stop_reason={stop_reason}\n"),
                                );
                            }
                        }
                        StreamEvent::MessageStop => {
                            if !use_structured_blocks && stream_server_events {
                                emit_text_update(
                                    stream_delta_tx,
                                    "\n* Event: message_stop\n".to_string(),
                                );
                            }
                        }
                        StreamEvent::Unknown => {
                            if !use_structured_blocks && stream_server_events {
                                emit_text_update(
                                    stream_delta_tx,
                                    "\n* Event: unknown\n".to_string(),
                                );
                            }
                        }
                    }
                }
            }

            let mut tool_use_blocks: Vec<ContentBlock> =
                tool_use_blocks.into_iter().flatten().collect();
            let mut tagged_fallback_used = false;
            if tool_use_blocks.is_empty() {
                let tagged_calls = parse_tagged_tool_calls(&assistant_text);
                if !tagged_calls.is_empty() {
                    tagged_fallback_used = true;
                    for (idx, call) in tagged_calls.into_iter().enumerate() {
                        tool_use_blocks.push(ContentBlock::ToolUse {
                            id: format!("toolu_fallback_{rounds}_{idx}"),
                            name: call.name,
                            input: call.input,
                        });
                    }
                }
            }

            let assistant_history_text = if assistant_text.is_empty() && !tool_use_blocks.is_empty()
            {
                render_tool_calls_for_text_protocol(&tool_use_blocks)
            } else {
                assistant_text.clone()
            };
            let assistant_history_text =
                truncate_for_history(&assistant_history_text, limits.max_assistant_history_chars);

            let use_structured_round = use_structured_tool_protocol && !tagged_fallback_used;

            if use_structured_round {
                let mut assistant_content_blocks = Vec::new();
                if !assistant_text.is_empty() {
                    assistant_content_blocks.push(ContentBlock::Text {
                        text: truncate_for_history(
                            &assistant_text,
                            limits.max_assistant_history_chars,
                        ),
                    });
                }
                assistant_content_blocks.extend(tool_use_blocks.clone());

                self.api_messages.push(ApiMessage {
                    role: "assistant".to_string(),
                    content: Content::Blocks(assistant_content_blocks),
                });
            } else {
                self.api_messages.push(ApiMessage {
                    role: "assistant".to_string(),
                    content: Content::Text(assistant_history_text),
                });
            }

            if tool_use_blocks.is_empty() {
                if use_structured_blocks {
                    self.promote_thinking_blocks_to_final_text();
                }
                return Ok(assistant_text);
            }

            let mut tool_result_blocks = Vec::new();
            let mut text_protocol_tool_results = Vec::new();
            for block in tool_use_blocks {
                if let ContentBlock::ToolUse { id, name, input } = block {
                    if use_structured_blocks && require_tool_approval {
                        self.set_tool_call_status(
                            &id,
                            ToolStatus::WaitingApproval,
                            stream_delta_tx,
                        );
                    }
                    let approved = if require_tool_approval {
                        self.request_tool_approval(&name, &input, stream_delta_tx)
                            .await
                    } else {
                        true
                    };

                    if use_structured_blocks {
                        if approved {
                            self.set_tool_call_status(&id, ToolStatus::Executing, stream_delta_tx);
                        } else {
                            self.set_tool_call_status(&id, ToolStatus::Cancelled, stream_delta_tx);
                        }
                    }

                    let result = if approved {
                        self.execute_tool(&name, &input).await
                    } else {
                        Err(anyhow::anyhow!("Tool execution cancelled by user"))
                    };
                    if use_structured_blocks {
                        let final_status = if approved {
                            ToolStatus::Complete
                        } else {
                            ToolStatus::Cancelled
                        };
                        self.set_tool_call_status(&id, final_status, stream_delta_tx);

                        let output_for_stream = result
                            .as_ref()
                            .map_or_else(|e| e.to_string(), ToString::to_string);
                        self.push_tool_result_block(
                            StreamBlock::ToolResult {
                                tool_call_id: id.clone(),
                                output: output_for_stream,
                                is_error: result.is_err(),
                            },
                            stream_delta_tx,
                        );
                    } else if stream_local_tool_events {
                        match &result {
                            Ok(_) => {
                                emit_text_update(
                                    stream_delta_tx,
                                    format!("\n+ [tool_result] {name}\n"),
                                );
                            }
                            Err(error) => {
                                emit_text_update(
                                    stream_delta_tx,
                                    format!("\n- [tool_error] {name}: {error}\n"),
                                );
                            }
                        }
                    }

                    if use_structured_round {
                        let history_content = truncate_for_history(
                            &result.as_ref().map_or_else(
                                |e| format!("Error executing tool: {e}"),
                                ToString::to_string,
                            ),
                            limits.max_tool_result_history_chars,
                        );
                        tool_result_blocks.push(ContentBlock::ToolResult {
                            tool_use_id: id,
                            content: history_content,
                            is_error: result.is_err(),
                        });
                    } else {
                        let rendered = result.as_ref().map_or_else(
                            |e| format!("tool_error {name}:\n{e}"),
                            |output| format!("tool_result {name}:\n{output}"),
                        );
                        text_protocol_tool_results.push(truncate_for_history(
                            &rendered,
                            limits.max_tool_result_history_chars,
                        ));
                    }
                }
            }

            if use_structured_round {
                self.api_messages.push(ApiMessage {
                    role: "user".to_string(),
                    content: Content::Blocks(tool_result_blocks),
                });
            } else {
                self.api_messages.push(ApiMessage {
                    role: "user".to_string(),
                    content: Content::Text(text_protocol_tool_results.join("\n\n")),
                });
            }
        }
    }

    async fn request_tool_approval(
        &self,
        name: &str,
        input: &serde_json::Value,
        stream_delta_tx: Option<&mpsc::UnboundedSender<ConversationStreamUpdate>>,
    ) -> bool {
        let Some(tx) = stream_delta_tx else {
            return true;
        };

        let (response_tx, response_rx) = oneshot::channel();
        let request = ToolApprovalRequest {
            tool_name: name.to_string(),
            input_preview: tool_input_preview(name, input),
            response_tx,
        };

        if tx
            .send(ConversationStreamUpdate::ToolApprovalRequest(request))
            .is_err()
        {
            return false;
        }

        response_rx.await.unwrap_or(false)
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
            "rename_file" => self
                .tool_executor
                .rename_file(get_str("old_path"), get_str("new_path")),
            "list_files" | "list_directory" => self.tool_executor.list_files(
                input.get("path").and_then(|v| v.as_str()),
                get_usize("max_entries", 100),
            ),
            "search_files" | "search" => self.tool_executor.search_files(
                get_str("query"),
                input.get("path").and_then(|v| v.as_str()),
                get_usize("max_results", 30),
            ),
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

    fn upsert_turn_block(
        &mut self,
        index: usize,
        block: StreamBlock,
        stream_delta_tx: Option<&mpsc::UnboundedSender<ConversationStreamUpdate>>,
    ) {
        while self.current_turn_blocks.len() < index {
            self.current_turn_blocks.push(StreamBlock::Thinking {
                content: String::new(),
                collapsed: true,
            });
        }

        if index < self.current_turn_blocks.len() {
            self.current_turn_blocks[index] = block.clone();
        } else {
            self.current_turn_blocks.push(block.clone());
        }

        emit_stream_update(
            stream_delta_tx,
            ConversationStreamUpdate::BlockStart { index, block },
        );
    }

    fn append_text_delta(
        &mut self,
        index: usize,
        text: &str,
        stream_delta_tx: Option<&mpsc::UnboundedSender<ConversationStreamUpdate>>,
    ) -> String {
        let mut appended = String::new();

        if let Some(StreamBlock::Thinking { content, .. }) = self.current_turn_blocks.get_mut(index)
        {
            appended = append_incremental_suffix(content, text);
        } else if index >= self.current_turn_blocks.len() {
            appended = text.to_string();
            self.upsert_turn_block(
                index,
                StreamBlock::Thinking {
                    content: text.to_string(),
                    collapsed: false,
                },
                stream_delta_tx,
            );
        }

        if !appended.is_empty() {
            emit_stream_update(
                stream_delta_tx,
                ConversationStreamUpdate::BlockDelta {
                    index,
                    delta: appended.clone(),
                },
            );
        }

        appended
    }

    fn set_tool_call_status(
        &mut self,
        tool_call_id: &str,
        status: ToolStatus,
        stream_delta_tx: Option<&mpsc::UnboundedSender<ConversationStreamUpdate>>,
    ) {
        if let Some((index, block)) =
            self.current_turn_blocks
                .iter_mut()
                .enumerate()
                .find(|(_, block)| {
                    matches!(
                        block,
                        StreamBlock::ToolCall { id, .. } if id == tool_call_id
                    )
                })
        {
            if let StreamBlock::ToolCall {
                status: current, ..
            } = block
            {
                *current = status;
            }

            emit_stream_update(
                stream_delta_tx,
                ConversationStreamUpdate::BlockStart {
                    index,
                    block: block.clone(),
                },
            );
        }
    }

    fn push_tool_result_block(
        &mut self,
        block: StreamBlock,
        stream_delta_tx: Option<&mpsc::UnboundedSender<ConversationStreamUpdate>>,
    ) {
        let index = self.current_turn_blocks.len();
        self.current_turn_blocks.push(block.clone());
        emit_stream_update(
            stream_delta_tx,
            ConversationStreamUpdate::BlockStart { index, block },
        );
        emit_stream_update(
            stream_delta_tx,
            ConversationStreamUpdate::BlockComplete { index },
        );
    }

    fn promote_thinking_blocks_to_final_text(&mut self) {
        for block in &mut self.current_turn_blocks {
            if let StreamBlock::Thinking { content, .. } = block {
                *block = StreamBlock::FinalText {
                    content: content.clone(),
                };
            }
        }
    }

    fn prune_message_history(&mut self, max_api_messages: usize) {
        if self.api_messages.len() <= max_api_messages {
            return;
        }
        let to_drop = self.api_messages.len() - max_api_messages;
        self.api_messages.drain(0..to_drop);
    }
}

fn emit_stream_update(
    stream_delta_tx: Option<&mpsc::UnboundedSender<ConversationStreamUpdate>>,
    update: ConversationStreamUpdate,
) {
    if let Some(tx) = stream_delta_tx {
        let _ = tx.send(update);
    }
}

fn emit_text_update(
    stream_delta_tx: Option<&mpsc::UnboundedSender<ConversationStreamUpdate>>,
    text: String,
) {
    emit_stream_update(stream_delta_tx, ConversationStreamUpdate::Delta(text));
}

fn structured_blocks_enabled() -> bool {
    std::env::var("AISTAR_USE_STRUCTURED_BLOCKS")
        .ok()
        .and_then(parse_bool_flag)
        .unwrap_or(true)
}

fn append_incremental_suffix(existing: &mut String, incoming: &str) -> String {
    if incoming.is_empty() {
        return String::new();
    }
    if existing.is_empty() {
        existing.push_str(incoming);
        return incoming.to_string();
    }
    if existing == incoming {
        return String::new();
    }
    if incoming.starts_with(existing.as_str()) {
        let suffix = incoming[existing.len()..].to_string();
        existing.clear();
        existing.push_str(incoming);
        return suffix;
    }
    if existing.starts_with(incoming) || existing.ends_with(incoming) {
        return String::new();
    }

    let max_overlap = existing.len().min(incoming.len());
    let mut overlap = 0usize;
    for idx in 1..=max_overlap {
        if existing.ends_with(&incoming[..idx]) {
            overlap = idx;
        }
    }
    let suffix = incoming[overlap..].to_string();
    if !suffix.is_empty() {
        existing.push_str(&suffix);
    }
    suffix
}

fn resolve_history_limits(is_local_endpoint: bool) -> HistoryLimits {
    let defaults = if is_local_endpoint {
        HistoryLimits {
            max_assistant_history_chars: LOCAL_DEFAULT_MAX_ASSISTANT_HISTORY_CHARS,
            max_tool_result_history_chars: LOCAL_DEFAULT_MAX_TOOL_RESULT_HISTORY_CHARS,
            max_api_messages: LOCAL_DEFAULT_MAX_API_MESSAGES,
        }
    } else {
        HistoryLimits {
            max_assistant_history_chars: REMOTE_DEFAULT_MAX_ASSISTANT_HISTORY_CHARS,
            max_tool_result_history_chars: REMOTE_DEFAULT_MAX_TOOL_RESULT_HISTORY_CHARS,
            max_api_messages: REMOTE_DEFAULT_MAX_API_MESSAGES,
        }
    };

    HistoryLimits {
        max_assistant_history_chars: env_override_usize(
            "AISTAR_MAX_ASSISTANT_HISTORY_CHARS",
            defaults.max_assistant_history_chars,
            200,
            20_000,
        ),
        max_tool_result_history_chars: env_override_usize(
            "AISTAR_MAX_TOOL_RESULT_HISTORY_CHARS",
            defaults.max_tool_result_history_chars,
            200,
            40_000,
        ),
        max_api_messages: env_override_usize(
            "AISTAR_MAX_API_MESSAGES",
            defaults.max_api_messages,
            4,
            128,
        ),
    }
}

fn env_override_usize(key: &str, default: usize, min: usize, max: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .map(|v| v.clamp(min, max))
        .unwrap_or(default)
}

fn parse_bool_flag(value: String) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn stream_local_tool_events_enabled() -> bool {
    std::env::var("AISTAR_STREAM_LOCAL_TOOL_EVENTS")
        .ok()
        .and_then(parse_bool_flag)
        .unwrap_or(false)
}

fn tool_approval_enabled() -> bool {
    std::env::var("AISTAR_TOOL_CONFIRM")
        .ok()
        .and_then(parse_bool_flag)
        .unwrap_or(true)
}

fn stream_server_events_enabled() -> bool {
    std::env::var("AISTAR_STREAM_SERVER_EVENTS")
        .ok()
        .and_then(parse_bool_flag)
        .unwrap_or(true)
}

fn tool_input_preview(tool_name: &str, input: &serde_json::Value) -> String {
    match tool_name {
        "edit_file" => preview_edit_file_input(input),
        "write_file" => preview_write_file_input(input),
        _ => serde_json::to_string_pretty(input).unwrap_or_else(|_| input.to_string()),
    }
}

fn preview_edit_file_input(input: &serde_json::Value) -> String {
    let path = input
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or("<missing>");
    let old_str = input.get("old_str").and_then(|v| v.as_str()).unwrap_or("");
    let new_str = input.get("new_str").and_then(|v| v.as_str()).unwrap_or("");

    let mut out = String::new();
    out.push_str(&format!("path: {path}\n"));
    out.push_str(&format!(
        "old_str: {} chars, {} lines\n",
        old_str.chars().count(),
        old_str
            .lines()
            .count()
            .max(usize::from(!old_str.is_empty()))
    ));
    out.push_str(&preview_lines('-', old_str, usize::MAX, 1));
    out.push_str(&format!(
        "new_str: {} chars, {} lines\n",
        new_str.chars().count(),
        new_str
            .lines()
            .count()
            .max(usize::from(!new_str.is_empty()))
    ));
    out.push_str(&preview_lines('+', new_str, usize::MAX, 1));
    out
}

fn preview_write_file_input(input: &serde_json::Value) -> String {
    let path = input
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or("<missing>");
    let content = input.get("content").and_then(|v| v.as_str()).unwrap_or("");

    let mut out = String::new();
    out.push_str(&format!("path: {path}\n"));
    out.push_str(&format!(
        "content: {} chars, {} lines\n",
        content.chars().count(),
        content
            .lines()
            .count()
            .max(usize::from(!content.is_empty()))
    ));
    out.push_str(&preview_lines('+', content, usize::MAX, 1));
    out
}

fn preview_lines(prefix: char, text: &str, max_lines: usize, start_line: usize) -> String {
    if text.is_empty() {
        return format!("  {start_line} {prefix} <empty>\n");
    }

    let mut out = String::new();
    let lines: Vec<&str> = text.lines().collect();
    for (idx, line) in lines.iter().take(max_lines).enumerate() {
        let line_number = start_line + idx;
        out.push_str(&format!("  {line_number} {prefix} {line}\n"));
    }
    if lines.len() > max_lines {
        out.push_str(&format!("  ... ({} more lines)\n", lines.len() - max_lines));
    }
    out
}

#[derive(Debug, Clone)]
struct TaggedToolCall {
    name: String,
    input: serde_json::Value,
}

fn parse_tagged_tool_calls(text: &str) -> Vec<TaggedToolCall> {
    let mut calls = Vec::new();
    let mut cursor = 0usize;

    while let Some(function_rel) = text[cursor..].find("<function=") {
        let function_start = cursor + function_rel;
        let name_start = function_start + "<function=".len();
        let Some(name_end_rel) = text[name_start..].find('>') else {
            break;
        };
        let name_end = name_start + name_end_rel;
        let function_name = text[name_start..name_end]
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .to_string();

        let body_start = name_end + 1;
        let (body_end, next_cursor) = find_function_body_bounds(text, body_start);
        let body = &text[body_start..body_end];

        let input = parse_tagged_parameters(body);

        if !function_name.is_empty() {
            calls.push(TaggedToolCall {
                name: function_name,
                input: serde_json::Value::Object(input),
            });
        }

        cursor = next_cursor.max(function_start + 1);
    }

    calls
}

fn find_function_body_bounds(text: &str, body_start: usize) -> (usize, usize) {
    let function_close = text[body_start..]
        .find("</function>")
        .map(|rel| body_start + rel);
    let next_function = text[body_start..]
        .find("<function=")
        .map(|rel| body_start + rel);

    match (function_close, next_function) {
        (Some(close), Some(next)) if next < close => (next, next),
        (Some(close), _) => (close, close + "</function>".len()),
        (None, Some(next)) => (next, next),
        (None, None) => (text.len(), text.len()),
    }
}

fn parse_tagged_parameters(body: &str) -> serde_json::Map<String, serde_json::Value> {
    let mut input = serde_json::Map::new();
    let mut parameter_cursor = 0usize;

    while let Some(parameter_rel) = body[parameter_cursor..].find("<parameter=") {
        let parameter_start = parameter_cursor + parameter_rel;
        let key_start = parameter_start + "<parameter=".len();
        let Some(key_end_rel) = body[key_start..].find('>') else {
            break;
        };
        let key_end = key_start + key_end_rel;
        let key = body[key_start..key_end]
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .to_string();

        let value_start = key_end + 1;
        let parameter_close = body[value_start..]
            .find("</parameter>")
            .map(|rel| value_start + rel);
        let next_parameter = body[value_start..]
            .find("<parameter=")
            .map(|rel| value_start + rel);

        let (value_end, next_cursor) = match (parameter_close, next_parameter) {
            (Some(close), Some(next)) if next < close => (next, next),
            (Some(close), _) => (close, close + "</parameter>".len()),
            (None, Some(next)) => (next, next),
            (None, None) => (body.len(), body.len()),
        };

        let value = normalize_tagged_parameter_value(&body[value_start..value_end]);
        if !key.is_empty() {
            input.insert(key, serde_json::Value::String(value));
        }

        parameter_cursor = next_cursor.max(parameter_start + 1);
    }

    input
}

fn render_tool_calls_for_text_protocol(blocks: &[ContentBlock]) -> String {
    let mut out = String::new();
    for block in blocks {
        if let ContentBlock::ToolUse { name, input, .. } = block {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&format!("<function={name}>\n"));

            if let Some(obj) = input.as_object() {
                let mut keys: Vec<_> = obj.keys().collect();
                keys.sort_unstable();
                for key in keys {
                    let value = obj
                        .get(key)
                        .map(json_value_to_text_protocol_value)
                        .unwrap_or_default();
                    out.push_str(&format!("<parameter={key}>\n{value}\n</parameter>\n"));
                }
            }

            out.push_str("</function>");
        }
    }
    out
}

fn json_value_to_text_protocol_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        _ => value.to_string(),
    }
}

fn normalize_tagged_parameter_value(raw: &str) -> String {
    let mut value = raw.replace("\r\n", "\n");
    if value.starts_with('\n') {
        value.remove(0);
    }
    if value.ends_with('\n') {
        value.pop();
    }
    value
}

fn truncate_for_history(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }

    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        return text.to_string();
    }

    let indicator = format!("\n...[truncated {} chars]", chars.len() - max_chars);
    let keep = max_chars.saturating_sub(indicator.chars().count());
    let mut out: String = chars.into_iter().take(keep).collect();
    out.push_str(&indicator);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::ApiClient;
    use serde_json::json;
    use tempfile::TempDir;

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

    #[test]
    fn test_parse_tagged_tool_calls() {
        let text = r#"I can do this.
<function=write_file>
<parameter=path>
cal.rs
</parameter>
<parameter=content>
fn main() {}
</parameter>
</function>"#;

        let calls = parse_tagged_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "write_file");
        assert_eq!(calls[0].input["path"], "cal.rs");
        assert_eq!(calls[0].input["content"], "fn main() {}");
    }

    #[test]
    fn test_parse_tagged_tool_calls_without_parameters() {
        let text = "Checking files.\n<function=list_files></function>";
        let calls = parse_tagged_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "list_files");
        assert_eq!(calls[0].input, json!({}));
    }

    #[test]
    fn test_parse_tagged_tool_calls_with_missing_closing_tags() {
        let text = r#"I'll check it.
<function=read_file>
<parameter=path>
cal.js
"#;
        let calls = parse_tagged_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(calls[0].input["path"], "cal.js");
    }

    #[test]
    fn test_truncate_for_history() {
        let text = "abcdefghij";
        let truncated = truncate_for_history(text, 40);
        assert_eq!(truncated, text);

        let truncated = truncate_for_history(text, 5);
        assert!(truncated.contains("[truncated"));
    }

    #[tokio::test]
    async fn test_text_tagged_tool_call_fallback_flow() -> Result<()> {
        let first_response_sse = vec![
            r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_mock_10","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
            r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
            r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"I'll read it.\n<function=read_file>\n<parameter=path>\nfile.txt\n</parameter>\n</function>"}}"#.to_string(),
            r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}"#.to_string(),
            r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
        ];

        let second_response_sse = vec![
            r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_mock_11","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
            r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
            r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"The content is Hello from fallback."}}"#.to_string(),
            r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":8}}"#.to_string(),
            r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
        ];

        let mock_api_client =
            ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
                first_response_sse,
                second_response_sse,
            ])));

        let mut mock_tool_responses = HashMap::new();
        mock_tool_responses.insert("file.txt".to_string(), "Hello from fallback.".to_string());
        let mut manager = ConversationManager::new_mock(mock_api_client, mock_tool_responses);

        let final_text = manager.send_message("Read file".into(), None).await?;
        assert!(final_text.contains("Hello from fallback."));

        let messages = &manager.api_messages;
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[1].role, "assistant");
        if let Content::Text(text) = &messages[1].content {
            assert!(text.contains("<function=read_file>"));
        } else {
            panic!("expected assistant text fallback content");
        }

        assert_eq!(messages[2].role, "user");
        if let Content::Text(text) = &messages[2].content {
            assert!(text.contains("tool_result read_file:"));
        } else {
            panic!("expected user text tool result");
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_openai_stream_tool_call_round_trip() -> Result<()> {
        let first_response_sse = vec![
            r#"data: {"id":"chatcmpl_mock_1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","content":"I'll read it. "},"finish_reason":null}]}"#.to_string(),
            r#"data: {"id":"chatcmpl_mock_1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_mock_1","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"file.txt\"}"}}]},"finish_reason":"tool_calls"}]}"#.to_string(),
            "data: [DONE]".to_string(),
        ];

        let second_response_sse = vec![
            r#"data: {"id":"chatcmpl_mock_2","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","content":"The content is Hello from OpenAI stream."},"finish_reason":"stop"}]}"#.to_string(),
            "data: [DONE]".to_string(),
        ];

        let mock_api_client =
            ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
                first_response_sse,
                second_response_sse,
            ])));

        let mut mock_tool_responses = HashMap::new();
        mock_tool_responses.insert(
            "file.txt".to_string(),
            "Hello from OpenAI stream.".to_string(),
        );
        let mut manager = ConversationManager::new_mock(mock_api_client, mock_tool_responses);

        let final_text = manager.send_message("Read file".into(), None).await?;
        assert!(final_text.contains("Hello from OpenAI stream."));

        let messages = &manager.api_messages;
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[1].role, "assistant");
        if let Content::Blocks(blocks) = &messages[1].content {
            assert!(blocks.iter().any(
                |block| matches!(block, ContentBlock::ToolUse { name, .. } if name == "read_file")
            ));
        } else {
            panic!("expected assistant blocks");
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_local_text_protocol_tool_round_trip() -> Result<()> {
        let first_response_sse = vec![
            r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_mock_local_10","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
            r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
            r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"I will read it.\n<function=read_file>\n<parameter=path>\nfile.txt\n</parameter>\n"}}"#.to_string(),
            r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}"#.to_string(),
            r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
        ];

        let second_response_sse = vec![
            r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_mock_local_11","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
            r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
            r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Done: Hello local text protocol."}}"#.to_string(),
            r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":8}}"#.to_string(),
            r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
        ];

        let mock_api_client =
            ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
                first_response_sse,
                second_response_sse,
            ])))
            .with_structured_tool_protocol(false);

        let mut mock_tool_responses = HashMap::new();
        mock_tool_responses.insert(
            "file.txt".to_string(),
            "Hello local text protocol.".to_string(),
        );
        let mut manager = ConversationManager::new_mock(mock_api_client, mock_tool_responses);

        let final_text = manager.send_message("Read file".into(), None).await?;
        assert!(final_text.contains("Hello local text protocol."));

        let messages = &manager.api_messages;
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[1].role, "assistant");
        match &messages[1].content {
            Content::Text(text) => {
                assert!(text.contains("<function=read_file>"));
            }
            _ => panic!("expected assistant text content in local text protocol"),
        }

        assert_eq!(messages[2].role, "user");
        match &messages[2].content {
            Content::Text(text) => {
                assert!(text.contains("tool_result read_file:"));
                assert!(text.contains("Hello local text protocol."));
            }
            _ => panic!("expected user text tool result in local text protocol"),
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_tool_use_without_input_then_partial_json_executes_write_file() -> Result<()> {
        let temp = TempDir::new()?;

        let first_response_sse = vec![
            r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_mock_20","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
            r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
            r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Saving now."}}"#.to_string(),
            r#"event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_mock_write_1","name":"write_file"}}"#.to_string(),
            r#"event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"cal.rs\",\"content\":\"fn main() {}\\n\"}"}}"#.to_string(),
            r#"event: content_block_stop
data: {"type":"content_block_stop","index":1}"#.to_string(),
            r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":12}}"#.to_string(),
            r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
        ];

        let second_response_sse = vec![
            r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_mock_21","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
            r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
            r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Saved cal.rs."}}"#.to_string(),
            r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":5}}"#.to_string(),
            r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
        ];

        let mock_api_client =
            ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
                first_response_sse,
                second_response_sse,
            ])));

        let executor = ToolExecutor::new(temp.path().to_path_buf());
        let mut manager = ConversationManager::new(mock_api_client, executor);

        let final_text = manager
            .send_message("create calculator".to_string(), None)
            .await?;
        assert!(final_text.contains("Saved cal.rs."));

        let written = std::fs::read_to_string(temp.path().join("cal.rs"))?;
        assert_eq!(written, "fn main() {}\n");

        Ok(())
    }

    #[test]
    fn test_append_incremental_suffix_snapshot_streaming() {
        let mut content = String::new();
        let a = append_incremental_suffix(&mut content, "Hello");
        let b = append_incremental_suffix(&mut content, "Hello world");
        let c = append_incremental_suffix(&mut content, "Hello world");
        let d = append_incremental_suffix(&mut content, "Hello world!");

        assert_eq!(a, "Hello");
        assert_eq!(b, " world");
        assert_eq!(c, "");
        assert_eq!(d, "!");
        assert_eq!(content, "Hello world!");
    }
}
