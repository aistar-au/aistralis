use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::widgets::Clear;
use std::time::{Duration, Instant};
use vexcoder::app::{build_runtime, TuiMode};
use vexcoder::config::Config;
use vexcoder::runtime::frontend::{FrontendAdapter, ScrollAction, ScrollTarget, UserInputEvent};
use vexcoder::terminal;
use vexcoder::ui::editor::{InputAction, InputEditor};
use vexcoder::ui::layout::split_three_pane_layout;
use vexcoder::ui::render::{
    history_content_width_for_area, input_visual_rows, render_input, render_messages,
    render_overlay_modal, render_status_line, OverlayModal,
};

const STARTUP_NOISE_GUARD: Duration = Duration::from_secs(15);

fn has_numbered_transcript_prefix(line: &str) -> bool {
    let mut saw_digit = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.peek() {
        if ch.is_ascii_digit() {
            saw_digit = true;
            chars.next();
            continue;
        }
        break;
    }
    saw_digit && chars.next() == Some(' ') && chars.next() == Some('|') && chars.next() == Some(' ')
}

fn transcript_signature_hits(text: &str) -> usize {
    let lower = text.to_ascii_lowercase();
    let signatures = [
        "mode:ready approval:",
        "view:scrolled",
        "view:following",
        "running tests/",
        "target/debug/deps/",
        "finished `dev` profile",
        "running `target/debug/vex`",
        "test result:",
        "[error] error sending request for url",
    ];
    signatures
        .iter()
        .filter(|pattern| lower.contains(*pattern))
        .count()
}

fn looks_like_terminal_transcript(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }

    let signature_hits = transcript_signature_hits(trimmed);
    let numbered_lines = trimmed
        .lines()
        .take(64)
        .filter(|line| has_numbered_transcript_prefix(line))
        .count();

    signature_hits >= 2 || (signature_hits >= 1 && numbered_lines >= 2)
}

struct ManagedTuiFrontend {
    terminal: terminal::TerminalType,
    quit: bool,
    editor: InputEditor,
    started_at: Instant,
}

impl ManagedTuiFrontend {
    fn new() -> Result<Self> {
        let terminal = terminal::setup()?;
        Self::drain_startup_events();
        Ok(Self {
            terminal,
            quit: false,
            editor: InputEditor::new(),
            started_at: Instant::now(),
        })
    }

    fn drain_startup_events() {
        for _ in 0..1024 {
            match event::poll(Duration::from_millis(0)) {
                Ok(true) => {
                    if event::read().is_err() {
                        break;
                    }
                }
                Ok(false) | Err(_) => break,
            }
        }
    }

    fn should_ignore_startup_paste(&self, text: &str) -> bool {
        if text.contains('\u{1b}') || looks_like_terminal_transcript(text) {
            return true;
        }

        if self.started_at.elapsed() > STARTUP_NOISE_GUARD {
            return false;
        }

        text.lines().take(64).count() > 12
    }

    fn should_ignore_startup_submission(&self, text: &str) -> bool {
        self.started_at.elapsed() <= STARTUP_NOISE_GUARD && looks_like_terminal_transcript(text)
    }

    fn map_editor_action(&mut self, action: InputAction) -> Option<UserInputEvent> {
        match action {
            InputAction::None => None,
            InputAction::Interrupt => Some(UserInputEvent::Interrupt),
            InputAction::Quit => {
                self.quit = true;
                None
            }
            InputAction::Submit(value) => {
                if self.should_ignore_startup_submission(&value) {
                    None
                } else {
                    Some(UserInputEvent::Text(value))
                }
            }
        }
    }

    fn map_overlay_key(&mut self, key: KeyEvent) -> Option<UserInputEvent> {
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(UserInputEvent::Interrupt)
            }
            KeyCode::Up => Some(UserInputEvent::Scroll {
                target: ScrollTarget::Overlay,
                action: ScrollAction::LineUp,
            }),
            KeyCode::Down => Some(UserInputEvent::Scroll {
                target: ScrollTarget::Overlay,
                action: ScrollAction::LineDown,
            }),
            KeyCode::PageUp => Some(UserInputEvent::Scroll {
                target: ScrollTarget::Overlay,
                action: ScrollAction::PageUp(10),
            }),
            KeyCode::PageDown => Some(UserInputEvent::Scroll {
                target: ScrollTarget::Overlay,
                action: ScrollAction::PageDown(10),
            }),
            KeyCode::Home => Some(UserInputEvent::Scroll {
                target: ScrollTarget::Overlay,
                action: ScrollAction::Home,
            }),
            KeyCode::End => Some(UserInputEvent::Scroll {
                target: ScrollTarget::Overlay,
                action: ScrollAction::End,
            }),
            KeyCode::Esc => Some(UserInputEvent::Text("esc".to_string())),
            KeyCode::Char(ch)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                Some(UserInputEvent::Text(ch.to_string()))
            }
            _ => None,
        }
    }

    fn map_regular_key(&mut self, key: KeyEvent) -> Option<UserInputEvent> {
        match key.code {
            KeyCode::PageUp => Some(UserInputEvent::Scroll {
                target: ScrollTarget::History,
                action: ScrollAction::PageUp(10),
            }),
            KeyCode::PageDown => Some(UserInputEvent::Scroll {
                target: ScrollTarget::History,
                action: ScrollAction::PageDown(10),
            }),
            KeyCode::Up if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(UserInputEvent::Scroll {
                    target: ScrollTarget::History,
                    action: ScrollAction::LineUp,
                })
            }
            KeyCode::Down if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(UserInputEvent::Scroll {
                    target: ScrollTarget::History,
                    action: ScrollAction::LineDown,
                })
            }
            KeyCode::Home if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(UserInputEvent::Scroll {
                    target: ScrollTarget::History,
                    action: ScrollAction::Home,
                })
            }
            KeyCode::End if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(UserInputEvent::Scroll {
                    target: ScrollTarget::History,
                    action: ScrollAction::End,
                })
            }
            _ => {
                let action = self.editor.apply_key(key);
                self.map_editor_action(action)
            }
        }
    }
}

impl Drop for ManagedTuiFrontend {
    fn drop(&mut self) {
        let _ = terminal::restore();
    }
}

impl FrontendAdapter<TuiMode> for ManagedTuiFrontend {
    fn poll_user_input(&mut self, mode: &TuiMode) -> Option<UserInputEvent> {
        if mode.quit_requested() {
            self.quit = true;
            return None;
        }

        let Ok(has_event) = event::poll(Duration::from_millis(16)) else {
            self.quit = true;
            return None;
        };
        if !has_event {
            return None;
        }

        let Ok(ev) = event::read() else {
            self.quit = true;
            return None;
        };

        match ev {
            Event::Key(key) => {
                if key.kind == KeyEventKind::Release {
                    return None;
                }
                if mode.overlay_active() {
                    self.map_overlay_key(key)
                } else {
                    self.map_regular_key(key)
                }
            }
            Event::Paste(text) => {
                if mode.overlay_active() {
                    let trimmed = text.trim();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(UserInputEvent::Text(trimmed.to_string()))
                    }
                } else {
                    if self.should_ignore_startup_paste(&text) {
                        return None;
                    }
                    self.editor.insert_str(&text);
                    None
                }
            }
            _ => None,
        }
    }

    fn render(&mut self, mode: &TuiMode) {
        let input = self.editor.buffer().to_string();
        let cursor = self.editor.cursor();

        let _ = self.terminal.draw(|frame| {
            let area = frame.area();
            frame.render_widget(Clear, area);
            let input_width = area.width.saturating_sub(2).max(1) as usize;
            let input_rows = input_visual_rows(&input, input_width).max(1) as u16;
            let panes = split_three_pane_layout(area, input_rows);
            let history_width = history_content_width_for_area(mode.history_lines(), panes.history);
            mode.set_history_content_width(history_width);

            let status = mode.status_line();
            let history_scroll = mode.history_scroll_offset();

            render_status_line(frame, panes.header, &status);
            render_messages(frame, panes.history, mode.history_lines(), history_scroll);
            render_input(frame, panes.input, &input, cursor);

            if let Some((patch_preview, scroll_offset)) = mode.pending_patch_overlay() {
                render_overlay_modal(
                    frame,
                    OverlayModal::PatchApprove {
                        patch_preview,
                        scroll_offset,
                        viewport_rows: panes.history.height.max(1) as usize,
                    },
                );
            } else if let Some((tool_name, input_preview, auto_approve_enabled)) =
                mode.pending_tool_overlay()
            {
                render_overlay_modal(
                    frame,
                    OverlayModal::ToolPermission {
                        tool_name,
                        input_preview,
                        auto_approve_enabled,
                    },
                );
            }
        });
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
    let mut frontend = ManagedTuiFrontend::new()?;
    runtime.run(&mut frontend, &mut ctx).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::looks_like_terminal_transcript;

    #[test]
    fn transcript_detection_matches_following_view_dump() {
        let input =
            "mode:ready approval:none history:9 view:scrolled\n1 | > list files\ntest result: ok.";
        assert!(looks_like_terminal_transcript(input));
    }

    #[test]
    fn transcript_detection_matches_cargo_test_noise() {
        let input = "Running tests/integration_test.rs (target/debug/deps/integration_test-b458ef4801b11438)\n\
                     test result: ok. 2 passed; 0 failed; 0 ignored;\n\
                     Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.05s";
        assert!(looks_like_terminal_transcript(input));
    }

    #[test]
    fn transcript_detection_keeps_normal_prompt() {
        let input = "list files in this directory and summarize in one sentence";
        assert!(!looks_like_terminal_transcript(input));
    }
}
