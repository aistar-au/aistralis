use super::ConversationManager;
use crate::tool_preview::{
    format_read_file_snapshot_message, read_file_path, ReadFileSnapshotSummary,
    ReadFileSummaryMessageStyle,
};
use crate::types::{ApiMessage, Content, ContentBlock};
use anyhow::Result;
use std::time::Duration;

const LOCAL_DEFAULT_MAX_ASSISTANT_HISTORY_CHARS: usize = 1_200;
const LOCAL_DEFAULT_MAX_TOOL_RESULT_HISTORY_CHARS: usize = 2_500;
const LOCAL_DEFAULT_MAX_API_MESSAGES: usize = 14;
const LOCAL_DEFAULT_TOOL_TIMEOUT_SECS: u64 = 20;
const REMOTE_DEFAULT_MAX_ASSISTANT_HISTORY_CHARS: usize = 3_000;
const REMOTE_DEFAULT_MAX_TOOL_RESULT_HISTORY_CHARS: usize = 6_000;
const REMOTE_DEFAULT_MAX_API_MESSAGES: usize = 32;
const REMOTE_DEFAULT_TOOL_TIMEOUT_SECS: u64 = 60;

#[derive(Clone, Copy)]
pub(super) struct HistoryLimits {
    pub(super) max_assistant_history_chars: usize,
    pub(super) max_tool_result_history_chars: usize,
    pub(super) max_api_messages: usize,
}

impl ConversationManager {
    #[cfg(test)]
    pub(super) fn prune_message_history(&mut self, max_api_messages: usize) {
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

    pub(super) fn prune_message_history_preserving(
        &mut self,
        max_api_messages: usize,
        preserve_index: usize,
    ) -> usize {
        if self.api_messages.is_empty() {
            return 0;
        }
        if self.api_messages.len() <= max_api_messages {
            return preserve_index.min(self.api_messages.len().saturating_sub(1));
        }

        let len = self.api_messages.len();
        let target_keep_start = len.saturating_sub(max_api_messages);
        let preserve_distance = target_keep_start.saturating_sub(preserve_index);
        let keep_preserve_anchor = preserve_index < target_keep_start && preserve_distance <= 2;
        let mut keep_start = if keep_preserve_anchor {
            preserve_index
        } else {
            target_keep_start
        };

        while keep_start < len {
            if keep_preserve_anchor && keep_start == preserve_index {
                break;
            }
            let message = &self.api_messages[keep_start];
            if message.role == "user" && !message_contains_tool_result(message) {
                break;
            }
            keep_start += 1;
        }

        if keep_start >= len {
            self.api_messages.clear();
            return 0;
        }

        if keep_start > 0 {
            self.api_messages.drain(0..keep_start);
            preserve_index.saturating_sub(keep_start)
        } else {
            preserve_index
        }
    }

    pub(super) fn format_tool_result_for_history(
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
            return self.format_read_file_result_for_model_context(&path, output, summary);
        }

        output.clone()
    }

    pub(super) fn format_read_file_result_for_model_context(
        &self,
        path: &str,
        output: &str,
        summary: ReadFileSnapshotSummary,
    ) -> String {
        match summary {
            ReadFileSnapshotSummary::Unchanged { .. } => format_read_file_snapshot_message(
                path,
                summary,
                ReadFileSummaryMessageStyle::History,
            ),
            ReadFileSnapshotSummary::FirstRead { .. } | ReadFileSnapshotSummary::Changed { .. } => {
                let summary_message = match summary {
                    ReadFileSnapshotSummary::FirstRead { chars, lines } => format!(
                        "Read {path}: {chars} chars, {lines} lines. Snapshot included below for model context."
                    ),
                    ReadFileSnapshotSummary::Changed {
                        before_chars,
                        before_lines,
                        after_chars,
                        after_lines,
                    } => format!(
                        "Read {path}: content changed ({before_chars} chars/{before_lines} lines -> {after_chars} chars/{after_lines} lines). Snapshot included below for model context."
                    ),
                    ReadFileSnapshotSummary::Unchanged { .. } => unreachable!(),
                };
                format!(
                    "{summary_message}\nContent for model context:\n--- {path} ---\n{output}\n--- end {path} ---"
                )
            }
        }
    }
}

pub(super) fn message_contains_tool_result(message: &ApiMessage) -> bool {
    match &message.content {
        Content::Blocks(blocks) => blocks
            .iter()
            .any(|block| matches!(block, ContentBlock::ToolResult { .. })),
        Content::Text(_) => false,
    }
}

pub(super) fn resolve_history_limits(is_local_endpoint: bool) -> HistoryLimits {
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
            "VEX_MAX_ASSISTANT_HISTORY_CHARS",
            defaults.max_assistant_history_chars,
            200,
            20_000,
        ),
        max_tool_result_history_chars: env_override_usize(
            "VEX_MAX_TOOL_RESULT_HISTORY_CHARS",
            defaults.max_tool_result_history_chars,
            200,
            40_000,
        ),
        max_api_messages: env_override_usize(
            "VEX_MAX_API_MESSAGES",
            defaults.max_api_messages,
            4,
            128,
        ),
    }
}

pub(super) fn resolve_tool_timeout(is_local_endpoint: bool) -> Duration {
    let default_secs = if is_local_endpoint {
        LOCAL_DEFAULT_TOOL_TIMEOUT_SECS
    } else {
        REMOTE_DEFAULT_TOOL_TIMEOUT_SECS
    };

    let secs = std::env::var("VEX_TOOL_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(default_secs)
        .clamp(2, 300);
    Duration::from_secs(secs)
}

pub(super) fn resolve_max_tool_rounds(is_local_endpoint: bool) -> usize {
    let default_rounds = if is_local_endpoint { 12 } else { 24 };
    std::env::var("VEX_MAX_TOOL_ROUNDS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(default_rounds)
        .clamp(2, 64)
}

pub(super) fn env_override_usize(key: &str, default: usize, min: usize, max: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .map(|v| v.clamp(min, max))
        .unwrap_or(default)
}

pub(super) fn truncate_for_history(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }

    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        return text.to_string();
    }

    let total = chars.len();
    let indicator = format!(
        "\n...[truncated {} chars]...\n",
        total.saturating_sub(max_chars)
    );
    let indicator_len = indicator.chars().count();
    if indicator_len >= max_chars {
        return chars.into_iter().take(max_chars).collect();
    }

    let available = max_chars - indicator_len;
    let keep_head = available / 2;
    let keep_tail = available - keep_head;

    let head: String = chars.iter().take(keep_head).collect();
    let tail: String = chars.iter().skip(total.saturating_sub(keep_tail)).collect();
    format!("{head}{indicator}{tail}")
}
