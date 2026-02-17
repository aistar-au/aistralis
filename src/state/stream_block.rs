use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamBlock {
    /// Assistant thinking phase text.
    Thinking { content: String, collapsed: bool },
    /// Tool invocation and lifecycle state.
    ToolCall {
        id: String,
        name: String,
        input: serde_json::Value,
        status: ToolStatus,
    },
    /// Tool execution result.
    ToolResult {
        tool_call_id: String,
        output: String,
        is_error: bool,
    },
    /// Final assistant text for the turn.
    FinalText { content: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    Pending,
    WaitingApproval,
    Executing,
    Complete,
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_block_round_trip_serialization() {
        let block = StreamBlock::Thinking {
            content: "test".to_string(),
            collapsed: false,
        };
        let json = serde_json::to_string(&block).unwrap();
        let parsed: StreamBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, block);
    }
}
