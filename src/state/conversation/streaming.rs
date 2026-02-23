use super::super::stream_block::{StreamBlock, ToolStatus};
use super::{ConversationManager, ConversationStreamUpdate};
use crate::util::parse_bool_flag;
use std::collections::BTreeSet;
use tokio::sync::mpsc;

impl ConversationManager {
    pub(super) fn upsert_turn_block(
        &mut self,
        index: usize,
        block: StreamBlock,
        stream_delta_tx: Option<&mpsc::UnboundedSender<ConversationStreamUpdate>>,
    ) {
        while self.current_turn_blocks.len() < index {
            let pad_index = self.current_turn_blocks.len();
            let placeholder = StreamBlock::Thinking {
                content: String::new(),
                collapsed: true,
            };
            self.current_turn_blocks.push(placeholder.clone());
            emit_stream_update(
                stream_delta_tx,
                ConversationStreamUpdate::BlockStart {
                    index: pad_index,
                    block: placeholder,
                },
            );
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

    pub(super) fn append_text_delta(
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

    pub(super) fn set_tool_call_status(
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

    pub(super) fn push_tool_result_block(
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

    pub(super) fn flush_deferred_thinking_blocks(
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

    pub(super) fn promote_thinking_blocks_to_final_text(
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

pub(super) fn emit_stream_update(
    stream_delta_tx: Option<&mpsc::UnboundedSender<ConversationStreamUpdate>>,
    update: ConversationStreamUpdate,
) {
    if let Some(tx) = stream_delta_tx {
        let _ = tx.send(update);
    }
}

pub(super) fn emit_text_update(
    stream_delta_tx: Option<&mpsc::UnboundedSender<ConversationStreamUpdate>>,
    text: String,
) {
    emit_stream_update(stream_delta_tx, ConversationStreamUpdate::Delta(text));
}

pub(super) fn structured_blocks_enabled() -> bool {
    std::env::var("VEX_USE_STRUCTURED_BLOCKS")
        .ok()
        .and_then(parse_bool_flag)
        .unwrap_or(true)
}

pub(super) fn append_incremental_suffix(existing: &mut String, incoming: &str) -> String {
    if incoming.is_empty() {
        return String::new();
    }

    // If the incoming string starts with the existing content, it's a cumulative update.
    // We extract the new part and append it.
    if incoming.starts_with(existing.as_str()) {
        let suffix = &incoming[existing.len()..];
        if suffix.is_empty() {
            return String::new();
        }
        let suffix_owned = suffix.to_string();
        existing.push_str(&suffix_owned);
        return suffix_owned;
    }

    // If the existing content already starts with the incoming string, it's a redundant
    // prefix or a re-transmission of an earlier part of the stream.
    if existing.starts_with(incoming) {
        return String::new();
    }

    // In all other cases, we treat the incoming string as a new delta.
    // We specifically avoid searching for partial overlaps between the end of 'existing'
    // and the start of 'incoming' to prevent data loss when the assistant repeats text.
    existing.push_str(incoming);
    incoming.to_string()
}

pub(super) fn stream_local_tool_events_enabled() -> bool {
    std::env::var("VEX_STREAM_LOCAL_TOOL_EVENTS")
        .ok()
        .and_then(parse_bool_flag)
        .unwrap_or(false)
}

pub(super) fn stream_server_events_enabled() -> bool {
    std::env::var("VEX_STREAM_SERVER_EVENTS")
        .ok()
        .and_then(parse_bool_flag)
        .unwrap_or(true)
}
