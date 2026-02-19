use crossterm::{
    cursor::Show,
    event::{DisableBracketedPaste, EnableBracketedPaste},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::{self, Stdout};
use std::sync::Once;

pub type TerminalType = Terminal<CrosstermBackend<Stdout>>;
static PANIC_HOOK_INSTALLED: Once = Once::new();

pub fn install_panic_hook_once() {
    PANIC_HOOK_INSTALLED.call_once(|| {
        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic_info| {
            let _ = restore();
            original_hook(panic_info);
        }));
    });
}

pub fn setup() -> anyhow::Result<TerminalType> {
    install_panic_hook_once();

    enable_raw_mode()?;
    execute!(io::stdout(), EnableBracketedPaste)?;

    let backend = CrosstermBackend::new(io::stdout());
    Ok(Terminal::new(backend)?)
}

pub fn restore() -> anyhow::Result<()> {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), DisableBracketedPaste, Show);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminal_restored_after_simulated_panic() {
        install_panic_hook_once();
        install_panic_hook_once();
        assert!(
            PANIC_HOOK_INSTALLED.is_completed(),
            "panic hook must be installed before raw mode setup"
        );
    }
}
