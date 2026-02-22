use super::super::stream_block::{StreamBlock, ToolStatus};
use super::{history::*, streaming::*, tools::*, ConversationManager, ConversationStreamUpdate};
use crate::api::stream::StreamParser;
use crate::runtime::policy::{default_runtime_policy, RuntimeCorePolicy};
use crate::types::{ApiMessage, Content, ContentBlock, StreamEvent};
use anyhow::Result;
use futures::StreamExt;
use std::collections::BTreeSet;
use tokio::sync::mpsc;

impl ConversationManager {
    pub async fn send_message(
        &mut self,
        content: String,
        stream_delta_tx: Option<&mpsc::UnboundedSender<ConversationStreamUpdate>>,
    ) -> Result<String> {
        self.current_turn_blocks.clear();
        let original_user_input = content.clone();
        self.push_user_message(content);
        if let Some(response) = builtin_supported_git_tools_response(&original_user_input) {
            self.api_messages.push(ApiMessage {
                role: "assistant".to_string(),
                content: Content::Text(response.clone()),
            });
            emit_text_update(stream_delta_tx, response.clone());
            return Ok(response);
        }
        let mut turn_user_anchor_index = self.api_messages.len().saturating_sub(1);

        let core_policy = default_runtime_policy();
        let use_structured_tool_protocol = self.client.supports_structured_tool_protocol();
        let use_structured_blocks = structured_blocks_enabled();
        let requires_tool_evidence =
            core_policy.request_requires_tool_evidence(&original_user_input);
        let limits = resolve_history_limits(self.client.is_local_endpoint());
        let tool_timeout = resolve_tool_timeout(self.client.is_local_endpoint());
        let max_tool_rounds = resolve_max_tool_rounds(self.client.is_local_endpoint());
        let stream_server_events = stream_server_events_enabled();
        let stream_local_tool_events = stream_local_tool_events_enabled();
        let require_tool_approval = tool_approval_enabled(self.client.is_local_endpoint());
        let mut rounds = 0usize;
        let mut forced_tool_retry_count = 0usize;
        let mut saw_any_tool_round = false;
        let mut previous_round_signature: Option<Vec<String>> = None;
        let mut repeated_read_only_rounds = 0usize;
        let mut repeated_mutating_rounds = 0usize;
        let mut repeated_round_nudge_used = false;
        let mut last_assistant_text_for_history = String::new();
        loop {
            self.current_turn_blocks.clear();
            turn_user_anchor_index = self
                .prune_message_history_preserving(limits.max_api_messages, turn_user_anchor_index);
            rounds += 1;
            if rounds > max_tool_rounds {
                return Ok(render_loop_limit_guard_message(
                    &last_assistant_text_for_history,
                    max_tool_rounds,
                ));
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

            let mut assistant_text_for_history = assistant_text.clone();
            let mut used_tagged_fallback = false;
            let mut tool_use_blocks: Vec<ContentBlock> =
                tool_use_blocks.into_iter().flatten().collect();
            if tool_use_blocks.is_empty() && self.client.is_local_endpoint() {
                let tagged_calls = parse_tagged_tool_calls(&assistant_text);
                if !tagged_calls.is_empty() {
                    used_tagged_fallback = true;
                    assistant_text_for_history =
                        core_policy.sanitize_assistant_text(&assistant_text);
                    tool_use_blocks = tagged_calls
                        .into_iter()
                        .enumerate()
                        .map(|(index, call)| ContentBlock::ToolUse {
                            id: format!("toolu_tagged_{rounds}_{index}"),
                            name: call.name,
                            input: call.input,
                        })
                        .collect();
                    if use_structured_blocks {
                        let fallback_start_index = self.current_turn_blocks.len();
                        for (offset, block) in tool_use_blocks.iter().enumerate() {
                            if let ContentBlock::ToolUse { id, name, input } = block {
                                self.upsert_turn_block(
                                    fallback_start_index + offset,
                                    StreamBlock::ToolCall {
                                        id: id.clone(),
                                        name: name.clone(),
                                        input: input.clone(),
                                        status: ToolStatus::Pending,
                                    },
                                    stream_delta_tx,
                                );
                            }
                        }
                    }
                }
            }

            let use_structured_round = use_structured_tool_protocol && !used_tagged_fallback;

            let assistant_history_source = if !tool_use_blocks.is_empty() && !use_structured_round {
                let rendered_tool_calls = render_tool_calls_for_text_protocol(&tool_use_blocks);
                if assistant_text_for_history.is_empty() {
                    rendered_tool_calls
                } else {
                    format!("{assistant_text_for_history}\n{rendered_tool_calls}")
                }
            } else if assistant_text_for_history.is_empty() && !tool_use_blocks.is_empty() {
                render_tool_calls_for_text_protocol(&tool_use_blocks)
            } else {
                assistant_text_for_history.clone()
            };

            let mut inject_repeated_round_nudge = false;
            if !tool_use_blocks.is_empty() {
                saw_any_tool_round = true;
                let current_signature = tool_round_signature(&tool_use_blocks);
                let repeated_signature = previous_round_signature
                    .as_ref()
                    .is_some_and(|previous| previous == &current_signature);
                if is_read_only_tool_round(&tool_use_blocks) && repeated_signature {
                    repeated_read_only_rounds += 1;
                } else {
                    repeated_read_only_rounds = 0;
                }

                if is_mutating_tool_round(&tool_use_blocks) && repeated_signature {
                    repeated_mutating_rounds += 1;
                } else {
                    repeated_mutating_rounds = 0;
                }
                previous_round_signature = Some(current_signature);

                if repeated_mutating_rounds >= 1 {
                    return Ok(render_repeated_mutating_tool_guard_message(
                        &assistant_text_for_history,
                    ));
                }

                if repeated_read_only_rounds >= 2 {
                    if !repeated_round_nudge_used && rounds < max_tool_rounds {
                        repeated_round_nudge_used = true;
                        inject_repeated_round_nudge = true;
                    } else {
                        return Ok(render_repeated_tool_guard_message(
                            &assistant_text_for_history,
                        ));
                    }
                }
            } else {
                previous_round_signature = None;
                repeated_read_only_rounds = 0;
                repeated_mutating_rounds = 0;
            }

            let assistant_history_text = assistant_history_source;
            let assistant_history_text =
                truncate_for_history(&assistant_history_text, limits.max_assistant_history_chars);

            if use_structured_round {
                let mut assistant_content_blocks = Vec::new();
                if !assistant_text_for_history.is_empty() {
                    assistant_content_blocks.push(ContentBlock::Text {
                        text: truncate_for_history(
                            &assistant_text_for_history,
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
            last_assistant_text_for_history = assistant_text_for_history.clone();

            if inject_repeated_round_nudge {
                self.api_messages.push(ApiMessage {
                    role: "user".to_string(),
                    content: Content::Text(
                        core_policy.repeated_tool_round_instruction().to_string(),
                    ),
                });
                continue;
            }

            if tool_use_blocks.is_empty() {
                if self.client.is_local_endpoint()
                    && requires_tool_evidence
                    && !saw_any_tool_round
                    && forced_tool_retry_count < 2
                    && rounds < max_tool_rounds
                {
                    forced_tool_retry_count += 1;
                    self.api_messages.push(ApiMessage {
                        role: "user".to_string(),
                        content: Content::Text(core_policy.tool_retry_instruction().to_string()),
                    });
                    continue;
                }
                if self.client.is_local_endpoint() && requires_tool_evidence && !saw_any_tool_round
                {
                    return Ok(render_missing_tool_evidence_guard_message(
                        &assistant_text_for_history,
                    ));
                }
                if use_structured_blocks {
                    self.promote_thinking_blocks_to_final_text(
                        &deferred_text_block_indices,
                        stream_delta_tx,
                    );
                }
                return Ok(assistant_text_for_history);
            }

            let mut tool_result_blocks = Vec::new();
            let mut text_protocol_tool_results = Vec::new();
            for block in tool_use_blocks {
                if let ContentBlock::ToolUse { id, name, input } = block {
                    if let Some(clarification) = missing_mutating_location_prompt(&name, &input) {
                        if use_structured_blocks {
                            self.set_tool_call_status(&id, ToolStatus::Cancelled, stream_delta_tx);
                            self.push_tool_result_block(
                                StreamBlock::ToolResult {
                                    tool_call_id: id.clone(),
                                    output: clarification.clone(),
                                    is_error: true,
                                },
                                stream_delta_tx,
                            );
                        } else if stream_local_tool_events {
                            emit_text_update(
                                stream_delta_tx,
                                format!("\n- [tool_error] {name}: {clarification}\n"),
                            );
                        }
                        emit_text_update(stream_delta_tx, clarification.clone());
                        let history_content = truncate_for_history(
                            &clarification,
                            limits.max_tool_result_history_chars,
                        );
                        if use_structured_round {
                            tool_result_blocks.push(ContentBlock::ToolResult {
                                tool_use_id: id,
                                content: history_content,
                                is_error: true,
                            });
                        } else {
                            let rendered = format!("tool_error {name}:\n{history_content}");
                            text_protocol_tool_results.push(truncate_for_history(
                                &rendered,
                                limits.max_tool_result_history_chars,
                            ));
                        }
                        continue;
                    }

                    if let Some(read_only_guard) =
                        mutating_tool_read_only_conflict_prompt(&original_user_input, &name)
                    {
                        if use_structured_blocks {
                            self.set_tool_call_status(&id, ToolStatus::Cancelled, stream_delta_tx);
                            self.push_tool_result_block(
                                StreamBlock::ToolResult {
                                    tool_call_id: id.clone(),
                                    output: read_only_guard.clone(),
                                    is_error: true,
                                },
                                stream_delta_tx,
                            );
                        } else if stream_local_tool_events {
                            emit_text_update(
                                stream_delta_tx,
                                format!("\n- [tool_error] {name}: {read_only_guard}\n"),
                            );
                        }
                        emit_text_update(stream_delta_tx, read_only_guard.clone());
                        let history_content = truncate_for_history(
                            &read_only_guard,
                            limits.max_tool_result_history_chars,
                        );
                        if use_structured_round {
                            tool_result_blocks.push(ContentBlock::ToolResult {
                                tool_use_id: id,
                                content: history_content,
                                is_error: true,
                            });
                        } else {
                            let rendered = format!("tool_error {name}:\n{history_content}");
                            text_protocol_tool_results.push(truncate_for_history(
                                &rendered,
                                limits.max_tool_result_history_chars,
                            ));
                        }
                        continue;
                    }

                    let tool_requires_approval =
                        require_tool_approval || tool_requires_confirmation(&name);

                    if use_structured_blocks && tool_requires_approval {
                        self.set_tool_call_status(
                            &id,
                            ToolStatus::WaitingApproval,
                            stream_delta_tx,
                        );
                    }
                    let approved = if tool_requires_approval {
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

                    if !approved {
                        let denial = render_tool_denied_message(&name);
                        if use_structured_blocks {
                            self.push_tool_result_block(
                                StreamBlock::ToolResult {
                                    tool_call_id: id.clone(),
                                    output: denial.clone(),
                                    is_error: true,
                                },
                                stream_delta_tx,
                            );
                        } else if stream_local_tool_events {
                            emit_text_update(
                                stream_delta_tx,
                                format!("\n- [tool_error] {name}: {denial}\n"),
                            );
                        }
                        emit_text_update(stream_delta_tx, denial.clone());
                        let history_content =
                            truncate_for_history(&denial, limits.max_tool_result_history_chars);
                        if use_structured_round {
                            tool_result_blocks.push(ContentBlock::ToolResult {
                                tool_use_id: id,
                                content: history_content,
                                is_error: true,
                            });
                        } else {
                            let rendered = format!("tool_error {name}:\n{history_content}");
                            text_protocol_tool_results.push(truncate_for_history(
                                &rendered,
                                limits.max_tool_result_history_chars,
                            ));
                        }
                        continue;
                    }

                    let result = self
                        .execute_tool_with_timeout(&name, &input, tool_timeout)
                        .await;
                    if use_structured_blocks {
                        let final_status = if result.is_err() {
                            ToolStatus::Error
                        } else {
                            ToolStatus::Complete
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
}
