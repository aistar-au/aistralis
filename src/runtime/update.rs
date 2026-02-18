use crate::state::{StreamBlock, ToolApprovalRequest};

pub enum UiUpdate {
    StreamDelta(String),
    StreamBlockStart { index: usize, block: StreamBlock },
    StreamBlockDelta { index: usize, delta: String },
    StreamBlockComplete { index: usize },
    ToolApprovalRequest(ToolApprovalRequest),
    TurnComplete,
    Error(String),
}
