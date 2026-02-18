use crate::app::UiUpdate;

use super::context::RuntimeContext;

pub trait RuntimeMode {
    fn on_user_input(&mut self, input: String, ctx: &mut RuntimeContext);
    fn on_model_update(&mut self, update: UiUpdate, ctx: &mut RuntimeContext);
    fn is_turn_in_progress(&self) -> bool;
}
