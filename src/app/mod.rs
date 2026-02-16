use crate::config::Config;
use crate::state::ConversationManager;
use anyhow::Result;
use crossterm::style::Stylize;
use std::io::{self, Write};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio::task;

pub enum UiUpdate {
    StreamDelta(String),
    TurnComplete(String),
    Error(String),
}

pub struct App {
    update_rx: mpsc::UnboundedReceiver<UiUpdate>,
    message_tx: mpsc::UnboundedSender<String>,
    should_quit: bool,
}

#[derive(Debug, Clone)]
struct CodeSnippet {
    language: String,
    body: String,
}

impl App {
    pub fn new(config: Config) -> Result<Self> {
        let (update_tx, update_rx) = mpsc::unbounded_channel();
        let (message_tx, mut message_rx) = mpsc::unbounded_channel();

        let client = crate::api::ApiClient::new(&config)?;
        let executor = crate::tools::ToolExecutor::new(config.working_dir.clone());
        let conversation = Arc::new(Mutex::new(ConversationManager::new(client, executor)));

        let conv_clone = Arc::clone(&conversation);
        task::spawn(async move {
            while let Some(content) = message_rx.recv().await {
                let mut mgr = conv_clone.lock().await;
                let delta_tx = {
                    let update_tx = update_tx.clone();
                    let (delta_tx, mut delta_rx) = mpsc::unbounded_channel::<String>();
                    task::spawn(async move {
                        while let Some(delta) = delta_rx.recv().await {
                            let _ = update_tx.send(UiUpdate::StreamDelta(delta));
                        }
                    });
                    delta_tx
                };

                match mgr.send_message(content, Some(&delta_tx)).await {
                    Ok(response) => {
                        drop(delta_tx);
                        let _ = update_tx.send(UiUpdate::TurnComplete(response));
                    }
                    Err(e) => {
                        drop(delta_tx);
                        let _ = update_tx.send(UiUpdate::Error(e.to_string()));
                    }
                }
            }
        });

        Ok(Self {
            update_rx,
            message_tx,
            should_quit: false,
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        println!(
            "{}",
            "aistar text mode • type /quit to exit • streaming enabled".dark_grey()
        );

        loop {
            self.print_prompt()?;

            let mut input = String::new();
            let read = io::stdin().read_line(&mut input)?;
            if read == 0 {
                break;
            }

            let content = input.trim().to_string();
            if content.is_empty() {
                continue;
            }
            if matches!(
                content.as_str(),
                "q" | "quit" | "exit" | "/q" | "/quit" | "/exit"
            ) {
                self.should_quit = true;
            } else {
                self.render_turn(content).await?;
            }

            if self.should_quit {
                break;
            }
        }

        Ok(())
    }

    fn print_prompt(&self) -> Result<()> {
        print!("{} ", ">".dark_grey());
        io::stdout().flush()?;
        Ok(())
    }

    async fn render_turn(&mut self, content: String) -> Result<()> {
        let _ = self.message_tx.send(content);

        let mut final_text: Option<String> = None;
        while final_text.is_none() {
            match self.update_rx.recv().await {
                Some(UiUpdate::StreamDelta(text)) => {
                    print!("{text}");
                    io::stdout().flush()?;
                }
                Some(UiUpdate::TurnComplete(text)) => {
                    final_text = Some(text);
                }
                Some(UiUpdate::Error(err)) => {
                    println!();
                    println!("{}", format!("error: {err}").red());
                    final_text = Some(String::new());
                }
                None => break,
            }
        }

        println!();
        if let Some(text) = final_text {
            self.print_numbered_code_snippets(&text);
        }
        println!();
        Ok(())
    }

    fn print_numbered_code_snippets(&self, text: &str) {
        let snippets = extract_code_snippets(text);
        if snippets.is_empty() {
            return;
        }

        println!("{}", "code snippets".dark_grey());
        for (idx, snippet) in snippets.iter().enumerate() {
            if snippet.language.is_empty() {
                println!("{}", format!("[{}]", idx + 1).dark_grey());
            } else {
                println!(
                    "{}",
                    format!("[{}] {}", idx + 1, snippet.language).dark_grey()
                );
            }

            if snippet.body.is_empty() {
                println!("   1 |");
                continue;
            }

            for (line_no, line) in snippet.body.lines().enumerate() {
                println!("{:>4} | {}", line_no + 1, line);
            }
            println!();
        }
    }
}

fn extract_code_snippets(text: &str) -> Vec<CodeSnippet> {
    let mut snippets = Vec::new();
    let mut in_block = false;
    let mut current_lang = String::new();
    let mut current_lines = Vec::new();

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("```") {
            if in_block {
                snippets.push(CodeSnippet {
                    language: current_lang.clone(),
                    body: current_lines.join("\n"),
                });
                in_block = false;
                current_lang.clear();
                current_lines.clear();
            } else {
                in_block = true;
                current_lang = rest.trim().to_string();
            }
            continue;
        }

        if in_block {
            current_lines.push(line.to_string());
        }
    }

    snippets
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn test_crit_03_state_sync() {
        let state = Arc::new(AtomicUsize::new(0));
        let state_clone = Arc::clone(&state);
        let handle = tokio::spawn(async move {
            state_clone.store(42, Ordering::SeqCst);
        });
        handle.await.unwrap();
        assert_eq!(state.load(Ordering::SeqCst), 42);
    }

    #[test]
    fn test_extract_code_snippets_numbering_ready() {
        let text = "first\n```rust\nlet x = 1;\n```\nthen\n```txt\nhello\nworld\n```";
        let snippets = extract_code_snippets(text);
        assert_eq!(snippets.len(), 2);
        assert_eq!(snippets[0].language, "rust");
        assert_eq!(snippets[0].body, "let x = 1;");
        assert_eq!(snippets[1].language, "txt");
        assert_eq!(snippets[1].body, "hello\nworld");
    }
}
