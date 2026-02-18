use crate::app::UiUpdate;
use tokio::sync::mpsc;

use super::{context::RuntimeContext, frontend::FrontendAdapter, mode::RuntimeMode};

pub struct Runtime<M: RuntimeMode> {
    pub mode: M,
    update_rx: mpsc::UnboundedReceiver<UiUpdate>,
}

impl<M: RuntimeMode> Runtime<M> {
    pub fn new(mode: M, update_rx: mpsc::UnboundedReceiver<UiUpdate>) -> Self {
        Self { mode, update_rx }
    }
    // run() wired in REF-05
}
