use super::mode::RuntimeMode;

pub enum UserInputEvent {
    Text(String),
    Interrupt,
}

pub trait FrontendAdapter<M: RuntimeMode> {
    fn poll_user_input(&mut self, mode: &M) -> Option<UserInputEvent>;
    fn render(&mut self, mode: &M);
    fn should_quit(&self) -> bool;
}
