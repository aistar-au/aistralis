use super::mode::RuntimeMode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollTarget {
    History,
    Overlay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollAction {
    LineUp,
    LineDown,
    PageUp(usize),
    PageDown(usize),
    Home,
    End,
}

pub enum UserInputEvent {
    Text(String),
    Interrupt,
    Scroll {
        target: ScrollTarget,
        action: ScrollAction,
    },
}

pub trait FrontendAdapter<M: RuntimeMode> {
    fn poll_user_input(&mut self, mode: &M) -> Option<UserInputEvent>;
    fn render(&mut self, mode: &M);
    fn should_quit(&self) -> bool;
}
