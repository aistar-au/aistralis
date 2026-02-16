use crate::config::Config;
use crate::state::ConversationManager;
use crate::terminal::{self, TerminalType};
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};
use tokio::task;

pub enum UiUpdate {
    StreamDelta(String),
    TurnComplete(String),
    Error(String),
}

pub struct App {
    terminal: TerminalType,
    conversation: Arc<Mutex<ConversationManager>>,
    update_rx: mpsc::UnboundedReceiver<UiUpdate>,
    message_tx: mpsc::UnboundedSender<String>,
    input: String,
    cursor_pos: usize,
    messages: Vec<String>,
    scroll: usize,
    should_quit: bool,
}

impl App {
    pub fn new(config: Config) -> Result<Self> {
        let terminal = terminal::setup()?;

        let (update_tx, update_rx) = mpsc::unbounded_channel();
        let (message_tx, mut message_rx) = mpsc::unbounded_channel();

        let client = crate::api::ApiClient::new(&config)?;
        let executor = crate::tools::ToolExecutor::new(config.working_dir.clone());
        let conversation = Arc::new(Mutex::new(ConversationManager::new(client, executor)));

        let conv_clone = Arc::clone(&conversation);
        task::spawn(async move {
            while let Some(content) = message_rx.recv().await {
                let mut mgr = conv_clone.lock().await;

                match mgr.send_message(content).await {
                    Ok(response) => {
                        let _ = update_tx.send(UiUpdate::TurnComplete(response));
                    }
                    Err(e) => {
                        let _ = update_tx.send(UiUpdate::Error(e.to_string()));
                    }
                }
            }
        });

        Ok(Self {
            terminal,
            conversation,
            update_rx,
            message_tx,
            input: String::new(),
            cursor_pos: 0,
            messages: Vec::new(),
            scroll: 0,
            should_quit: false,
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        loop {
            self.terminal.draw(|f| {
                self.render(f);
            })?;

            tokio::select! {
                Some(update) = self.update_rx.recv() => {
                    self.handle_update(update);
                }
                _ = tokio::time::sleep(Duration::from_millis(16)) => {
                    if event::poll(Duration::from_millis(0))? {
                        if let Event::Key(key) = event::read()? {
                            self.handle_key(key)?;
                        }
                    }
                }
            }

            if self.should_quit {
                break;
            }
        }

        terminal::restore()?;
        Ok(())
    }

    fn handle_key(&mut self, key: event::KeyEvent) -> Result<()> {
        match (key.code, key.modifiers) {
            (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            (KeyCode::Char(c), KeyModifiers::NONE) | (KeyCode::Char(c), KeyModifiers::SHIFT) => {
                self.input.insert(self.cursor_pos, c);
                self.cursor_pos += 1;
            }
            (KeyCode::Backspace, _) => {
                if self.cursor_pos > 0 {
                    self.input.remove(self.cursor_pos - 1);
                    self.cursor_pos -= 1;
                }
            }
            (KeyCode::Enter, _) => {
                if !self.input.is_empty() {
                    let content: String = self.input.drain(..).collect();
                    self.cursor_pos = 0;
                    self.messages.push(format!("You: {content}"));
                    let _ = self.message_tx.send(content);
                }
            }
            (KeyCode::Up, _) => {
                if self.scroll > 0 {
                    self.scroll -= 1;
                }
            }
            (KeyCode::Down, _) => {
                self.scroll += 1;
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_update(&mut self, update: UiUpdate) {
        match update {
            UiUpdate::StreamDelta(text) => {
                if let Some(last) = self.messages.last_mut() {
                    last.push_str(&text);
                } else {
                    self.messages.push(format!("Assistant: {text}"));
                }
            }
            UiUpdate::TurnComplete(text) => {
                self.messages.push(format!("Assistant: {text}"));
                if self.messages.len() > 20 {
                    self.scroll = self.messages.len() - 20;
                }
            }
            UiUpdate::Error(err) => {
                self.messages.push(format!("⚠️  Error: {err}"));
            }
        }
    }

    fn render(&mut self, frame: &mut ratatui::Frame<'_>) {
        use ratatui::layout::{Constraint, Direction, Layout};

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(10), Constraint::Length(3)])
            .split(frame.area());

        crate::ui::render::render_messages(frame, chunks[0], &self.messages, self.scroll);
        crate::ui::render::render_input(frame, chunks[1], &self.input, self.cursor_pos);
    }

    pub fn conversation(&self) -> &Arc<Mutex<ConversationManager>> {
        &self.conversation
    }
}
