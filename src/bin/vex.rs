use anyhow::Result;
use std::io::{self, Write};
use std::time::Duration;
use vexcoder::app::{build_runtime, TuiMode};
use vexcoder::config::Config;
use vexcoder::runtime::frontend::{FrontendAdapter, UserInputEvent};
use vexcoder::runtime::mode::RuntimeMode;

struct AppendTerminalFrontend {
    quit: bool,
    rendered_line_count: usize,
    rendered_last_line_bytes: usize,
    streaming_line_open: bool,
}

impl AppendTerminalFrontend {
    fn new() -> Self {
        Self {
            quit: false,
            rendered_line_count: 0,
            rendered_last_line_bytes: 0,
            streaming_line_open: false,
        }
    }

    fn print_prompt(mode: &TuiMode) {
        if mode.overlay_active() {
            print!("approval> ");
        } else {
            print!("> ");
        }
        let _ = io::stdout().flush();
    }

    fn print_stream_delta(&mut self, line: &str) {
        if line.len() > self.rendered_last_line_bytes {
            if let Some(delta) = line.get(self.rendered_last_line_bytes..) {
                print!("{delta}");
                self.rendered_last_line_bytes = line.len();
                self.streaming_line_open = true;
                let _ = io::stdout().flush();
            }
        } else if line.len() < self.rendered_last_line_bytes {
            if self.streaming_line_open {
                println!();
            }
            print!("{line}");
            self.rendered_last_line_bytes = line.len();
            self.streaming_line_open = true;
            let _ = io::stdout().flush();
        }
    }
}

impl FrontendAdapter<TuiMode> for AppendTerminalFrontend {
    fn poll_user_input(&mut self, mode: &TuiMode) -> Option<UserInputEvent> {
        if mode.quit_requested() {
            self.quit = true;
            return None;
        }

        if mode.is_turn_in_progress() {
            std::thread::sleep(Duration::from_millis(25));
            return None;
        }

        Self::print_prompt(mode);

        let mut input = String::new();
        let Ok(read) = io::stdin().read_line(&mut input) else {
            self.quit = true;
            return None;
        };
        if read == 0 {
            self.quit = true;
            return None;
        }

        let value = input
            .trim_end_matches('\n')
            .trim_end_matches('\r')
            .to_string();
        if value.trim().is_empty() {
            return None;
        }
        if value == "/quit" {
            self.quit = true;
            return None;
        }
        if value == "/interrupt" {
            return Some(UserInputEvent::Interrupt);
        }
        Some(UserInputEvent::Text(value))
    }

    fn render(&mut self, mode: &TuiMode) {
        let lines = mode.history_lines();
        if lines.len() < self.rendered_line_count {
            self.rendered_line_count = lines.len();
            self.rendered_last_line_bytes = 0;
            self.streaming_line_open = false;
        }

        for (idx, line) in lines.iter().enumerate().skip(self.rendered_line_count) {
            if line.starts_with("> ") {
                continue;
            }

            let is_streaming_line =
                mode.is_turn_in_progress() && mode.active_assistant_index() == Some(idx);
            if is_streaming_line {
                if !line.is_empty() {
                    print!("{line}");
                    self.rendered_last_line_bytes = line.len();
                    self.streaming_line_open = true;
                    let _ = io::stdout().flush();
                } else {
                    self.rendered_last_line_bytes = 0;
                    self.streaming_line_open = true;
                }
            } else {
                if self.streaming_line_open {
                    println!();
                }
                println!("{line}");
                self.rendered_last_line_bytes = 0;
                self.streaming_line_open = false;
            }
        }
        self.rendered_line_count = lines.len();

        if mode.is_turn_in_progress() {
            if let Some(idx) = mode.active_assistant_index() {
                if let Some(line) = lines.get(idx) {
                    self.print_stream_delta(line);
                }
            }
        } else if self.streaming_line_open {
            println!();
            self.streaming_line_open = false;
            self.rendered_last_line_bytes = 0;
        }
    }

    fn should_quit(&self) -> bool {
        self.quit
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::load()?;
    config.validate()?;

    let (mut runtime, mut ctx) = build_runtime(config)?;
    let mut frontend = AppendTerminalFrontend::new();
    runtime.run(&mut frontend, &mut ctx).await;
    Ok(())
}
