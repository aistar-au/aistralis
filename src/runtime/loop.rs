use crate::runtime::UiUpdate;
use tokio::sync::mpsc;

use super::mode::RuntimeMode;

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
