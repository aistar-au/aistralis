use super::stream_block::{StreamBlock, ToolStatus};
use crate::api::{stream::StreamParser, ApiClient};
use crate::edit_diff::DEFAULT_EDIT_DIFF_CONTEXT_LINES;
use crate::runtime::parse_bool_flag;
use crate::tool_preview::{
    format_read_file_snapshot_message, preview_tool_input, read_file_path, ReadFileSnapshotCache,
    ReadFileSummaryMessageStyle, ToolPreviewStyle,
};
use crate::tools::ToolExecutor;
use crate::types::{ApiMessage, Content, ContentBlock, StreamEvent};
use anyhow::bail;
use anyhow::Result;
use futures::StreamExt;
use std::collections::BTreeSet;
#[cfg(test)]
use std::collections::HashMap;
use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;
use std::time::Duration;
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

#[cfg(test)]
impl ToolApprovalRequest {
    pub fn test_stub() -> Self {
        let (response_tx, _response_rx) = oneshot::channel::<bool>();
        Self {
            tool_name: "read_file".to_string(),
            input_preview: "{}".to_string(),
            response_tx,
        }
    }
}

const LOCAL_DEFAULT_MAX_ASSISTANT_HISTORY_CHARS: usize = 1_200;
const LOCAL_DEFAULT_MAX_TOOL_RESULT_HISTORY_CHARS: usize = 2_500;
const LOCAL_DEFAULT_MAX_API_MESSAGES: usize = 14;
const LOCAL_DEFAULT_TOOL_TIMEOUT_SECS: u64 = 20;
const REMOTE_DEFAULT_MAX_ASSISTANT_HISTORY_CHARS: usize = 3_000;
const REMOTE_DEFAULT_MAX_TOOL_RESULT_HISTORY_CHARS: usize = 6_000;
const REMOTE_DEFAULT_MAX_API_MESSAGES: usize = 32;
const REMOTE_DEFAULT_TOOL_TIMEOUT_SECS: u64 = 60;

#[derive(Clone, Copy)]
struct HistoryLimits {
    max_assistant_history_chars: usize,
    max_tool_result_history_chars: usize,
    max_api_messages: usize,
}

pub struct ConversationManager {
    client: Arc<ApiClient>,
    tool_executor: ToolExecutor,
    api_messages: Vec<ApiMessage>,
    current_turn_blocks: Vec<StreamBlock>,
    read_file_history_cache: ReadFileSnapshotCache,
    #[cfg(test)]
    mock_tool_executor_responses: Option<Arc<Mutex<HashMap<String, String>>>>,
}

impl ConversationManager {
    pub fn new(client: ApiClient, executor: ToolExecutor) -> Self {
        Self {
            client: Arc::new(client),
            tool_executor: executor,
            api_messages: Vec::new(),
            current_turn_blocks: Vec::new(),
            read_file_history_cache: ReadFileSnapshotCache::default(),
            #[cfg(test)]
            mock_tool_executor_responses: None,
        }
    }

    #[cfg(test)]
    pub fn new_mock(client: ApiClient, tool_executor_responses: HashMap<String, String>) -> Self {
        Self {
            client: Arc::new(client),
            tool_executor: ToolExecutor::new(std::path::PathBuf::from("/tmp")), // Dummy executor
            api_messages: Vec::new(),
            current_turn_blocks: Vec::new(),
            read_file_history_cache: ReadFileSnapshotCache::default(),
            mock_tool_executor_responses: Some(Arc::new(Mutex::new(tool_executor_responses))),
        }
    }

    pub fn push_user_message(&mut self, input: String) {
        self.api_messages.push(ApiMessage {
            role: "user".to_string(),
            content: Content::Text(input),
        });
    }

    pub fn messages_for_api(&self) -> Vec<ApiMessage> {
        self.api_messages.clone()
    }

    pub fn client(&self) -> Arc<ApiClient> {
        Arc::clone(&self.client)
    }

    pub async fn send_message(
        &mut self,
        content: String,
        stream_delta_tx: Option<&mpsc::UnboundedSender<ConversationStreamUpdate>>,
    ) -> Result<String> {
        self.current_turn_blocks.clear();
        self.push_user_message(content);

        let use_structured_tool_protocol = self.client.supports_structured_tool_protocol();
        let use_structured_blocks = structured_blocks_enabled();
        let limits = resolve_history_limits(self.client.is_local_endpoint());
        let tool_timeout = resolve_tool_timeout(self.client.is_local_endpoint());
        let max_tool_rounds = resolve_max_tool_rounds(self.client.is_local_endpoint());
        let stream_server_events = stream_server_events_enabled();
        let stream_local_tool_events = stream_local_tool_events_enabled();
        let require_tool_approval = tool_approval_enabled(self.client.is_local_endpoint());
        let mut rounds = 0usize;
        let mut previous_round_signature: Option<Vec<String>> = None;
        let mut repeated_read_only_rounds = 0usize;
        loop {
            self.current_turn_blocks.clear();
            self.prune_message_history(limits.max_api_messages);
            rounds += 1;
            if rounds > max_tool_rounds {
                bail!("Exceeded max tool rounds ({max_tool_rounds}). Possible tool-calling loop.");
            }

            let mut stream = self.client.create_stream(&self.api_messages).await?;
            let mut parser = StreamParser::new();
            let mut assistant_text = String::new();
            let mut tool_use_blocks = Vec::new();
            let mut tool_input_buffers: Vec<Option<String>> = Vec::new();
            let mut tool_input_event_emitted: Vec<bool> = Vec::new();
            let mut deferred_text_block_indices = BTreeSet::new();

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
                                            None,
                                        );
                                        deferred_text_block_indices.insert(index);
                                    }
                                    ContentBlock::ToolUse { id, name, input } => {
                                        self.flush_deferred_thinking_blocks(
                                            &mut deferred_text_block_indices,
                                            stream_delta_tx,
                                        );
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
                                    let delta_tx = if deferred_text_block_indices.contains(&index) {
                                        None
                                    } else {
                                        stream_delta_tx
                                    };
                                    let appended = self.append_text_delta(index, &text, delta_tx);
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

            let tool_use_blocks: Vec<ContentBlock> =
                tool_use_blocks.into_iter().flatten().collect();

            if !tool_use_blocks.is_empty() {
                let current_signature = tool_round_signature(&tool_use_blocks);
                if is_read_only_tool_round(&tool_use_blocks)
                    && previous_round_signature
                        .as_ref()
                        .is_some_and(|previous| previous == &current_signature)
                {
                    repeated_read_only_rounds += 1;
                } else {
                    repeated_read_only_rounds = 0;
                }
                previous_round_signature = Some(current_signature);

                if repeated_read_only_rounds >= 2 {
                    bail!(
                        "Detected repeated identical read/search tool round. Stopping to avoid infinite tool loop."
                    );
                }
            } else {
                previous_round_signature = None;
                repeated_read_only_rounds = 0;
            }

            let assistant_history_text = if assistant_text.is_empty() && !tool_use_blocks.is_empty()
            {
                render_tool_calls_for_text_protocol(&tool_use_blocks)
            } else {
                assistant_text.clone()
            };
            let assistant_history_text =
                truncate_for_history(&assistant_history_text, limits.max_assistant_history_chars);

            let use_structured_round = use_structured_tool_protocol;

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
                    self.promote_thinking_blocks_to_final_text(
                        &deferred_text_block_indices,
                        stream_delta_tx,
                    );
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
                        self.execute_tool_with_timeout(&name, &input, tool_timeout)
                            .await
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

                    let history_content = truncate_for_history(
                        &self.format_tool_result_for_history(&name, &input, &result),
                        limits.max_tool_result_history_chars,
                    );
                    if use_structured_round {
                        tool_result_blocks.push(ContentBlock::ToolResult {
                            tool_use_id: id,
                            content: history_content,
                            is_error: result.is_err(),
                        });
                    } else {
                        let rendered = result.as_ref().map_or_else(
                            |_| format!("tool_error {name}:\n{history_content}"),
                            |_| format!("tool_result {name}:\n{history_content}"),
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

    #[cfg(test)]
    async fn execute_tool(&self, name: &str, input: &serde_json::Value) -> Result<String> {
        #[cfg(test)]
        {
            execute_tool_blocking_with_executor(
                &self.tool_executor,
                name,
                input,
                self.mock_tool_executor_responses.clone(),
            )
        }
        #[cfg(not(test))]
        {
            execute_tool_blocking_with_executor(&self.tool_executor, name, input)
        }
    }

    async fn execute_tool_with_timeout(
        &self,
        name: &str,
        input: &serde_json::Value,
        tool_timeout: Duration,
    ) -> Result<String> {
        let tool_name = name.to_string();
        let task_name = tool_name.clone();
        let task_input = input.clone();
        let task_executor = self.tool_executor.clone();
        #[cfg(test)]
        let task_mock_responses = self.mock_tool_executor_responses.clone();

        let mut task = tokio::task::spawn_blocking(move || {
            #[cfg(test)]
            {
                execute_tool_blocking_with_executor(
                    &task_executor,
                    &task_name,
                    &task_input,
                    task_mock_responses,
                )
            }
            #[cfg(not(test))]
            {
                execute_tool_blocking_with_executor(&task_executor, &task_name, &task_input)
            }
        });

        match tokio::time::timeout(tool_timeout, &mut task).await {
            Ok(join_result) => match join_result {
                Ok(result) => result,
                Err(join_error) => Err(anyhow::anyhow!(
                    "Tool execution task failed for {tool_name}: {join_error}"
                )),
            },
            Err(_) => {
                task.abort();
                Err(anyhow::anyhow!(
                    "Tool execution timed out after {}s for {tool_name}",
                    tool_timeout.as_secs()
                ))
            }
        }
    }

    fn prune_message_history(&mut self, max_api_messages: usize) {
        if self.api_messages.len() <= max_api_messages {
            return;
        }

        let len = self.api_messages.len();
        let mut keep_start = len.saturating_sub(max_api_messages);

        // Anthropic requires history to begin with a user message.
        // Additionally, a leading user tool_result is invalid without its preceding assistant tool_use.
        while keep_start < len {
            let message = &self.api_messages[keep_start];
            if message.role == "user" && !message_contains_tool_result(message) {
                break;
            }
            keep_start += 1;
        }

        if keep_start >= len {
            self.api_messages.clear();
            return;
        }

        if keep_start > 0 {
            self.api_messages.drain(0..keep_start);
        }
    }

    fn format_tool_result_for_history(
        &mut self,
        name: &str,
        input: &serde_json::Value,
        result: &Result<String>,
    ) -> String {
        let Ok(output) = result else {
            return result
                .as_ref()
                .err()
                .map_or_else(|| "Unknown tool error".to_string(), ToString::to_string);
        };

        if name == "read_file" {
            // read_file_path returns None if the "path" key is absent or non-string.
            // The fallback "<missing>" is a display-layer decision kept here, not baked into the helper.
            let path = read_file_path(input).unwrap_or_else(|| "<missing>".to_string());
            let summary = self.read_file_history_cache.summarize(&path, output);
            return format_read_file_snapshot_message(
                &path,
                summary,
                ReadFileSummaryMessageStyle::History,
            );
        }

        output.clone()
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

    fn flush_deferred_thinking_blocks(
        &self,
        deferred_text_block_indices: &mut BTreeSet<usize>,
        stream_delta_tx: Option<&mpsc::UnboundedSender<ConversationStreamUpdate>>,
    ) {
        let pending_indices: Vec<usize> = deferred_text_block_indices.iter().copied().collect();
        for index in pending_indices {
            let Some(StreamBlock::Thinking { collapsed, content }) =
                self.current_turn_blocks.get(index)
            else {
                continue;
            };

            emit_stream_update(
                stream_delta_tx,
                ConversationStreamUpdate::BlockStart {
                    index,
                    block: StreamBlock::Thinking {
                        content: String::new(),
                        collapsed: *collapsed,
                    },
                },
            );
            if !content.is_empty() {
                emit_stream_update(
                    stream_delta_tx,
                    ConversationStreamUpdate::BlockDelta {
                        index,
                        delta: content.clone(),
                    },
                );
            }
            deferred_text_block_indices.remove(&index);
        }
    }

    fn promote_thinking_blocks_to_final_text(
        &mut self,
        deferred_text_block_indices: &BTreeSet<usize>,
        stream_delta_tx: Option<&mpsc::UnboundedSender<ConversationStreamUpdate>>,
    ) {
        for (index, block) in self.current_turn_blocks.iter_mut().enumerate() {
            if let StreamBlock::Thinking { content, .. } = block {
                let full_content = content.clone();
                *block = StreamBlock::FinalText {
                    content: full_content.clone(),
                };

                // If the text was already streamed as Thinking, only emit a section switch.
                // Otherwise include the full content as the final response body.
                let streamed_content = if deferred_text_block_indices.contains(&index) {
                    full_content
                } else {
                    String::new()
                };
                emit_stream_update(
                    stream_delta_tx,
                    ConversationStreamUpdate::BlockStart {
                        index,
                        block: StreamBlock::FinalText {
                            content: streamed_content,
                        },
                    },
                );
            }
        }
    }
}

#[cfg(test)]
fn execute_tool_blocking_with_executor(
    tool_executor: &ToolExecutor,
    name: &str,
    input: &serde_json::Value,
    mock_tool_executor_responses: Option<Arc<Mutex<HashMap<String, String>>>>,
) -> Result<String> {
    if let Some(responses_arc) = mock_tool_executor_responses {
        let responses = responses_arc.lock().unwrap();
        if name == "read_file" {
            let path = required_tool_string(input, name, "path")?;
            if let Some(content) = responses.get(path) {
                return Ok(content.clone());
            }
            return Err(anyhow::anyhow!(
                "Mock tool 'read_file' not configured for path: {}",
                path
            ));
        }
    }

    execute_tool_dispatch(tool_executor, name, input)
}

#[cfg(not(test))]
fn execute_tool_blocking_with_executor(
    tool_executor: &ToolExecutor,
    name: &str,
    input: &serde_json::Value,
) -> Result<String> {
    execute_tool_dispatch(tool_executor, name, input)
}

fn execute_tool_dispatch(
    tool_executor: &ToolExecutor,
    name: &str,
    input: &serde_json::Value,
) -> Result<String> {
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

    match name {
        "read_file" => {
            let path = required_tool_string(input, name, "path")?;
            tool_executor.read_file(path)
        }
        "write_file" => {
            let path = required_tool_string(input, name, "path")?;
            tool_executor
                .write_file(path, get_str("content"))
                .map(|_| format!("Successfully wrote to {path}"))
        }
        "edit_file" => {
            let path = required_tool_string(input, name, "path")?;
            let old_str = required_tool_string(input, name, "old_str")?;
            tool_executor
                .edit_file(path, old_str, get_str("new_str"))
                .map(|_| format!("Successfully edited {path}"))
        }
        "rename_file" => {
            let old_path = required_tool_string(input, name, "old_path")?;
            let new_path = required_tool_string(input, name, "new_path")?;
            tool_executor.rename_file(old_path, new_path)
        }
        "list_files" | "list_directory" => tool_executor.list_files(
            input.get("path").and_then(|v| v.as_str()),
            get_usize("max_entries", 100),
        ),
        "search_files" | "search" => tool_executor.search_files(
            get_str("query"),
            input.get("path").and_then(|v| v.as_str()),
            get_usize("max_results", 30),
        ),
        "git_status" => tool_executor.git_status(
            get_bool("short", true),
            input.get("path").and_then(|v| v.as_str()),
        ),
        "git_diff" => tool_executor.git_diff(
            get_bool("cached", false),
            input.get("path").and_then(|v| v.as_str()),
        ),
        "git_log" => tool_executor.git_log(get_usize("max_count", 10)),
        "git_show" => tool_executor.git_show(required_tool_string(input, name, "revision")?),
        "git_add" => tool_executor.git_add(required_tool_string(input, name, "path")?),
        "git_commit" => tool_executor.git_commit(required_tool_string(input, name, "message")?),
        _ => bail!("Unknown tool: {name}"),
    }
}

fn message_contains_tool_result(message: &ApiMessage) -> bool {
    match &message.content {
        Content::Blocks(blocks) => blocks
            .iter()
            .any(|block| matches!(block, ContentBlock::ToolResult { .. })),
        Content::Text(_) => false,
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
    for idx in incoming
        .char_indices()
        .map(|(idx, _)| idx)
        .chain(std::iter::once(incoming.len()))
    {
        if idx > max_overlap {
            break;
        }
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

fn resolve_tool_timeout(is_local_endpoint: bool) -> Duration {
    let default_secs = if is_local_endpoint {
        LOCAL_DEFAULT_TOOL_TIMEOUT_SECS
    } else {
        REMOTE_DEFAULT_TOOL_TIMEOUT_SECS
    };

    let secs = std::env::var("AISTAR_TOOL_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(default_secs)
        .clamp(2, 300);
    Duration::from_secs(secs)
}

fn resolve_max_tool_rounds(is_local_endpoint: bool) -> usize {
    let default_rounds = if is_local_endpoint { 12 } else { 24 };
    std::env::var("AISTAR_MAX_TOOL_ROUNDS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(default_rounds)
        .clamp(2, 64)
}

fn env_override_usize(key: &str, default: usize, min: usize, max: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .map(|v| v.clamp(min, max))
        .unwrap_or(default)
}

fn required_tool_string<'a>(
    input: &'a serde_json::Value,
    tool: &str,
    key: &str,
) -> Result<&'a str> {
    let value = input
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .unwrap_or("");
    if value.is_empty() {
        bail!("{tool} requires a non-empty '{key}' string argument");
    }
    Ok(value)
}

fn stream_local_tool_events_enabled() -> bool {
    std::env::var("AISTAR_STREAM_LOCAL_TOOL_EVENTS")
        .ok()
        .and_then(parse_bool_flag)
        .unwrap_or(false)
}

fn default_tool_approval_enabled(is_local_endpoint: bool) -> bool {
    !is_local_endpoint
}

fn tool_approval_enabled(is_local_endpoint: bool) -> bool {
    std::env::var("AISTAR_TOOL_CONFIRM")
        .ok()
        .and_then(parse_bool_flag)
        .unwrap_or(default_tool_approval_enabled(is_local_endpoint))
}

fn stream_server_events_enabled() -> bool {
    std::env::var("AISTAR_STREAM_SERVER_EVENTS")
        .ok()
        .and_then(parse_bool_flag)
        .unwrap_or(true)
}

fn tool_input_preview(tool_name: &str, input: &serde_json::Value) -> String {
    preview_tool_input(
        tool_name,
        input,
        ToolPreviewStyle::Compact,
        DEFAULT_EDIT_DIFF_CONTEXT_LINES,
    )
}

#[cfg(test)]
#[derive(Debug, Clone)]
struct TaggedToolCall {
    name: String,
    input: serde_json::Value,
}

#[cfg(test)]
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

#[cfg(test)]
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

#[cfg(test)]
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

#[cfg(test)]
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

fn is_read_only_tool_name(name: &str) -> bool {
    matches!(
        name,
        "read_file" | "search" | "search_files" | "list_files" | "list_directory"
    )
}

fn is_read_only_tool_round(blocks: &[ContentBlock]) -> bool {
    blocks.iter().all(|block| {
        matches!(
            block,
            ContentBlock::ToolUse { name, .. } if is_read_only_tool_name(name)
        )
    })
}

fn tool_round_signature(blocks: &[ContentBlock]) -> Vec<String> {
    let mut signature = Vec::new();
    for block in blocks {
        if let ContentBlock::ToolUse { name, input, .. } = block {
            let payload = serde_json::to_string(input).unwrap_or_else(|_| input.to_string());
            signature.push(format!("{name}:{payload}"));
        }
    }
    signature
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::ApiClient;
    use serde_json::json;
    use tempfile::TempDir;
    use tokio::sync::mpsc;

    #[test]
    fn test_read_only_tool_round_helpers() {
        let read_round = vec![ContentBlock::ToolUse {
            id: "tool_1".to_string(),
            name: "read_file".to_string(),
            input: json!({"path":"src/app/mod.rs"}),
        }];
        assert!(is_read_only_tool_round(&read_round));

        let write_round = vec![ContentBlock::ToolUse {
            id: "tool_2".to_string(),
            name: "write_file".to_string(),
            input: json!({"path":"src/app/mod.rs","content":"x"}),
        }];
        assert!(!is_read_only_tool_round(&write_round));

        let sig_a = tool_round_signature(&read_round);
        let sig_b = tool_round_signature(&read_round);
        assert_eq!(sig_a, sig_b);

        let changed_read_round = vec![ContentBlock::ToolUse {
            id: "tool_3".to_string(),
            name: "read_file".to_string(),
            input: json!({"path":"src/state/conversation.rs"}),
        }];
        let sig_c = tool_round_signature(&changed_read_round);
        assert_ne!(sig_a, sig_c);
    }

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
                assert!(content.contains("Read file.txt:"));
                assert!(content.contains("Full content omitted"));
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

    #[tokio::test]
    async fn test_structured_text_only_round_streams_final_text_block() -> Result<()> {
        let response_sse = vec![
            r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_text_only_1","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
            r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
            r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"This is the final answer."}}"#.to_string(),
            r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":8}}"#.to_string(),
            r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
        ];

        let mock_api_client =
            ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
                response_sse,
            ])));

        let mut manager = ConversationManager::new_mock(mock_api_client, HashMap::new());
        let (tx, mut rx) = mpsc::unbounded_channel();

        let final_text = manager
            .send_message("say hi".to_string(), Some(&tx))
            .await?;
        assert_eq!(final_text, "This is the final answer.");

        drop(tx);

        let mut saw_thinking_start = false;
        let mut final_block_content = String::new();
        while let Ok(update) = rx.try_recv() {
            if let ConversationStreamUpdate::BlockStart { block, .. } = update {
                match block {
                    StreamBlock::Thinking { .. } => saw_thinking_start = true,
                    StreamBlock::FinalText { content } => final_block_content = content,
                    StreamBlock::ToolCall { .. } | StreamBlock::ToolResult { .. } => {}
                }
            }
        }

        assert!(!saw_thinking_start);
        assert_eq!(final_block_content, "This is the final answer.");
        Ok(())
    }

    #[tokio::test]
    async fn test_structured_tool_then_final_round_streams_thinking_then_final_text() -> Result<()>
    {
        let first_response_sse = vec![
            r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_tool_then_final_1","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
            r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
            r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"I will read the file."}}"#.to_string(),
            r#"event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_mock_round_1","name":"read_file","input":{}}}"#.to_string(),
            r#"event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"file.txt\"}"}}"#.to_string(),
            r#"event: content_block_stop
data: {"type":"content_block_stop","index":1}"#.to_string(),
            r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":10}}"#.to_string(),
            r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
        ];

        let second_response_sse = vec![
            r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_tool_then_final_2","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
            r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
            r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"The file says hello."}}"#.to_string(),
            r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":7}}"#.to_string(),
            r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
        ];

        let mock_api_client =
            ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
                first_response_sse,
                second_response_sse,
            ])));

        let mut mock_tool_responses = HashMap::new();
        mock_tool_responses.insert("file.txt".to_string(), "hello".to_string());
        let mut manager = ConversationManager::new_mock(mock_api_client, mock_tool_responses);

        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut saw_thinking_start = false;
        let mut saw_final_start = false;

        let tx_for_send = tx.clone();
        let mut send_future =
            std::pin::pin!(manager.send_message("read file".to_string(), Some(&tx_for_send)));
        let final_text = loop {
            tokio::select! {
                result = &mut send_future => break result?,
                maybe_update = rx.recv() => {
                    let Some(update) = maybe_update else { continue; };
                    match update {
                        ConversationStreamUpdate::BlockStart { block, .. } => {
                            match block {
                                StreamBlock::Thinking { .. } => saw_thinking_start = true,
                                StreamBlock::FinalText { .. } => saw_final_start = true,
                                StreamBlock::ToolCall { .. } | StreamBlock::ToolResult { .. } => {}
                            }
                        }
                        ConversationStreamUpdate::ToolApprovalRequest(request) => {
                            let _ = request.response_tx.send(true);
                        }
                        ConversationStreamUpdate::Delta(_)
                        | ConversationStreamUpdate::BlockDelta { .. }
                        | ConversationStreamUpdate::BlockComplete { .. } => {}
                    }
                }
            }
        };
        assert_eq!(final_text, "The file says hello.");
        drop(tx);

        while let Ok(update) = rx.try_recv() {
            if let ConversationStreamUpdate::BlockStart { block, .. } = update {
                match block {
                    StreamBlock::Thinking { .. } => saw_thinking_start = true,
                    StreamBlock::FinalText { .. } => saw_final_start = true,
                    StreamBlock::ToolCall { .. } | StreamBlock::ToolResult { .. } => {}
                }
            }
        }

        assert!(saw_thinking_start);
        assert!(saw_final_start);
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

    #[test]
    fn test_required_tool_string_validation() {
        let input = json!({ "path": " cal.rs " });
        assert_eq!(
            required_tool_string(&input, "read_file", "path").unwrap(),
            "cal.rs"
        );

        let missing = json!({});
        assert!(required_tool_string(&missing, "read_file", "path").is_err());
    }

    #[test]
    fn test_default_tool_approval_enabled_prefers_remote_only() {
        assert!(default_tool_approval_enabled(false));
        assert!(!default_tool_approval_enabled(true));
    }

    #[test]
    fn test_format_tool_result_for_history_read_file_diff_and_repeat() {
        let mock_api_client = ApiClient::new_mock(Arc::new(
            crate::api::mock_client::MockApiClient::new(vec![]),
        ));
        let mut manager = ConversationManager::new_mock(mock_api_client, HashMap::new());
        let input = serde_json::json!({ "path": "cal.rs" });

        let first = manager.format_tool_result_for_history(
            "read_file",
            &input,
            &Ok("line1\nline2".to_string()),
        );
        assert!(first.contains("Read cal.rs:"));
        assert!(first.contains("Full content omitted"));

        let second = manager.format_tool_result_for_history(
            "read_file",
            &input,
            &Ok("line1\nline2".to_string()),
        );
        assert!(second.contains("No changes since last read"));

        let third = manager.format_tool_result_for_history(
            "read_file",
            &input,
            &Ok("line1\nline2 changed".to_string()),
        );
        assert!(third.contains("content changed"));
        assert!(third.contains("Full content omitted"));

        // After a change the cache must update, so the same content read again
        // must be classified as Unchanged — not another Changed.
        let fourth = manager.format_tool_result_for_history(
            "read_file",
            &input,
            &Ok("line1\nline2 changed".to_string()),
        );
        assert!(
            fourth.contains("No changes since last read"),
            "expected Unchanged after re-reading the post-change content, got: {fourth}"
        );
    }

    #[tokio::test]
    async fn test_text_tagged_tool_call_text_is_not_executed_as_tool() -> Result<()> {
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

        let mock_api_client =
            ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
                first_response_sse,
            ])));

        let mut mock_tool_responses = HashMap::new();
        mock_tool_responses.insert("file.txt".to_string(), "Hello from fallback.".to_string());
        let mut manager = ConversationManager::new_mock(mock_api_client, mock_tool_responses);

        let final_text = manager.send_message("Read file".into(), None).await?;
        assert!(final_text.contains("<function=read_file>"));
        assert!(!final_text.contains("Hello from fallback."));

        let messages = &manager.api_messages;
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1].role, "assistant");
        if let Content::Blocks(blocks) = &messages[1].content {
            assert_eq!(blocks.len(), 1);
            if let ContentBlock::Text { text } = &blocks[0] {
                assert!(text.contains("<function=read_file>"));
            } else {
                panic!("expected assistant text block content");
            }
        } else {
            panic!("expected assistant blocks content");
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_text_tagged_tool_call_does_not_emit_structured_tool_blocks() -> Result<()> {
        let first_response_sse = vec![
            r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_mock_fallback_20","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
            r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
            r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"I will read it.\n<function=read_file>\n<parameter=path>\nfile.txt\n</parameter>\n</function>"}}"#.to_string(),
            r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}"#.to_string(),
            r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
        ];

        let mock_api_client =
            ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
                first_response_sse,
            ])));
        let mut mock_tool_responses = HashMap::new();
        mock_tool_responses.insert("file.txt".to_string(), "Hello from fallback.".to_string());
        let mut manager = ConversationManager::new_mock(mock_api_client, mock_tool_responses);

        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut saw_tool_call_block = false;
        let tx_for_send = tx.clone();
        let mut send_future =
            std::pin::pin!(manager.send_message("Read file".to_string(), Some(&tx_for_send)));
        let _final_text = loop {
            tokio::select! {
                result = &mut send_future => break result?,
                maybe_update = rx.recv() => {
                    let Some(update) = maybe_update else { continue; };
                    match update {
                        ConversationStreamUpdate::BlockStart { block, .. } => {
                            if matches!(block, StreamBlock::ToolCall { ref name, .. } if name == "read_file") {
                                saw_tool_call_block = true;
                            }
                        }
                        ConversationStreamUpdate::ToolApprovalRequest(request) => {
                            let _ = request.response_tx.send(true);
                        }
                        ConversationStreamUpdate::Delta(_)
                        | ConversationStreamUpdate::BlockDelta { .. }
                        | ConversationStreamUpdate::BlockComplete { .. } => {}
                    }
                }
            }
        };

        drop(tx);
        while let Ok(update) = rx.try_recv() {
            if let ConversationStreamUpdate::BlockStart { block, .. } = update {
                if matches!(block, StreamBlock::ToolCall { ref name, .. } if name == "read_file") {
                    saw_tool_call_block = true;
                }
            }
        }

        assert!(!saw_tool_call_block);
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

        let mock_api_client =
            ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
                first_response_sse,
            ])))
            .with_structured_tool_protocol(false);

        let mut mock_tool_responses = HashMap::new();
        mock_tool_responses.insert(
            "file.txt".to_string(),
            "Hello local text protocol.".to_string(),
        );
        let mut manager = ConversationManager::new_mock(mock_api_client, mock_tool_responses);

        let final_text = manager.send_message("Read file".into(), None).await?;
        assert!(final_text.contains("<function=read_file>"));

        let messages = &manager.api_messages;
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1].role, "assistant");
        match &messages[1].content {
            Content::Text(text) => {
                assert!(text.contains("<function=read_file>"));
            }
            _ => panic!("expected assistant text content in local text protocol"),
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

    #[tokio::test]
    async fn test_execute_tool_edit_file_empty_path_rejected_before_executor() -> Result<()> {
        let temp = TempDir::new()?;
        let mock_api_client = ApiClient::new_mock(Arc::new(
            crate::api::mock_client::MockApiClient::new(vec![]),
        ));
        let executor = ToolExecutor::new(temp.path().to_path_buf());
        let manager = ConversationManager::new(mock_api_client, executor);

        let err = manager
            .execute_tool(
                "edit_file",
                &json!({
                    "path": "",
                    "old_str": "old",
                    "new_str": "new"
                }),
            )
            .await
            .expect_err("empty path should be rejected");
        assert!(err.to_string().contains("non-empty 'path'"));
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

    #[test]
    fn test_append_incremental_suffix_handles_unicode_char_boundaries() {
        let mut content = String::new();
        let first = append_incremental_suffix(&mut content, "•");
        let second = append_incremental_suffix(&mut content, "• item");

        assert_eq!(first, "•");
        assert_eq!(second, " item");
        assert_eq!(content, "• item");
    }

    #[test]
    fn test_prune_message_history_reanchors_to_user() {
        let mock_api_client = ApiClient::new_mock(Arc::new(
            crate::api::mock_client::MockApiClient::new(vec![]),
        ));
        let executor = ToolExecutor::new(std::path::PathBuf::from("."));
        let mut manager = ConversationManager::new(mock_api_client, executor);

        manager.api_messages = vec![
            ApiMessage {
                role: "user".to_string(),
                content: Content::Text("u0".to_string()),
            },
            ApiMessage {
                role: "assistant".to_string(),
                content: Content::Text("a0".to_string()),
            },
            ApiMessage {
                role: "assistant".to_string(),
                content: Content::Text("a1".to_string()),
            },
            ApiMessage {
                role: "user".to_string(),
                content: Content::Text("u1".to_string()),
            },
            ApiMessage {
                role: "assistant".to_string(),
                content: Content::Text("a2".to_string()),
            },
        ];

        manager.prune_message_history(3);

        assert_eq!(manager.api_messages.len(), 2);
        assert_eq!(manager.api_messages[0].role, "user");
        assert_eq!(manager.api_messages[1].role, "assistant");
    }

    #[test]
    fn test_prune_message_history_clears_if_no_user_remains() {
        let mock_api_client = ApiClient::new_mock(Arc::new(
            crate::api::mock_client::MockApiClient::new(vec![]),
        ));
        let executor = ToolExecutor::new(std::path::PathBuf::from("."));
        let mut manager = ConversationManager::new(mock_api_client, executor);

        manager.api_messages = vec![
            ApiMessage {
                role: "user".to_string(),
                content: Content::Text("u0".to_string()),
            },
            ApiMessage {
                role: "assistant".to_string(),
                content: Content::Text("a0".to_string()),
            },
            ApiMessage {
                role: "assistant".to_string(),
                content: Content::Text("a1".to_string()),
            },
            ApiMessage {
                role: "assistant".to_string(),
                content: Content::Text("a2".to_string()),
            },
        ];

        manager.prune_message_history(2);
        assert!(manager.api_messages.is_empty());
    }

    #[test]
    fn test_prune_message_history_reanchors_even_if_it_reduces_below_limit() {
        let mock_api_client = ApiClient::new_mock(Arc::new(
            crate::api::mock_client::MockApiClient::new(vec![]),
        ));
        let executor = ToolExecutor::new(std::path::PathBuf::from("."));
        let mut manager = ConversationManager::new(mock_api_client, executor);

        manager.api_messages = vec![
            ApiMessage {
                role: "user".to_string(),
                content: Content::Text("u0".to_string()),
            },
            ApiMessage {
                role: "assistant".to_string(),
                content: Content::Text("a0".to_string()),
            },
            ApiMessage {
                role: "user".to_string(),
                content: Content::Text("u1".to_string()),
            },
            ApiMessage {
                role: "assistant".to_string(),
                content: Content::Text("a1".to_string()),
            },
        ];

        manager.prune_message_history(3);

        assert_eq!(manager.api_messages.len(), 2);
        assert_eq!(manager.api_messages[0].role, "user");
        if let Content::Text(text) = &manager.api_messages[0].content {
            assert_eq!(text, "u1");
        } else {
            panic!("expected user text content");
        }
    }

    #[test]
    fn test_prune_message_history_skips_leading_tool_result_user_message() {
        let mock_api_client = ApiClient::new_mock(Arc::new(
            crate::api::mock_client::MockApiClient::new(vec![]),
        ));
        let executor = ToolExecutor::new(std::path::PathBuf::from("."));
        let mut manager = ConversationManager::new(mock_api_client, executor);

        manager.api_messages = vec![
            ApiMessage {
                role: "user".to_string(),
                content: Content::Text("u0".to_string()),
            },
            ApiMessage {
                role: "assistant".to_string(),
                content: Content::Blocks(vec![ContentBlock::ToolUse {
                    id: "tool_1".to_string(),
                    name: "read_file".to_string(),
                    input: json!({"path":"src/lib.rs"}),
                }]),
            },
            ApiMessage {
                role: "user".to_string(),
                content: Content::Blocks(vec![ContentBlock::ToolResult {
                    tool_use_id: "tool_1".to_string(),
                    content: "ok".to_string(),
                    is_error: false,
                }]),
            },
            ApiMessage {
                role: "assistant".to_string(),
                content: Content::Text("a1".to_string()),
            },
            ApiMessage {
                role: "user".to_string(),
                content: Content::Text("u1".to_string()),
            },
            ApiMessage {
                role: "assistant".to_string(),
                content: Content::Text("a2".to_string()),
            },
        ];

        manager.prune_message_history(4);

        assert_eq!(manager.api_messages.len(), 2);
        assert_eq!(manager.api_messages[0].role, "user");
        match &manager.api_messages[0].content {
            Content::Text(text) => assert_eq!(text, "u1"),
            _ => panic!("expected first retained message to be user text, not tool_result"),
        }
    }

    #[test]
    fn test_prune_message_history_clears_if_only_tool_result_user_messages_remain() {
        let mock_api_client = ApiClient::new_mock(Arc::new(
            crate::api::mock_client::MockApiClient::new(vec![]),
        ));
        let executor = ToolExecutor::new(std::path::PathBuf::from("."));
        let mut manager = ConversationManager::new(mock_api_client, executor);

        manager.api_messages = vec![
            ApiMessage {
                role: "assistant".to_string(),
                content: Content::Text("a0".to_string()),
            },
            ApiMessage {
                role: "user".to_string(),
                content: Content::Blocks(vec![ContentBlock::ToolResult {
                    tool_use_id: "tool_1".to_string(),
                    content: "ok".to_string(),
                    is_error: false,
                }]),
            },
            ApiMessage {
                role: "assistant".to_string(),
                content: Content::Text("a1".to_string()),
            },
        ];

        manager.prune_message_history(2);

        assert!(manager.api_messages.is_empty());
    }
}
