use crate::api::ApiClient;
use crate::config::Config;
use crate::runtime::context::RuntimeContext;
use crate::runtime::frontend::{FrontendAdapter, UserInputEvent};
use crate::runtime::mode::RuntimeMode;
use crate::runtime::r#loop::Runtime;
use crate::runtime::UiUpdate;
use crate::state::{ConversationManager, ToolApprovalRequest};
use crate::tools::ToolExecutor;
use crate::ui::layout::split_three_pane_layout;
use crate::ui::render::{
    input_visual_rows, render_input, render_messages, render_status_line,
    render_tool_approval_modal,
};
use anyhow::Result;
use crossterm::event::{poll, read, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::{backend::CrosstermBackend, Frame, Terminal};
use std::io::Stdout;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

struct PendingApproval {
    tool_name: String,
    input_preview: String,
    response_tx: tokio::sync::oneshot::Sender<bool>,
}

const DEFAULT_MAX_HISTORY_LINES: usize = 2000;
const MAX_HISTORY_LINES_ENV: &str = "AISTAR_MAX_HISTORY_LINES";
const SCROLL_PAGE_UP_CMD_PREFIX: &str = "__AISTAR_SCROLL_PAGE_UP__:";
const SCROLL_PAGE_DOWN_CMD_PREFIX: &str = "__AISTAR_SCROLL_PAGE_DOWN__:";
const SCROLL_HOME_CMD: &str = "__AISTAR_SCROLL_HOME__";
const SCROLL_END_CMD: &str = "__AISTAR_SCROLL_END__";

struct HistoryState {
    lines: Vec<String>,
    turn_in_progress: bool,
    active_assistant_index: Option<usize>,
    scroll_offset: usize,
    auto_follow: bool,
}

impl Default for HistoryState {
    fn default() -> Self {
        Self {
            lines: Vec::new(),
            turn_in_progress: false,
            active_assistant_index: None,
            scroll_offset: 0,
            auto_follow: true,
        }
    }
}

#[derive(Default)]
struct OverlayState {
    pending_approval: Option<PendingApproval>,
    auto_approve_session: bool,
}

#[derive(Default)]
struct InputState {
    buffer: String,
    cursor: usize,
    history: Vec<String>,
    history_index: Option<usize>,
    undo_stack: Vec<EditorSnapshot>,
    redo_stack: Vec<EditorSnapshot>,
}

pub struct TuiMode {
    history_state: HistoryState,
    overlay_state: OverlayState,
    history_line_cap: usize,
    pending_quit: bool,
    quit_requested: bool,
}

impl TuiMode {
    pub fn new() -> Self {
        Self {
            history_state: HistoryState::default(),
            overlay_state: OverlayState::default(),
            history_line_cap: resolve_history_line_cap(),
            pending_quit: false,
            quit_requested: false,
        }
    }

    fn status(&self) -> &'static str {
        if self.overlay_state.pending_approval.is_some() {
            "awaiting tool approval (1/y, 2/a, 3/n/esc)"
        } else if self.pending_quit {
            "press Ctrl+C again to exit"
        } else if self.history_state.turn_in_progress {
            "assistant is responding"
        } else {
            "ready"
        }
    }

    fn overlay_active(&self) -> bool {
        self.overlay_state.pending_approval.is_some()
    }

    fn resolve_pending_approval(&mut self, approved: bool) {
        if let Some(pending) = self.overlay_state.pending_approval.take() {
            let _ = pending.response_tx.send(approved);
        }
    }

    fn handle_approval_input(&mut self, input: &str) {
        let normalized = input.trim().to_lowercase();
        let context = self
            .overlay_state
            .pending_approval
            .as_ref()
            .map(|p| format!("{} {}", p.tool_name, p.input_preview))
            .unwrap_or_else(|| "unknown".to_string());
        match normalized.as_str() {
            "1" | "y" | "yes" => {
                self.push_history_line(format!("[tool approval accepted once: {context}]"));
                self.resolve_pending_approval(true);
            }
            "2" | "a" | "always" => {
                self.overlay_state.auto_approve_session = true;
                self.push_history_line(format!("[tool approval enabled for session: {context}]"));
                self.resolve_pending_approval(true);
            }
            "3" | "n" | "no" | "esc" => {
                self.push_history_line(format!("[tool approval denied: {context}]"));
                self.resolve_pending_approval(false);
            }
            _ => {
                self.push_history_line("[invalid selection, expected 1/2/3]".to_string());
            }
        }
    }

    fn push_history_line(&mut self, line: String) {
        self.history_state.lines.push(line);
        self.enforce_history_cap();
        if self.history_state.auto_follow {
            self.set_scroll_to_bottom();
        } else {
            self.clamp_scroll_offset();
        }
    }

    fn enforce_history_cap(&mut self) {
        let cap = self.history_line_cap;
        if self.history_state.lines.len() <= cap {
            return;
        }

        let excess = self.history_state.lines.len() - cap;
        self.history_state.lines.drain(..excess);
        self.history_state.active_assistant_index = self
            .history_state
            .active_assistant_index
            .and_then(|idx| idx.checked_sub(excess));
        self.history_state.scroll_offset = self.history_state.scroll_offset.saturating_sub(excess);
        self.clamp_scroll_offset();
    }

    fn max_scroll_offset(&self) -> usize {
        self.history_state.lines.len().saturating_sub(1)
    }

    fn set_scroll_to_bottom(&mut self) {
        self.history_state.scroll_offset = self.max_scroll_offset();
    }

    fn clamp_scroll_offset(&mut self) {
        let max = self.max_scroll_offset();
        self.history_state.scroll_offset = self.history_state.scroll_offset.min(max);
    }

    fn apply_page_up(&mut self, page_step: usize) {
        self.history_state.scroll_offset = self
            .history_state
            .scroll_offset
            .saturating_sub(page_step.max(1));
        self.history_state.auto_follow = false;
    }

    fn apply_page_down(&mut self, page_step: usize) {
        let max = self.max_scroll_offset();
        self.history_state.scroll_offset = self
            .history_state
            .scroll_offset
            .saturating_add(page_step.max(1))
            .min(max);
        self.history_state.auto_follow = self.history_state.scroll_offset >= max;
    }

    fn apply_home(&mut self) {
        self.history_state.scroll_offset = 0;
        self.history_state.auto_follow = false;
    }

    fn apply_end(&mut self) {
        self.set_scroll_to_bottom();
        self.history_state.auto_follow = true;
    }

    fn handle_scrollback_command(&mut self, input: &str) -> bool {
        if let Some(step_text) = input.strip_prefix(SCROLL_PAGE_UP_CMD_PREFIX) {
            if let Ok(step) = step_text.parse::<usize>() {
                self.apply_page_up(step);
            }
            return true;
        }
        if let Some(step_text) = input.strip_prefix(SCROLL_PAGE_DOWN_CMD_PREFIX) {
            if let Ok(step) = step_text.parse::<usize>() {
                self.apply_page_down(step);
            }
            return true;
        }
        match input {
            SCROLL_HOME_CMD => {
                self.apply_home();
                true
            }
            SCROLL_END_CMD => {
                self.apply_end();
                true
            }
            _ => false,
        }
    }
}

fn resolve_history_line_cap() -> usize {
    std::env::var(MAX_HISTORY_LINES_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|cap| *cap > 0)
        .unwrap_or(DEFAULT_MAX_HISTORY_LINES)
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
        if self.handle_scrollback_command(&input) {
            return;
        }

        if self.history_state.turn_in_progress {
            self.push_history_line("[busy - turn in progress, input discarded]".to_string());
            return;
        }

        self.pending_quit = false;
        self.quit_requested = false;
        self.push_history_line(format!("> {input}"));
        self.push_history_line(String::new());
        self.history_state.active_assistant_index = Some(self.history_state.lines.len() - 1);
        self.history_state.turn_in_progress = true;
        ctx.start_turn(input);
    }

    fn on_model_update(&mut self, update: UiUpdate, _ctx: &mut RuntimeContext) {
        match update {
            UiUpdate::StreamDelta(text) => {
                let idx = match self.history_state.active_assistant_index {
                    Some(idx) => idx,
                    None => {
                        self.push_history_line(String::new());
                        let idx = self.history_state.lines.len() - 1;
                        self.history_state.active_assistant_index = Some(idx);
                        idx
                    }
                };
                if let Some(line) = self.history_state.lines.get_mut(idx) {
                    line.push_str(&text);
                }
                if self.history_state.auto_follow {
                    self.set_scroll_to_bottom();
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
                if self.overlay_state.auto_approve_session {
                    let _ = response_tx.send(true);
                    self.push_history_line(format!("[auto-approved tool: {tool_name} session]"));
                    return;
                }

                self.resolve_pending_approval(false);
                self.push_history_line(format!(
                    "[tool approval requested: {tool_name}] {input_preview}"
                ));
                self.overlay_state.pending_approval = Some(PendingApproval {
                    tool_name,
                    input_preview,
                    response_tx,
                });
            }
            UiUpdate::TurnComplete => {
                self.resolve_pending_approval(false);
                self.history_state.turn_in_progress = false;
                self.history_state.active_assistant_index = None;
                if self.history_state.auto_follow {
                    self.set_scroll_to_bottom();
                } else {
                    self.clamp_scroll_offset();
                }
            }
            UiUpdate::Error(msg) => {
                self.resolve_pending_approval(false);
                self.push_history_line(format!("[error] {msg}"));
                self.history_state.turn_in_progress = false;
                self.history_state.active_assistant_index = None;
            }
        }
    }

    fn on_interrupt(&mut self, ctx: &mut RuntimeContext) {
        if self.history_state.turn_in_progress {
            ctx.cancel_turn();
            self.resolve_pending_approval(false);
            self.history_state.turn_in_progress = false;
            self.history_state.active_assistant_index = None;
            self.push_history_line("[turn cancelled]".to_string());
            self.pending_quit = false;
            self.quit_requested = false;
            return;
        }

        if self.pending_quit {
            self.quit_requested = true;
        } else {
            self.pending_quit = true;
            self.push_history_line("[press Ctrl+C again to exit]".to_string());
        }
    }

    fn is_turn_in_progress(&self) -> bool {
        self.history_state.turn_in_progress
    }
}

#[derive(Clone)]
struct EditorSnapshot {
    buffer: String,
    cursor: usize,
}

struct InputEditor {
    input_state: InputState,
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
            input_state: InputState::default(),
        }
    }

    fn clamp_cursor_to_boundary_left(&self, mut idx: usize) -> usize {
        idx = idx.min(self.input_state.buffer.len());
        while idx > 0 && !self.input_state.buffer.is_char_boundary(idx) {
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
        while j > 0 && !self.input_state.buffer.is_char_boundary(j) {
            j -= 1;
        }
        j
    }

    fn next_char_boundary(&self, idx: usize) -> usize {
        let i = self.clamp_cursor_to_boundary_left(idx);
        if i >= self.input_state.buffer.len() {
            return self.input_state.buffer.len();
        }
        match self.input_state.buffer[i..].chars().next() {
            Some(ch) => i + ch.len_utf8(),
            None => self.input_state.buffer.len(),
        }
    }

    fn snapshot(&self) -> EditorSnapshot {
        EditorSnapshot {
            buffer: self.input_state.buffer.clone(),
            cursor: self.input_state.cursor,
        }
    }

    fn push_undo(&mut self) {
        self.input_state.undo_stack.push(self.snapshot());
        self.input_state.redo_stack.clear();
    }

    fn restore(&mut self, snap: EditorSnapshot) {
        self.input_state.buffer = snap.buffer;
        self.input_state.cursor = self.clamp_cursor_to_boundary_left(snap.cursor);
    }

    fn insert_str(&mut self, value: &str) {
        let cursor = self.clamp_cursor_to_boundary_left(self.input_state.cursor);
        self.push_undo();
        self.input_state.buffer.insert_str(cursor, value);
        self.input_state.cursor = cursor + value.len();
    }

    fn backspace(&mut self) {
        let end = self.clamp_cursor_to_boundary_left(self.input_state.cursor);
        if end == 0 {
            return;
        }
        let start = self.prev_char_boundary(end);
        self.push_undo();
        self.input_state.buffer.replace_range(start..end, "");
        self.input_state.cursor = start;
    }

    fn delete(&mut self) {
        let start = self.clamp_cursor_to_boundary_left(self.input_state.cursor);
        if start >= self.input_state.buffer.len() {
            return;
        }
        let end = self.next_char_boundary(start);
        self.push_undo();
        self.input_state.buffer.replace_range(start..end, "");
        self.input_state.cursor = start;
    }

    fn submit(&mut self) -> Option<String> {
        let value = self.input_state.buffer.trim().to_string();
        if value.is_empty() {
            return None;
        }
        self.input_state
            .history
            .push(self.input_state.buffer.clone());
        self.input_state.history_index = None;
        self.push_undo();
        self.input_state.buffer.clear();
        self.input_state.cursor = 0;
        Some(value)
    }

    fn history_up(&mut self) {
        if self.input_state.history.is_empty() {
            return;
        }

        let next_index = match self.input_state.history_index {
            Some(idx) if idx > 0 => idx - 1,
            Some(_) => 0,
            None => self.input_state.history.len().saturating_sub(1),
        };
        self.input_state.history_index = Some(next_index);
        self.input_state.buffer = self.input_state.history[next_index].clone();
        self.input_state.cursor = self.input_state.buffer.len();
    }

    fn history_down(&mut self) {
        let Some(idx) = self.input_state.history_index else {
            return;
        };

        if idx + 1 >= self.input_state.history.len() {
            self.input_state.history_index = None;
            self.input_state.buffer.clear();
            self.input_state.cursor = 0;
        } else {
            let next = idx + 1;
            self.input_state.history_index = Some(next);
            self.input_state.buffer = self.input_state.history[next].clone();
            self.input_state.cursor = self.input_state.buffer.len();
        }
    }

    fn undo(&mut self) {
        if let Some(previous) = self.input_state.undo_stack.pop() {
            self.input_state.redo_stack.push(self.snapshot());
            self.restore(previous);
        }
    }

    fn redo(&mut self) {
        if let Some(next) = self.input_state.redo_stack.pop() {
            self.input_state.undo_stack.push(self.snapshot());
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
                if self.input_state.buffer.is_empty() {
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
                self.input_state.cursor = self.prev_char_boundary(self.input_state.cursor);
            }
            KeyCode::Right => {
                self.input_state.cursor = self.next_char_boundary(self.input_state.cursor);
            }
            KeyCode::Home => self.input_state.cursor = 0,
            KeyCode::End => self.input_state.cursor = self.input_state.buffer.len(),
            KeyCode::Up => self.history_up(),
            KeyCode::Down => self.history_down(),
            KeyCode::Char(ch) => self.insert_str(&ch.to_string()),
            KeyCode::Esc => {
                if self.input_state.buffer.is_empty() {
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

    fn current_history_viewport_rows(&self) -> usize {
        let size = self.terminal.size().ok();
        let Some(size) = size else {
            return 1;
        };
        let area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
        let input_rows = input_visual_rows(&self.editor.input_state.buffer, area.width as usize)
            .clamp(1, 6) as u16;
        split_three_pane_layout(area, input_rows)
            .history
            .height
            .max(1) as usize
    }

    fn scroll_command_for_event(&self, event: &Event) -> Option<String> {
        let Event::Key(key) = event else {
            return None;
        };
        let page_step = self.current_history_viewport_rows().max(1);
        match key.code {
            KeyCode::PageUp => Some(format!("{SCROLL_PAGE_UP_CMD_PREFIX}{page_step}")),
            KeyCode::PageDown => Some(format!("{SCROLL_PAGE_DOWN_CMD_PREFIX}{page_step}")),
            KeyCode::Home if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(SCROLL_HOME_CMD.to_string())
            }
            KeyCode::End if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(SCROLL_END_CMD.to_string())
            }
            _ => None,
        }
    }
}

fn overlay_event_to_user_input(event: Event) -> Option<UserInputEvent> {
    match event {
        Event::Key(key) => match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(UserInputEvent::Interrupt)
            }
            KeyCode::Esc => Some(UserInputEvent::Text("esc".to_string())),
            KeyCode::Char(ch)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                Some(UserInputEvent::Text(ch.to_string()))
            }
            _ => None,
        },
        Event::Paste(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(UserInputEvent::Text(trimmed.to_string()))
            }
        }
        _ => None,
    }
}

fn draw_tui_frame(frame: &mut Frame<'_>, mode: &TuiMode, input_state: &InputState) {
    let input_rows =
        input_visual_rows(&input_state.buffer, frame.area().width as usize).clamp(1, 6) as u16;
    let panes = split_three_pane_layout(frame.area(), input_rows);

    for pass in render_pass_order(mode) {
        match pass {
            RenderPass::Header => render_status_line(frame, panes.header, mode.status()),
            RenderPass::History => render_messages(
                frame,
                panes.history,
                &mode.history_state.lines,
                mode.history_state.scroll_offset,
            ),
            RenderPass::Input => {
                render_input(frame, panes.input, &input_state.buffer, input_state.cursor)
            }
            RenderPass::Overlay => {
                if let Some(pending) = mode.overlay_state.pending_approval.as_ref() {
                    render_tool_approval_modal(
                        frame,
                        &pending.tool_name,
                        &pending.input_preview,
                        mode.overlay_state.auto_approve_session,
                    );
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RenderPass {
    Header,
    History,
    Input,
    Overlay,
}

fn render_pass_order(mode: &TuiMode) -> Vec<RenderPass> {
    let mut order = vec![RenderPass::Header, RenderPass::History, RenderPass::Input];
    if mode.overlay_active() {
        order.push(RenderPass::Overlay);
    }
    order
}

fn frontend_should_quit_for_mode(mode: &TuiMode) -> bool {
    mode.quit_requested
}

impl FrontendAdapter<TuiMode> for TuiFrontend {
    fn poll_user_input(&mut self, mode: &TuiMode) -> Option<UserInputEvent> {
        if frontend_should_quit_for_mode(mode) {
            self.quit = true;
            return None;
        }
        if poll(Duration::from_millis(16)).unwrap_or(false) {
            if let Ok(event) = read() {
                if mode.overlay_active() {
                    return overlay_event_to_user_input(event);
                }
                if let Some(command) = self.scroll_command_for_event(&event) {
                    return Some(UserInputEvent::Text(command));
                }
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
        let input_state = &self.editor.input_state;
        let _ = self.terminal.draw(|frame| {
            draw_tui_frame(frame, mode, input_state);
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
    use futures::FutureExt;
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
        assert!(
            !mode.history_state.turn_in_progress,
            "overlay must block input dispatch"
        );

        mode.on_user_input("1".to_string(), &mut ctx);
        assert!(
            !mode.overlay_active(),
            "overlay should clear after decision"
        );

        mode.on_user_input("resume".to_string(), &mut ctx);
        assert!(
            mode.history_state.turn_in_progress,
            "dispatch should resume after overlay clears"
        );
    }

    #[test]
    fn overlay_blocks_submit() {
        let overlay_none = overlay_event_to_user_input(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        )));
        assert!(
            overlay_none.is_none(),
            "overlay keymap must not route Enter as normal submit"
        );

        match overlay_event_to_user_input(Event::Key(KeyEvent::new(
            KeyCode::Char('1'),
            KeyModifiers::NONE,
        ))) {
            Some(UserInputEvent::Text(value)) => assert_eq!(value, "1"),
            _ => panic!("overlay key '1' must route to modal action"),
        }

        match overlay_event_to_user_input(Event::Key(KeyEvent::new(
            KeyCode::Esc,
            KeyModifiers::NONE,
        ))) {
            Some(UserInputEvent::Text(value)) => assert_eq!(value, "esc"),
            _ => panic!("overlay Esc must route to modal deny action"),
        }
    }

    #[test]
    fn test_ref_08_stream_delta_appends_to_assistant_placeholder_not_user_line() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        mode.on_user_input("hello".to_string(), &mut ctx);
        mode.on_model_update(UiUpdate::StreamDelta("assistant".to_string()), &mut ctx);

        assert_eq!(mode.history_state.lines[0], "> hello");
        assert_eq!(mode.history_state.lines[1], "assistant");
    }

    #[test]
    fn test_transcript_does_not_exceed_cap_after_n_turns() {
        let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
        std::env::set_var(MAX_HISTORY_LINES_ENV, "10");

        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        assert_eq!(mode.history_line_cap, 10);

        for i in 0..20 {
            mode.on_user_input(format!("user-{i}"), &mut ctx);
            assert!(
                mode.history_state.lines.len() <= 10,
                "history must be capped after on_user_input"
            );
            if let Some(idx) = mode.history_state.active_assistant_index {
                assert!(
                    idx < mode.history_state.lines.len(),
                    "active assistant index must remain valid after cap enforcement"
                );
            }

            mode.on_model_update(UiUpdate::StreamDelta(format!("assistant-{i}")), &mut ctx);
            assert!(
                mode.history_state.lines.len() <= 10,
                "history must be capped after stream update"
            );
            if let Some(idx) = mode.history_state.active_assistant_index {
                assert!(
                    idx < mode.history_state.lines.len(),
                    "active assistant index must remain valid during streaming"
                );
            }

            mode.on_model_update(UiUpdate::TurnComplete, &mut ctx);
            assert!(
                mode.history_state.lines.len() <= 10,
                "history must stay capped after turn completion"
            );
        }

        std::env::remove_var(MAX_HISTORY_LINES_ENV);
    }

    #[test]
    fn test_history_cap_env_invalid_uses_default() {
        let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
        std::env::set_var(MAX_HISTORY_LINES_ENV, "invalid-cap");

        let mode = TuiMode::new();
        assert_eq!(mode.history_line_cap, DEFAULT_MAX_HISTORY_LINES);

        std::env::remove_var(MAX_HISTORY_LINES_ENV);
    }

    #[test]
    fn test_scrollback_retains_position_during_streaming() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();

        mode.history_state.lines = (0..20).map(|i| format!("line-{i}")).collect();
        mode.history_state.active_assistant_index = Some(10);
        mode.history_state.scroll_offset = 5;
        mode.history_state.auto_follow = false;

        mode.on_model_update(UiUpdate::StreamDelta(" assistant".to_string()), &mut ctx);

        assert_eq!(
            mode.history_state.scroll_offset, 5,
            "scrollback position must not be forced while auto-follow is disabled"
        );
    }

    #[test]
    fn test_scrollback_commands_update_scroll_state() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();

        mode.history_state.lines = (0..100).map(|i| format!("line-{i}")).collect();
        mode.history_state.scroll_offset = 80;
        mode.history_state.auto_follow = true;

        mode.on_user_input(format!("{SCROLL_PAGE_UP_CMD_PREFIX}10"), &mut ctx);
        assert_eq!(mode.history_state.scroll_offset, 70);
        assert!(!mode.history_state.auto_follow);

        mode.on_user_input(format!("{SCROLL_PAGE_DOWN_CMD_PREFIX}200"), &mut ctx);
        assert_eq!(mode.history_state.scroll_offset, 99);
        assert!(mode.history_state.auto_follow);

        mode.on_user_input(SCROLL_HOME_CMD.to_string(), &mut ctx);
        assert_eq!(mode.history_state.scroll_offset, 0);
        assert!(!mode.history_state.auto_follow);

        mode.on_user_input(SCROLL_END_CMD.to_string(), &mut ctx);
        assert_eq!(mode.history_state.scroll_offset, 99);
        assert!(mode.history_state.auto_follow);
        assert!(
            !mode.history_state.turn_in_progress,
            "scroll commands must not dispatch new turns"
        );
    }

    #[test]
    fn test_idle_interrupt_shows_feedback() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();

        assert!(!mode.history_state.turn_in_progress);
        assert!(!mode.pending_quit);
        assert!(!mode.quit_requested);

        mode.on_interrupt(&mut ctx);
        assert!(mode.pending_quit, "first idle interrupt must arm quit");
        assert!(!mode.quit_requested, "first idle interrupt must not quit");
        assert!(
            mode.history_state
                .lines
                .iter()
                .any(|line| line.contains("[press Ctrl+C again to exit]")),
            "first idle interrupt must show user-visible feedback"
        );

        mode.on_interrupt(&mut ctx);
        assert!(
            mode.quit_requested,
            "second idle interrupt must request quit"
        );
        assert!(
            frontend_should_quit_for_mode(&mode),
            "frontend quit path must observe mode quit request"
        );
    }

    #[test]
    fn test_input_drop_shows_feedback() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();

        mode.history_state.turn_in_progress = true;
        mode.on_user_input("hello".to_string(), &mut ctx);

        assert!(
            mode.history_state.turn_in_progress,
            "busy input must not start a new turn"
        );
        assert!(
            mode.history_state
                .lines
                .iter()
                .any(|line| line.starts_with("[busy")),
            "busy input must produce visible rejection feedback"
        );
        assert!(
            !mode
                .history_state
                .lines
                .iter()
                .any(|line| line == "> hello"),
            "discarded busy input must not be appended as user message"
        );
    }

    #[test]
    fn test_pending_quit_resets_on_new_turn_accept() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();

        mode.on_interrupt(&mut ctx);
        assert!(mode.pending_quit);

        mode.on_user_input("resume".to_string(), &mut ctx);
        assert!(
            !mode.pending_quit,
            "pending quit must reset when a new turn is accepted"
        );
        assert!(!mode.quit_requested);
        assert!(mode.history_state.turn_in_progress);
    }

    #[test]
    fn overlay_renders_after_base_panes() {
        let mode = TuiMode::new();
        assert_eq!(
            render_pass_order(&mode),
            vec![RenderPass::Header, RenderPass::History, RenderPass::Input]
        );

        let mut overlay_mode = TuiMode::new();
        let (response_tx, _response_rx) = tokio::sync::oneshot::channel::<bool>();
        overlay_mode.overlay_state.pending_approval = Some(PendingApproval {
            tool_name: "read_file".to_string(),
            input_preview: "{\"path\":\"Cargo.toml\"}".to_string(),
            response_tx,
        });
        assert_eq!(
            render_pass_order(&overlay_mode),
            vec![
                RenderPass::Header,
                RenderPass::History,
                RenderPass::Input,
                RenderPass::Overlay,
            ],
            "overlay must always render last"
        );
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
        assert_eq!(editor.input_state.buffer, "aXbc");
    }

    #[test]
    fn test_editor_history_up_down() {
        let mut editor = InputEditor::new();
        editor.input_state.buffer = "first".to_string();
        let _ = editor.submit();
        editor.input_state.buffer = "second".to_string();
        let _ = editor.submit();

        editor.apply_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(editor.input_state.buffer, "second");
        editor.apply_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(editor.input_state.buffer, "first");
        editor.apply_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(editor.input_state.buffer, "second");
    }

    #[test]
    fn test_editor_multiline_shortcuts() {
        let mut editor = InputEditor::new();
        editor.apply_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        editor.apply_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
        editor.apply_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        editor.apply_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL));
        editor.apply_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        assert_eq!(editor.input_state.buffer, "a\nb\nc");
    }

    #[test]
    fn test_editor_undo_redo() {
        let mut editor = InputEditor::new();
        editor.apply_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        editor.apply_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        editor.apply_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL));
        assert_eq!(editor.input_state.buffer, "a");
        editor.apply_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL));
        assert_eq!(editor.input_state.buffer, "ab");
    }

    #[test]
    fn test_editor_paste_handling() {
        let mut editor = InputEditor::new();
        let _ = editor.apply_event(Event::Paste("hello".to_string()));
        assert_eq!(editor.input_state.buffer, "hello");
    }

    #[test]
    fn test_input_editor_unicode_cursor_backspace_delete_safe() {
        let mut editor = InputEditor::new();
        editor.insert_str("a😀b");
        editor.input_state.cursor = editor.input_state.buffer.len();
        editor.backspace();
        assert_eq!(editor.input_state.buffer, "a😀");
        editor.backspace();
        assert_eq!(editor.input_state.buffer, "a");

        editor.insert_str("😀b");
        editor.input_state.cursor = 2; // intentionally non-boundary (inside 😀 codepoint)
        editor.delete();
        assert_eq!(editor.input_state.buffer, "ab");
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
            mode.history_state
                .lines
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
            mode.history_state.turn_in_progress,
            "plain text matching old sentinel must be treated as normal user input"
        );

        mode.on_interrupt(&mut ctx);
        assert!(
            !mode.history_state.turn_in_progress,
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

    #[tokio::test]
    async fn approval_sender_resolved_exactly_once() {
        let mut ctx = setup_ctx();
        let mut mode = TuiMode::new();

        let (first_tx, first_rx) = tokio::sync::oneshot::channel::<bool>();
        mode.on_model_update(
            UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
                tool_name: "read_file".to_string(),
                input_preview: "first".to_string(),
                response_tx: first_tx,
            }),
            &mut ctx,
        );

        let mut first_rx = Box::pin(first_rx);
        assert!(
            first_rx.as_mut().now_or_never().is_none(),
            "first approval sender must remain unresolved while overlay is active"
        );

        let (second_tx, second_rx) = tokio::sync::oneshot::channel::<bool>();
        mode.on_model_update(
            UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
                tool_name: "write_file".to_string(),
                input_preview: "second".to_string(),
                response_tx: second_tx,
            }),
            &mut ctx,
        );

        assert!(
            !first_rx
                .await
                .expect("first sender should resolve when replaced"),
            "replaced approval sender must resolve false exactly once"
        );

        let mut second_rx = Box::pin(second_rx);
        assert!(
            second_rx.as_mut().now_or_never().is_none(),
            "second approval sender must remain unresolved before decision"
        );

        mode.on_user_input("1".to_string(), &mut ctx);
        assert!(
            second_rx
                .await
                .expect("second sender should resolve on accept"),
            "approved overlay should resolve true exactly once"
        );

        mode.on_model_update(UiUpdate::TurnComplete, &mut ctx);
        mode.on_model_update(UiUpdate::Error("post-resolution".to_string()), &mut ctx);
        assert!(
            !mode.overlay_active(),
            "overlay lifecycle should clear cleanly after sender resolution"
        );
    }
}
