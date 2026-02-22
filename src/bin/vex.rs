use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::time::Duration;
use vexcoder::app::{build_runtime, TuiMode};
use vexcoder::config::Config;
use vexcoder::runtime::frontend::{FrontendAdapter, ScrollAction, ScrollTarget, UserInputEvent};
use vexcoder::terminal;
use vexcoder::ui::layout::split_three_pane_layout;
use vexcoder::ui::render::{
    input_visual_rows, render_input, render_messages, render_overlay_modal, render_status_line,
    OverlayModal,
};

struct ManagedTuiFrontend {
    terminal: terminal::TerminalType,
    quit: bool,
    input_buffer: String,
    cursor: usize,
}

impl ManagedTuiFrontend {
    fn new() -> Result<Self> {
        let terminal = terminal::setup()?;
        Ok(Self {
            terminal,
            quit: false,
            input_buffer: String::new(),
            cursor: 0,
        })
    }

    fn clamp_cursor_to_boundary_left(&self, mut idx: usize) -> usize {
        idx = idx.min(self.input_buffer.len());
        while idx > 0 && !self.input_buffer.is_char_boundary(idx) {
            idx -= 1;
        }
        idx
    }

    fn prev_char_boundary(&self, idx: usize) -> usize {
        let i = self.clamp_cursor_to_boundary_left(idx);
        if i == 0 {
            return 0;
        }
        let mut j = i - 1;
        while j > 0 && !self.input_buffer.is_char_boundary(j) {
            j -= 1;
        }
        j
    }

    fn next_char_boundary(&self, idx: usize) -> usize {
        let i = self.clamp_cursor_to_boundary_left(idx);
        if i >= self.input_buffer.len() {
            return self.input_buffer.len();
        }
        match self.input_buffer[i..].chars().next() {
            Some(ch) => i + ch.len_utf8(),
            None => self.input_buffer.len(),
        }
    }

    fn insert_str(&mut self, value: &str) {
        let cursor = self.clamp_cursor_to_boundary_left(self.cursor);
        self.input_buffer.insert_str(cursor, value);
        self.cursor = cursor + value.len();
    }

    fn backspace(&mut self) {
        let end = self.clamp_cursor_to_boundary_left(self.cursor);
        if end == 0 {
            return;
        }
        let start = self.prev_char_boundary(end);
        self.input_buffer.replace_range(start..end, "");
        self.cursor = start;
    }

    fn delete(&mut self) {
        let start = self.clamp_cursor_to_boundary_left(self.cursor);
        if start >= self.input_buffer.len() {
            return;
        }
        let end = self.next_char_boundary(start);
        self.input_buffer.replace_range(start..end, "");
        self.cursor = start;
    }

    fn submit_input(&mut self) -> Option<String> {
        let value = self.input_buffer.trim().to_string();
        if value.is_empty() {
            return None;
        }
        self.input_buffer.clear();
        self.cursor = 0;
        Some(value)
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
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(UserInputEvent::Interrupt)
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.input_buffer.is_empty() {
                    self.quit = true;
                }
                None
            }
            KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.insert_str("\n");
                None
            }
            KeyCode::PageUp => Some(UserInputEvent::Scroll {
                target: ScrollTarget::History,
                action: ScrollAction::PageUp(10),
            }),
            KeyCode::PageDown => Some(UserInputEvent::Scroll {
                target: ScrollTarget::History,
                action: ScrollAction::PageDown(10),
            }),
            KeyCode::Up => Some(UserInputEvent::Scroll {
                target: ScrollTarget::History,
                action: ScrollAction::LineUp,
            }),
            KeyCode::Down => Some(UserInputEvent::Scroll {
                target: ScrollTarget::History,
                action: ScrollAction::LineDown,
            }),
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
            KeyCode::Home => {
                self.cursor = 0;
                None
            }
            KeyCode::End => {
                self.cursor = self.input_buffer.len();
                None
            }
            KeyCode::Left => {
                self.cursor = self.prev_char_boundary(self.cursor);
                None
            }
            KeyCode::Right => {
                self.cursor = self.next_char_boundary(self.cursor);
                None
            }
            KeyCode::Backspace => {
                self.backspace();
                None
            }
            KeyCode::Delete => {
                self.delete();
                None
            }
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.insert_str("\n");
                None
            }
            KeyCode::Enter => self.submit_input().map(UserInputEvent::Text),
            KeyCode::Char(ch)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.insert_str(&ch.to_string());
                None
            }
            _ => None,
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
                    self.insert_str(&text);
                    None
                }
            }
            _ => None,
        }
    }

    fn render(&mut self, mode: &TuiMode) {
        let status = mode.status_line();
        let history_scroll = mode.history_scroll_offset();
        let input = self.input_buffer.as_str();
        let cursor = self.cursor;

        let _ = self.terminal.draw(|frame| {
            let area = frame.area();
            let input_width = area.width.saturating_sub(2).max(1) as usize;
            let input_rows = input_visual_rows(input, input_width).max(1) as u16;
            let panes = split_three_pane_layout(area, input_rows);

            render_status_line(frame, panes.header, &status);
            render_messages(frame, panes.history, mode.history_lines(), history_scroll);
            render_input(frame, panes.input, input, cursor);

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
