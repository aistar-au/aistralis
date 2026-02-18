use super::mode::RuntimeMode;

pub trait FrontendAdapter {
    fn poll_user_input(&mut self) -> Option<String>;
    fn render<M: RuntimeMode>(&mut self, mode: &M);
    fn should_quit(&self) -> bool;
}
