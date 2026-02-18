use crate::state::ToolApprovalRequest;

/// Internal routing events for the runtime loop.
///
/// Distinct from `UiUpdate` which flows to modes.
/// Reserved for future multi-mode dispatch (REF-06+); stub for now.
pub enum RuntimeEvent {
    TurnStarted { id: u64 },
    StreamDelta { text: String },
    ToolApprovalRequest(ToolApprovalRequest),
    TurnComplete,
    Error(String),
}
