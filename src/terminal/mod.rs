use crossterm::{
    cursor::Show,
    event::{DisableBracketedPaste, EnableBracketedPaste},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::{self, Stdout};

pub type TerminalType = Terminal<CrosstermBackend<Stdout>>;

pub fn setup() -> anyhow::Result<TerminalType> {
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = restore();
        original_hook(panic_info);
    }));

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
