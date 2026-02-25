use crate::runtime::UiUpdate;

use super::context::RuntimeContext;
use super::frontend::UserInputEvent;

pub trait RuntimeMode {
    fn on_user_input(&mut self, input: String, ctx: &mut RuntimeContext);
    fn on_model_update(&mut self, update: UiUpdate, ctx: &mut RuntimeContext);
    fn on_interrupt(&mut self, _ctx: &mut RuntimeContext) {}
    fn on_frontend_event(&mut self, event: UserInputEvent, ctx: &mut RuntimeContext);
    fn is_turn_in_progress(&self) -> bool;
}
