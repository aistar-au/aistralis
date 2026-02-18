use crate::state::{StreamBlock, ToolApprovalRequest};

/// Events flowing from the model/tool layer up to the UI layer.
///
/// Defined in `runtime` (not `app`) so runtime types can reference it
/// without depending on the UI layer. `src/app/mod.rs` imports this as
/// `use crate::runtime::UiUpdate`.
pub enum UiUpdate {
    StreamDelta(String),
    StreamBlockStart { index: usize, block: StreamBlock },
    StreamBlockDelta { index: usize, delta: String },
    StreamBlockComplete { index: usize },
    ToolApprovalRequest(ToolApprovalRequest),
    TurnComplete,
    Error(String),
}
