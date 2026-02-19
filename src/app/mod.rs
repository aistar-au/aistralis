use crate::api::ApiClient;
use crate::config::Config;
use crate::runtime::context::RuntimeContext;
use crate::runtime::frontend::{FrontendAdapter, UserInputEvent};
use crate::runtime::mode::RuntimeMode;
use crate::runtime::r#loop::Runtime;
use crate::runtime::UiUpdate;
use crate::state::{ConversationManager, ToolApprovalRequest};
use crate::tools::ToolExecutor;
use crate::ui::render::{input_visual_rows, render_input, render_messages, render_status_line};
use anyhow::Result;
use crossterm::event::{poll, read, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    backend::CrosstermBackend, layout::Constraint, layout::Direction, layout::Layout, Terminal,
};
use std::io::Stdout;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

struct PendingApproval {
    tool_name: String,
    input_preview: String,
    response_tx: tokio::sync::oneshot::Sender<bool>,
}

pub struct TuiMode {
    history: Vec<String>,
    pending_approval: Option<PendingApproval>,
    auto_approve_session: bool,
    turn_in_progress: bool,
    active_assistant_index: Option<usize>,
}

impl TuiMode {
    pub fn new() -> Self {
        Self {
            history: Vec::new(),
            pending_approval: None,
            auto_approve_session: false,
            turn_in_progress: false,
            active_assistant_index: None,
        }
    }

    fn status(&self) -> &'static str {
        if self.pending_approval.is_some() {
            "awaiting tool approval (1/y, 2/a, 3/n/esc)"
        } else if self.turn_in_progress {
            "assistant is responding"
        } else {
            "ready"
        }
    }

    fn overlay_active(&self) -> bool {
        self.pending_approval.is_some()
    }

    fn resolve_pending_approval(&mut self, approved: bool) {
        if let Some(pending) = self.pending_approval.take() {
            let _ = pending.response_tx.send(approved);
        }
    }

    fn handle_approval_input(&mut self, input: &str) {
        let normalized = input.trim().to_lowercase();
        let context = self
            .pending_approval
            .as_ref()
            .map(|p| format!("{} {}", p.tool_name, p.input_preview))
            .unwrap_or_else(|| "unknown".to_string());
        match normalized.as_str() {
            "1" | "y" | "yes" => {
                self.history
                    .push(format!("[tool approval accepted once: {context}]"));
                self.resolve_pending_approval(true);
            }
            "2" | "a" | "always" => {
                self.auto_approve_session = true;
                self.history
                    .push(format!("[tool approval enabled for session: {context}]"));
                self.resolve_pending_approval(true);
            }
            "3" | "n" | "no" | "esc" => {
                self.history
                    .push(format!("[tool approval denied: {context}]"));
                self.resolve_pending_approval(false);
            }
            _ => {
                self.history
                    .push("[invalid selection, expected 1/2/3]".to_string());
            }
        }
    }
}

impl Default for TuiMode {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeMode for TuiMode {
    fn on_user_input(&mut self, input: String, ctx: &mut RuntimeContext) {
        if self.overlay_active() {
            self.handle_approval_input(&input);
            return;
        }

        if self.turn_in_progress {
            return;
        }

        self.history.push(format!("> {input}"));
        self.history.push(String::new());
        self.active_assistant_index = Some(self.history.len() - 1);
        self.turn_in_progress = true;
        ctx.start_turn(input);
    }

    fn on_model_update(&mut self, update: UiUpdate, _ctx: &mut RuntimeContext) {
        match update {
            UiUpdate::StreamDelta(text) => {
                let idx = match self.active_assistant_index {
                    Some(idx) => idx,
                    None => {
                        self.history.push(String::new());
                        let idx = self.history.len() - 1;
                        self.active_assistant_index = Some(idx);
                        idx
                    }
                };
                if let Some(line) = self.history.get_mut(idx) {
                    line.push_str(&text);
                }
            }
            UiUpdate::StreamBlockStart { .. }
            | UiUpdate::StreamBlockDelta { .. }
            | UiUpdate::StreamBlockComplete { .. } => {}
            UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
                tool_name,
                input_preview,
                response_tx,
            }) => {
                if self.auto_approve_session {
                    let _ = response_tx.send(true);
                    self.history
                        .push(format!("[auto-approved tool: {tool_name} session]"));
                    return;
                }

                self.resolve_pending_approval(false);
                self.history.push(format!(
                    "[tool approval requested: {tool_name}] {input_preview}"
                ));
                self.pending_approval = Some(PendingApproval {
                    tool_name,
                    input_preview,
                    response_tx,
                });
            }
            UiUpdate::TurnComplete => {
                self.resolve_pending_approval(false);
                self.turn_in_progress = false;
                self.active_assistant_index = None;
            }
            UiUpdate::Error(msg) => {
                self.resolve_pending_approval(false);
                self.history.push(format!("[error] {msg}"));
                self.turn_in_progress = false;
                self.active_assistant_index = None;
            }
        }
    }

    fn on_interrupt(&mut self, ctx: &mut RuntimeContext) {
        if self.turn_in_progress {
            ctx.cancel_turn();
            self.resolve_pending_approval(false);
            self.turn_in_progress = false;
            self.active_assistant_index = None;
            self.history.push("[turn cancelled]".to_string());
        }
    }

    fn is_turn_in_progress(&self) -> bool {
        self.turn_in_progress
    }
}

#[derive(Clone)]
struct EditorSnapshot {
    buffer: String,
    cursor: usize,
}

struct InputEditor {
    buffer: String,
    cursor: usize,
    history: Vec<String>,
    history_index: Option<usize>,
    undo_stack: Vec<EditorSnapshot>,
    redo_stack: Vec<EditorSnapshot>,
}

enum InputAction {
    None,
    Submit(String),
    Interrupt,
    Quit,
}

impl InputEditor {
    fn new() -> Self {
        Self {
            buffer: String::new(),
            cursor: 0,
            history: Vec::new(),
            history_index: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    fn clamp_cursor_to_boundary_left(&self, mut idx: usize) -> usize {
        idx = idx.min(self.buffer.len());
        while idx > 0 && !self.buffer.is_char_boundary(idx) {
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
        while j > 0 && !self.buffer.is_char_boundary(j) {
            j -= 1;
        }
        j
    }

    fn next_char_boundary(&self, idx: usize) -> usize {
        let i = self.clamp_cursor_to_boundary_left(idx);
        if i >= self.buffer.len() {
            return self.buffer.len();
        }
        match self.buffer[i..].chars().next() {
            Some(ch) => i + ch.len_utf8(),
            None => self.buffer.len(),
        }
    }

    fn snapshot(&self) -> EditorSnapshot {
        EditorSnapshot {
            buffer: self.buffer.clone(),
            cursor: self.cursor,
        }
    }

    fn push_undo(&mut self) {
        self.undo_stack.push(self.snapshot());
        self.redo_stack.clear();
    }

    fn restore(&mut self, snap: EditorSnapshot) {
        self.buffer = snap.buffer;
        self.cursor = self.clamp_cursor_to_boundary_left(snap.cursor);
    }

    fn insert_str(&mut self, value: &str) {
        let cursor = self.clamp_cursor_to_boundary_left(self.cursor);
        self.push_undo();
        self.buffer.insert_str(cursor, value);
        self.cursor = cursor + value.len();
    }

    fn backspace(&mut self) {
        let end = self.clamp_cursor_to_boundary_left(self.cursor);
        if end == 0 {
            return;
        }
        let start = self.prev_char_boundary(end);
        self.push_undo();
        self.buffer.replace_range(start..end, "");
        self.cursor = start;
    }

    fn delete(&mut self) {
        let start = self.clamp_cursor_to_boundary_left(self.cursor);
        if start >= self.buffer.len() {
            return;
        }
        let end = self.next_char_boundary(start);
        self.push_undo();
        self.buffer.replace_range(start..end, "");
        self.cursor = start;
    }

    fn submit(&mut self) -> Option<String> {
        let value = self.buffer.trim().to_string();
        if value.is_empty() {
            return None;
        }
        self.history.push(self.buffer.clone());
        self.history_index = None;
        self.push_undo();
        self.buffer.clear();
        self.cursor = 0;
        Some(value)
    }

    fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }

        let next_index = match self.history_index {
            Some(idx) if idx > 0 => idx - 1,
            Some(_) => 0,
            None => self.history.len().saturating_sub(1),
        };
        self.history_index = Some(next_index);
        self.buffer = self.history[next_index].clone();
        self.cursor = self.buffer.len();
    }

    fn history_down(&mut self) {
        let Some(idx) = self.history_index else {
            return;
        };

        if idx + 1 >= self.history.len() {
            self.history_index = None;
            self.buffer.clear();
            self.cursor = 0;
        } else {
            let next = idx + 1;
            self.history_index = Some(next);
            self.buffer = self.history[next].clone();
            self.cursor = self.buffer.len();
        }
    }

    fn undo(&mut self) {
        if let Some(previous) = self.undo_stack.pop() {
            self.redo_stack.push(self.snapshot());
            self.restore(previous);
        }
    }

    fn redo(&mut self) {
        if let Some(next) = self.redo_stack.pop() {
            self.undo_stack.push(self.snapshot());
            self.restore(next);
        }
    }

    fn apply_event(&mut self, event: Event) -> InputAction {
        match event {
            Event::Paste(text) => {
                self.insert_str(&text);
                InputAction::None
            }
            Event::Key(key) => self.apply_key(key),
            _ => InputAction::None,
        }
    }

    fn apply_key(&mut self, key: KeyEvent) -> InputAction {
        match key.code {
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.buffer.is_empty() {
                    return InputAction::Quit;
                }
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return InputAction::Interrupt;
            }
            KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.insert_str("\n");
            }
            KeyCode::Char('z') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.undo();
            }
            KeyCode::Char('y') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.redo();
            }
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.insert_str("\n");
            }
            KeyCode::Enter => {
                if let Some(value) = self.submit() {
                    return InputAction::Submit(value);
                }
            }
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete(),
            KeyCode::Left => {
                self.cursor = self.prev_char_boundary(self.cursor);
            }
            KeyCode::Right => {
                self.cursor = self.next_char_boundary(self.cursor);
            }
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.buffer.len(),
            KeyCode::Up => self.history_up(),
            KeyCode::Down => self.history_down(),
            KeyCode::Char(ch) => self.insert_str(&ch.to_string()),
            KeyCode::Esc => {
                if self.buffer.is_empty() {
                    return InputAction::Submit("esc".to_string());
                }
            }
            _ => {}
        }

        InputAction::None
    }
}

pub struct TuiFrontend {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    quit: bool,
    editor: InputEditor,
}

impl TuiFrontend {
    pub fn new(terminal: Terminal<CrosstermBackend<Stdout>>) -> Self {
        Self {
            terminal,
            quit: false,
            editor: InputEditor::new(),
        }
    }
}

impl FrontendAdapter<TuiMode> for TuiFrontend {
    fn poll_user_input(&mut self, _mode: &TuiMode) -> Option<UserInputEvent> {
        if poll(Duration::from_millis(16)).unwrap_or(false) {
            if let Ok(event) = read() {
                match self.editor.apply_event(event) {
                    InputAction::None => {}
                    InputAction::Submit(value) => return Some(UserInputEvent::Text(value)),
                    InputAction::Interrupt => return Some(UserInputEvent::Interrupt),
                    InputAction::Quit => self.quit = true,
                }
            }
        }
        None
    }

    fn render(&mut self, mode: &TuiMode) {
        let _ = self.terminal.draw(|frame| {
            let input_rows = input_visual_rows(&self.editor.buffer, frame.area().width as usize)
                .clamp(1, 6) as u16;
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(1),
                    Constraint::Length(1),
                    Constraint::Length(input_rows),
                ])
                .split(frame.area());

            render_messages(frame, chunks[0], &mode.history, 0);
            render_status_line(frame, chunks[1], mode.status());
            render_input(frame, chunks[2], &self.editor.buffer, self.editor.cursor);
        });
    }

    fn should_quit(&self) -> bool {
        self.quit
    }
}

pub struct App {
    runtime: Runtime<TuiMode>,
    frontend: TuiFrontend,
    ctx: RuntimeContext,
}

impl App {
    pub fn new(config: Config) -> Result<Self> {
        crate::terminal::install_panic_hook_once();

        let client = ApiClient::new(&config)?;
        let executor = ToolExecutor::new(config.working_dir.clone());
        let conversation = ConversationManager::new(client, executor);

        let (update_tx, update_rx) = mpsc::unbounded_channel::<UiUpdate>();
        let ctx = RuntimeContext::new(conversation, update_tx, CancellationToken::new());

        let mode = TuiMode::new();
        let runtime = Runtime::new(mode, update_rx);

        let terminal = crate::terminal::setup()?;
        let frontend = TuiFrontend::new(terminal);

        Ok(Self {
            runtime,
            frontend,
            ctx,
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        self.runtime.run(&mut self.frontend, &mut self.ctx).await;
        Ok(())
    }
}

impl Drop for App {
    fn drop(&mut self) {
        let _ = crate::terminal::restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{mock_client::MockApiClient, ApiClient};
    use crossterm::event::KeyEvent;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn setup_ctx() -> RuntimeContext {
        let (tx, _rx) = mpsc::unbounded_channel::<UiUpdate>();
        let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
        let conversation = ConversationManager::new_mock(client, HashMap::new());
        RuntimeContext::new(conversation, tx, CancellationToken::new())
    }

    #[tokio::test]
    async fn test_ref_03_tui_mode_overlay_blocks_input() {
        let mut ctx = setup_ctx();
        let mut mode = TuiMode::new();

        let (response_tx, _rx) = tokio::sync::oneshot::channel::<bool>();
        mode.on_model_update(
            UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
                tool_name: "read_file".to_string(),
                input_preview: "{}".to_string(),
                response_tx,
            }),
            &mut ctx,
        );

        mode.on_user_input("blocked".to_string(), &mut ctx);
        assert!(!mode.turn_in_progress, "overlay must block input dispatch");

        mode.on_user_input("1".to_string(), &mut ctx);
        assert!(
            !mode.overlay_active(),
            "overlay should clear after decision"
        );

        mode.on_user_input("resume".to_string(), &mut ctx);
        assert!(
            mode.turn_in_progress,
            "dispatch should resume after overlay clears"
        );
    }

    #[test]
    fn test_ref_08_stream_delta_appends_to_assistant_placeholder_not_user_line() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        mode.on_user_input("hello".to_string(), &mut ctx);
        mode.on_model_update(UiUpdate::StreamDelta("assistant".to_string()), &mut ctx);

        assert_eq!(mode.history[0], "> hello");
        assert_eq!(mode.history[1], "assistant");
    }

    #[test]
    fn test_editor_cursor_navigation() {
        let mut editor = InputEditor::new();
        editor.apply_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        editor.apply_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        editor.apply_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        editor.apply_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        editor.apply_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        editor.apply_key(KeyEvent::new(KeyCode::Char('X'), KeyModifiers::NONE));
        assert_eq!(editor.buffer, "aXbc");
    }

    #[test]
    fn test_editor_history_up_down() {
        let mut editor = InputEditor::new();
        editor.buffer = "first".to_string();
        let _ = editor.submit();
        editor.buffer = "second".to_string();
        let _ = editor.submit();

        editor.apply_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(editor.buffer, "second");
        editor.apply_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(editor.buffer, "first");
        editor.apply_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(editor.buffer, "second");
    }

    #[test]
    fn test_editor_multiline_shortcuts() {
        let mut editor = InputEditor::new();
        editor.apply_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        editor.apply_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
        editor.apply_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        editor.apply_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL));
        editor.apply_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        assert_eq!(editor.buffer, "a\nb\nc");
    }

    #[test]
    fn test_editor_undo_redo() {
        let mut editor = InputEditor::new();
        editor.apply_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        editor.apply_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        editor.apply_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL));
        assert_eq!(editor.buffer, "a");
        editor.apply_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL));
        assert_eq!(editor.buffer, "ab");
    }

    #[test]
    fn test_editor_paste_handling() {
        let mut editor = InputEditor::new();
        let _ = editor.apply_event(Event::Paste("hello".to_string()));
        assert_eq!(editor.buffer, "hello");
    }

    #[test]
    fn test_input_editor_unicode_cursor_backspace_delete_safe() {
        let mut editor = InputEditor::new();
        editor.insert_str("a😀b");
        editor.cursor = editor.buffer.len();
        editor.backspace();
        assert_eq!(editor.buffer, "a😀");
        editor.backspace();
        assert_eq!(editor.buffer, "a");

        editor.insert_str("😀b");
        editor.cursor = 2; // intentionally non-boundary (inside 😀 codepoint)
        editor.delete();
        assert_eq!(editor.buffer, "ab");
    }

    #[tokio::test]
    async fn test_invalid_approval_input_keeps_overlay_active_with_feedback() {
        let mut ctx = setup_ctx();
        let mut mode = TuiMode::new();
        let (response_tx, _response_rx) = tokio::sync::oneshot::channel::<bool>();

        mode.on_model_update(
            UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
                tool_name: "read_file".to_string(),
                input_preview: "{}".to_string(),
                response_tx,
            }),
            &mut ctx,
        );

        mode.on_user_input("x".to_string(), &mut ctx);
        assert!(
            mode.overlay_active(),
            "overlay should stay active on invalid input"
        );
        assert!(
            mode.history
                .iter()
                .any(|line| line.contains("[invalid selection, expected 1/2/3]")),
            "expected invalid selection feedback line"
        );
    }

    #[tokio::test]
    async fn test_interrupt_is_typed_event_not_magic_string_collision() {
        let mut ctx = setup_ctx();
        let mut mode = TuiMode::new();

        mode.on_user_input("__AISTAR_INTERRUPT__".to_string(), &mut ctx);
        assert!(
            mode.turn_in_progress,
            "plain text matching old sentinel must be treated as normal user input"
        );

        mode.on_interrupt(&mut ctx);
        assert!(
            !mode.turn_in_progress,
            "typed interrupt should cancel active turn"
        );
    }

    #[tokio::test]
    async fn test_tool_approval_accept_once() {
        let mut ctx = setup_ctx();
        let mut mode = TuiMode::new();
        let (response_tx, response_rx) = tokio::sync::oneshot::channel::<bool>();

        mode.on_model_update(
            UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
                tool_name: "read_file".to_string(),
                input_preview: "{}".to_string(),
                response_tx,
            }),
            &mut ctx,
        );
        mode.on_user_input("1".to_string(), &mut ctx);

        assert!(response_rx.await.expect("response should resolve"));
    }

    #[tokio::test]
    async fn test_tool_approval_deny() {
        let mut ctx = setup_ctx();
        let mut mode = TuiMode::new();
        let (response_tx, response_rx) = tokio::sync::oneshot::channel::<bool>();

        mode.on_model_update(
            UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
                tool_name: "read_file".to_string(),
                input_preview: "{}".to_string(),
                response_tx,
            }),
            &mut ctx,
        );
        mode.on_user_input("n".to_string(), &mut ctx);

        assert!(!response_rx.await.expect("response should resolve"));
    }
}
