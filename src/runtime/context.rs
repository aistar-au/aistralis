use crate::state::ConversationManager;

pub struct RuntimeContext<'a> {
    pub conversation: &'a mut ConversationManager,
}

impl<'a> RuntimeContext<'a> {
    /// Begin a new conversation turn. Wired in REF-04; currently a no-op stub.
    pub fn start_turn(&mut self, _input: String) {
        // wired in REF-04
    }

    /// Cancel the active turn. Wired in REF-04; currently a no-op stub.
    pub fn cancel_turn(&mut self) {
        // wired in REF-04
    }
}
