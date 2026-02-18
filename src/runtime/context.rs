use crate::state::ConversationManager;

pub struct RuntimeContext<'a> {
    pub conversation: &'a mut ConversationManager,
}

impl<'a> RuntimeContext<'a> {
    pub fn start_turn(&mut self, _input: String) {
        // wired in REF-04
    }

    pub fn cancel_turn(&mut self) {
        // wired in REF-04
    }
}
