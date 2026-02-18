// Adjust import path if ToolApprovalRequest lives elsewhere in your crate.
use crate::state::ToolApprovalRequest;

pub enum RuntimeEvent {
    TurnStarted { id: u64 },
    StreamDelta { text: String },
    ToolApprovalRequest(ToolApprovalRequest),
    TurnComplete,
    Error(String),
}
