use crate::api::ApiClient;
use crate::config::Config;
use crate::runtime::context::RuntimeContext;
use crate::runtime::frontend::{ScrollAction, ScrollTarget, UserInputEvent};
use crate::runtime::mode::RuntimeMode;
use crate::runtime::policy::sanitize_assistant_text;
use crate::runtime::r#loop::Runtime;
use crate::runtime::UiUpdate;
use crate::state::{ConversationManager, StreamBlock, ToolApprovalRequest};
use crate::tools::ToolOperator;
use crate::ui::render::history_visual_line_count;
#[cfg(test)]
use crate::ui::render::input_visual_rows;
use anyhow::Result;
#[cfg(test)]
use crossterm::event::{Event, KeyCode, KeyModifiers};
use std::cell::Cell;
#[cfg(test)]
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

struct PendingApproval {
    tool_name: String,
    input_preview: String,
    response_tx: tokio::sync::oneshot::Sender<bool>,
}

struct PendingPatchApproval {
    patch_preview: String,
    scroll_offset: usize,
    response_tx: Option<tokio::sync::oneshot::Sender<bool>>,
}

const DEFAULT_MAX_HISTORY_LINES: usize = 2000;
const MAX_HISTORY_LINES_ENV: &str = "VEX_MAX_HISTORY_LINES";
const HISTORY_CONTENT_WIDTH_FALLBACK: usize = usize::MAX;
#[cfg(test)]
const MAX_INPUT_PANE_ROWS: usize = 6;

struct HistoryState {
    lines: Vec<String>,
    turn_in_progress: bool,
    cancel_pending: bool,
    active_assistant_index: Option<usize>,
    scroll_offset: usize,
    auto_follow: bool,
}

impl Default for HistoryState {
    fn default() -> Self {
        Self {
            lines: Vec::new(),
            turn_in_progress: false,
            cancel_pending: false,
            active_assistant_index: None,
            scroll_offset: 0,
            auto_follow: true,
        }
    }
}

#[derive(Default)]
struct OverlayState {
    pending_approval: Option<PendingApproval>,
    pending_patch_approval: Option<PendingPatchApproval>,
    auto_approve_session: bool,
}

pub struct TuiMode {
    history_state: HistoryState,
    overlay_state: OverlayState,
    history_line_cap: usize,
    repo_label: String,
    history_content_width: Cell<usize>,
    active_stream_blocks: std::collections::HashMap<usize, StreamBlock>,
    pending_quit: bool,
    quit_requested: bool,
}

impl TuiMode {
    pub fn new() -> Self {
        Self {
            history_state: HistoryState::default(),
            overlay_state: OverlayState::default(),
            history_line_cap: resolve_history_line_cap(),
            repo_label: resolve_repo_label(),
            history_content_width: Cell::new(HISTORY_CONTENT_WIDTH_FALLBACK),
            active_stream_blocks: std::collections::HashMap::new(),
            pending_quit: false,
            quit_requested: false,
        }
    }

    fn mode_status_label(&self) -> &'static str {
        if self.overlay_active() {
            "overlay"
        } else if self.pending_quit {
            "quit-arm"
        } else if self.history_state.cancel_pending {
            "cancelling"
        } else if self.history_state.turn_in_progress {
            "streaming"
        } else {
            "ready"
        }
    }

    fn approval_status_label(&self) -> &'static str {
        if self.overlay_active() {
            "pending"
        } else if self.overlay_state.auto_approve_session {
            "auto"
        } else {
            "none"
        }
    }

    pub fn status_line(&self) -> String {
        let history_rows =
            history_visual_line_count(&self.history_state.lines, self.history_content_width.get());
        format!(
            "mode:{} approval:{} history:{} repo:{}",
            self.mode_status_label(),
            self.approval_status_label(),
            history_rows,
            self.repo_label
        )
    }

    pub fn overlay_active(&self) -> bool {
        self.overlay_state.pending_approval.is_some()
            || self.overlay_state.pending_patch_approval.is_some()
    }

    fn patch_overlay_active(&self) -> bool {
        self.overlay_state.pending_patch_approval.is_some()
    }

    pub fn history_lines(&self) -> &[String] {
        &self.history_state.lines
    }

    pub fn active_assistant_index(&self) -> Option<usize> {
        self.history_state.active_assistant_index
    }

    pub fn history_scroll_offset(&self) -> usize {
        self.history_state.scroll_offset
    }

    pub fn quit_requested(&self) -> bool {
        self.quit_requested
    }

    pub fn pending_patch_overlay(&self) -> Option<(&str, usize)> {
        self.overlay_state
            .pending_patch_approval
            .as_ref()
            .map(|pending| (pending.patch_preview.as_str(), pending.scroll_offset))
    }

    pub fn pending_tool_overlay(&self) -> Option<(&str, &str, bool)> {
        self.overlay_state.pending_approval.as_ref().map(|pending| {
            (
                pending.tool_name.as_str(),
                pending.input_preview.as_str(),
                self.overlay_state.auto_approve_session,
            )
        })
    }

    pub fn set_history_content_width(&self, width: usize) {
        self.history_content_width.set(width.max(1));
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
            .map(|p| summarize_tool_approval_context(&p.tool_name, &p.input_preview))
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

    fn resolve_pending_patch_approval(&mut self, approved: bool) {
        if let Some(mut pending) = self.overlay_state.pending_patch_approval.take() {
            if let Some(tx) = pending.response_tx.take() {
                let _ = tx.send(approved);
            }
            let decision = if approved { "accepted" } else { "denied" };
            self.push_history_line(format!("[patch approval {decision}]"));
        }
    }

    fn apply_patch_overlay_scroll_action(&mut self, action: ScrollAction) {
        if let Some(pending) = self.overlay_state.pending_patch_approval.as_mut() {
            let max = pending.patch_preview.lines().count().saturating_sub(1);
            match action {
                ScrollAction::LineUp => {
                    pending.scroll_offset = pending.scroll_offset.saturating_sub(1);
                }
                ScrollAction::LineDown => {
                    pending.scroll_offset = pending.scroll_offset.saturating_add(1).min(max);
                }
                ScrollAction::PageUp(step) => {
                    pending.scroll_offset = pending.scroll_offset.saturating_sub(step.max(1));
                }
                ScrollAction::PageDown(step) => {
                    pending.scroll_offset =
                        pending.scroll_offset.saturating_add(step.max(1)).min(max);
                }
                ScrollAction::Home => {
                    pending.scroll_offset = 0;
                }
                ScrollAction::End => {
                    pending.scroll_offset = max;
                }
            }
        }
    }

    fn handle_patch_overlay_input(&mut self, input: &str) {
        if self.overlay_state.pending_patch_approval.is_none() {
            return;
        }

        let normalized = input.trim().to_lowercase();
        match normalized.as_str() {
            "1" | "y" | "yes" => self.resolve_pending_patch_approval(true),
            "3" | "n" | "no" | "esc" => self.resolve_pending_patch_approval(false),
            _ => {}
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
        history_visual_line_count(&self.history_state.lines, self.history_content_width.get())
            .saturating_sub(1)
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

    fn apply_history_scroll_action(&mut self, action: ScrollAction) {
        match action {
            ScrollAction::LineUp => self.apply_page_up(1),
            ScrollAction::LineDown => self.apply_page_down(1),
            ScrollAction::PageUp(step) => self.apply_page_up(step),
            ScrollAction::PageDown(step) => self.apply_page_down(step),
            ScrollAction::Home => self.apply_home(),
            ScrollAction::End => self.apply_end(),
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

fn resolve_repo_label() -> String {
    std::env::var("VEX_REPO_LABEL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .and_then(|path| {
                    path.file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                })
                .filter(|name| !name.trim().is_empty())
        })
        .unwrap_or_else(|| "workspace".to_string())
}

#[cfg(test)]
fn input_rows_for_buffer(input: &str, width: usize) -> u16 {
    input_visual_rows(input, width).clamp(1, MAX_INPUT_PANE_ROWS) as u16
}

#[cfg(test)]
struct RenderGuard {
    dirty: bool,
    cursor_tick: Duration,
    status_tick: Duration,
    last_draw_at: Instant,
    last_render_state_hash: Option<u64>,
}

#[cfg(test)]
impl RenderGuard {
    fn with_intervals(cursor_tick: Duration, status_tick: Duration, now: Instant) -> Self {
        Self {
            dirty: true,
            cursor_tick,
            status_tick,
            last_draw_at: now,
            last_render_state_hash: None,
        }
    }

    fn poll_timeout(&self) -> Duration {
        self.cursor_tick.min(self.status_tick)
    }

    fn should_draw(&mut self, now: Instant, state_hash: u64) -> bool {
        if self.last_render_state_hash != Some(state_hash) {
            self.dirty = true;
        }

        if self.dirty || now.saturating_duration_since(self.last_draw_at) >= self.poll_timeout() {
            self.dirty = false;
            self.last_draw_at = now;
            self.last_render_state_hash = Some(state_hash);
            true
        } else {
            false
        }
    }
}

impl Default for TuiMode {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeMode for TuiMode {
    fn on_frontend_event(&mut self, event: UserInputEvent, ctx: &mut RuntimeContext) {
        match event {
            UserInputEvent::Text(input) => self.on_user_input(input, ctx),
            UserInputEvent::Interrupt => self.on_interrupt(ctx),
            UserInputEvent::Scroll { target, action } => {
                if self.overlay_active() {
                    if target == ScrollTarget::Overlay {
                        self.apply_patch_overlay_scroll_action(action);
                    }
                } else if target == ScrollTarget::History {
                    self.apply_history_scroll_action(action);
                }
            }
        }
    }

    fn on_user_input(&mut self, input: String, ctx: &mut RuntimeContext) {
        if self.overlay_active() {
            if self.patch_overlay_active() {
                self.handle_patch_overlay_input(&input);
            } else {
                self.handle_approval_input(&input);
            }
            return;
        }

        if self.history_state.turn_in_progress {
            if self.history_state.cancel_pending {
                self.push_history_line(
                    "[busy - cancelling current turn, input discarded]".to_string(),
                );
            } else {
                self.push_history_line("[busy - turn in progress, input discarded]".to_string());
            }
            return;
        }

        self.pending_quit = false;
        self.quit_requested = false;
        self.history_state.cancel_pending = false;
        self.push_history_line(format!("> {input}"));
        self.push_history_line(String::new());
        self.history_state.active_assistant_index = Some(self.history_state.lines.len() - 1);
        self.history_state.turn_in_progress = true;
        ctx.start_turn(input);
    }

    fn on_model_update(&mut self, update: UiUpdate, _ctx: &mut RuntimeContext) {
        match update {
            UiUpdate::StreamDelta(text) => {
                if self.history_state.cancel_pending {
                    return;
                }
                let idx = match self.history_state.active_assistant_index {
                    Some(idx) => idx,
                    None => {
                        if !self.history_state.turn_in_progress {
                            return;
                        }
                        self.push_history_line(String::new());
                        let idx = self.history_state.lines.len() - 1;
                        self.history_state.active_assistant_index = Some(idx);
                        idx
                    }
                };
                if let Some(line) = self.history_state.lines.get_mut(idx) {
                    line.push_str(&text);
                    *line = sanitize_assistant_text(line);
                }
                if self.history_state.auto_follow {
                    self.set_scroll_to_bottom();
                }
            }
            UiUpdate::StreamBlockStart { index, block } => {
                self.active_stream_blocks.insert(index, block);
            }
            UiUpdate::StreamBlockDelta { index, delta } => {
                if let Some(block) = self.active_stream_blocks.get_mut(&index) {
                    match block {
                        StreamBlock::Thinking { content, .. } => content.push_str(&delta),
                        StreamBlock::FinalText { content } => content.push_str(&delta),
                        StreamBlock::ToolCall { .. } | StreamBlock::ToolResult { .. } => {}
                    }
                }
            }
            UiUpdate::StreamBlockComplete { index } => {
                self.active_stream_blocks.remove(&index);
            }
            UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
                tool_name,
                input_preview,
                response_tx,
            }) => {
                if self.history_state.cancel_pending {
                    let _ = response_tx.send(false);
                    return;
                }
                if self.overlay_state.auto_approve_session {
                    let _ = response_tx.send(true);
                    self.push_history_line(format!("[auto-approved tool: {tool_name} session]"));
                    return;
                }

                self.resolve_pending_approval(false);
                self.resolve_pending_patch_approval(false);
                let summary = summarize_tool_approval_context(&tool_name, &input_preview);
                self.push_history_line(format!("[tool approval requested: {summary}]"));
                self.overlay_state.pending_approval = Some(PendingApproval {
                    tool_name,
                    input_preview,
                    response_tx,
                });
            }
            UiUpdate::TurnComplete => {
                self.resolve_pending_approval(false);
                self.resolve_pending_patch_approval(false);
                self.active_stream_blocks.clear();
                self.history_state.cancel_pending = false;
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
                self.resolve_pending_patch_approval(false);
                self.active_stream_blocks.clear();
                self.history_state.cancel_pending = false;
                self.push_history_line(format!("[error] {msg}"));
                self.history_state.turn_in_progress = false;
                self.history_state.active_assistant_index = None;
            }
        }
    }

    fn on_interrupt(&mut self, ctx: &mut RuntimeContext) {
        if self.history_state.turn_in_progress {
            if self.history_state.cancel_pending {
                return;
            }
            ctx.cancel_turn();
            self.resolve_pending_approval(false);
            self.resolve_pending_patch_approval(false);
            self.history_state.cancel_pending = true;
            self.push_history_line("[turn cancellation requested]".to_string());
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

fn summarize_tool_approval_context(tool_name: &str, input_preview: &str) -> String {
    let mut path: Option<&str> = None;
    let mut summary_line: Option<&str> = None;

    for line in input_preview.lines().take(8) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if path.is_none() && trimmed.starts_with("path:") {
            path = Some(trimmed);
            continue;
        }
        if summary_line.is_none()
            && (trimmed.starts_with("change:") || trimmed.starts_with("content:"))
        {
            summary_line = Some(trimmed);
            continue;
        }
        if summary_line.is_none() {
            summary_line = Some(trimmed);
        }
    }

    match (path, summary_line) {
        (Some(path), Some(summary)) => format!("{tool_name} {path} {summary}"),
        (Some(path), None) => format!("{tool_name} {path}"),
        (None, Some(summary)) => format!("{tool_name} {summary}"),
        (None, None) => tool_name.to_string(),
    }
}

#[cfg(test)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg(test)]
enum RenderPass {
    Header,
    History,
    Input,
    Overlay,
}

#[cfg(test)]
fn render_pass_order(mode: &TuiMode) -> Vec<RenderPass> {
    let mut order = vec![RenderPass::Header, RenderPass::History, RenderPass::Input];
    if mode.overlay_active() {
        order.push(RenderPass::Overlay);
    }
    order
}

pub fn build_runtime(config: Config) -> Result<(Runtime<TuiMode>, RuntimeContext)> {
    let client = ApiClient::new(&config)?;
    let operator = ToolOperator::new(config.working_dir.clone());
    let conversation = ConversationManager::new(client, operator);

    let (update_tx, update_rx) = mpsc::unbounded_channel::<UiUpdate>();
    let ctx = RuntimeContext::new(conversation, update_tx, CancellationToken::new());

    let mode = TuiMode::new();
    let runtime = Runtime::new(mode, update_rx);
    Ok((runtime, ctx))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{mock_client::MockApiClient, ApiClient};
    use crate::ui::editor::{InputAction, InputEditor};
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
    fn test_stream_delta_strips_tagged_tool_markup_from_history() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        mode.on_user_input("show diff".to_string(), &mut ctx);
        mode.on_model_update(
            UiUpdate::StreamDelta(
                "I will check.\n<function=git_diff>\n</function>\nDone.".to_string(),
            ),
            &mut ctx,
        );

        assert_eq!(mode.history_state.lines[1], "I will check.\n\nDone.");
        assert!(!mode.history_state.lines[1].contains("<function="));
    }

    #[test]
    fn test_stream_delta_hides_incomplete_tool_tag_suffix() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        mode.on_user_input("status".to_string(), &mut ctx);
        mode.on_model_update(
            UiUpdate::StreamDelta("Checking\n<function=git_status".to_string()),
            &mut ctx,
        );

        assert_eq!(mode.history_state.lines[1], "Checking\n");
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

        mode.on_frontend_event(
            UserInputEvent::Scroll {
                target: ScrollTarget::History,
                action: ScrollAction::PageUp(10),
            },
            &mut ctx,
        );
        assert_eq!(mode.history_state.scroll_offset, 70);
        assert!(!mode.history_state.auto_follow);

        mode.on_frontend_event(
            UserInputEvent::Scroll {
                target: ScrollTarget::History,
                action: ScrollAction::PageDown(200),
            },
            &mut ctx,
        );
        assert_eq!(mode.history_state.scroll_offset, 99);
        assert!(mode.history_state.auto_follow);

        mode.on_frontend_event(
            UserInputEvent::Scroll {
                target: ScrollTarget::History,
                action: ScrollAction::Home,
            },
            &mut ctx,
        );
        assert_eq!(mode.history_state.scroll_offset, 0);
        assert!(!mode.history_state.auto_follow);

        mode.on_frontend_event(
            UserInputEvent::Scroll {
                target: ScrollTarget::History,
                action: ScrollAction::End,
            },
            &mut ctx,
        );
        assert_eq!(mode.history_state.scroll_offset, 99);
        assert!(mode.history_state.auto_follow);
        assert!(
            !mode.history_state.turn_in_progress,
            "scroll commands must not dispatch new turns"
        );
    }

    #[test]
    fn test_history_status_and_scroll_use_visual_rows() {
        let mode = TuiMode {
            history_state: HistoryState {
                lines: vec!["a\nb\nc".to_string()],
                ..HistoryState::default()
            },
            ..TuiMode::new()
        };

        assert_eq!(mode.max_scroll_offset(), 2);
        assert!(mode.status_line().contains("history:3"));
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
            mode.quit_requested(),
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
    fn test_render_not_called_when_state_unchanged() {
        let start = Instant::now();
        let mut guard = RenderGuard::with_intervals(
            Duration::from_millis(500),
            Duration::from_millis(120),
            start,
        );

        assert!(
            guard.should_draw(start, 11),
            "first render should draw because the guard starts dirty"
        );
        assert!(
            !guard.should_draw(start + Duration::from_millis(20), 11),
            "unchanged state before tick interval must not draw"
        );
        assert!(
            !guard.should_draw(start + Duration::from_millis(100), 11),
            "unchanged state still below tick interval must not draw"
        );
        assert!(
            guard.should_draw(start + Duration::from_millis(121), 11),
            "unchanged state should draw when tick interval elapses"
        );
        assert!(
            guard.should_draw(start + Duration::from_millis(122), 12),
            "changed state should mark dirty and draw immediately"
        );
    }

    #[test]
    fn test_render_guard_poll_timeout_uses_min_tick_interval() {
        let guard = RenderGuard::with_intervals(
            Duration::from_millis(500),
            Duration::from_millis(120),
            Instant::now(),
        );
        assert_eq!(guard.poll_timeout(), Duration::from_millis(120));
    }

    #[test]
    fn header_stable_during_streaming() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();

        let ready_status = mode.status_line();
        assert!(
            ready_status.contains("mode:ready"),
            "ready state must publish mode token"
        );
        assert!(
            ready_status.contains("approval:none"),
            "ready state must publish approval token"
        );
        assert!(
            ready_status.contains("history:0"),
            "ready state must publish history count"
        );
        assert!(
            ready_status.contains("repo:"),
            "ready state must publish repo token"
        );
        assert_eq!(
            render_pass_order(&mode).first(),
            Some(&RenderPass::Header),
            "header row must remain first in render order"
        );

        mode.on_user_input("hello".to_string(), &mut ctx);
        mode.on_model_update(UiUpdate::StreamDelta("assistant".to_string()), &mut ctx);
        let streaming_status = mode.status_line();
        assert!(
            streaming_status.contains("mode:streaming"),
            "streaming state must publish mode token"
        );
        assert!(
            streaming_status.contains("approval:none"),
            "streaming state must preserve approval token"
        );
        assert!(
            streaming_status.contains("history:2"),
            "streaming state must keep compact history count"
        );
        assert_eq!(
            render_pass_order(&mode).first(),
            Some(&RenderPass::Header),
            "header row must remain first while streaming"
        );

        let (response_tx, _response_rx) = tokio::sync::oneshot::channel::<bool>();
        mode.on_model_update(
            UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
                tool_name: "read_file".to_string(),
                input_preview: "{}".to_string(),
                response_tx,
            }),
            &mut ctx,
        );
        let overlay_status = mode.status_line();
        assert!(
            overlay_status.contains("mode:overlay"),
            "overlay state must publish overlay mode token"
        );
        assert!(
            overlay_status.contains("approval:pending"),
            "overlay state must publish pending approval token"
        );
        assert_eq!(
            render_pass_order(&mode).first(),
            Some(&RenderPass::Header),
            "header row must remain first under overlay"
        );
    }

    #[test]
    fn multiline_submit_outside_overlay_only() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        let mut editor = InputEditor::new();

        editor.apply_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        editor.apply_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
        editor.apply_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        editor.apply_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL));
        editor.apply_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));

        let submitted = match editor.apply_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)) {
            InputAction::Submit(value) => value,
            _ => panic!("enter outside overlay must submit multiline buffer"),
        };
        assert_eq!(submitted, "a\nb\nc");

        mode.on_user_input(submitted.clone(), &mut ctx);
        assert!(
            mode.history_state.turn_in_progress,
            "outside overlay, enter must submit and start a turn"
        );
        assert!(
            mode.history_state
                .lines
                .iter()
                .any(|line| line == "> a\nb\nc"),
            "submitted multiline prompt should be recorded in history"
        );

        mode.history_state.turn_in_progress = false;
        mode.history_state.active_assistant_index = None;
        let (response_tx, _response_rx) = tokio::sync::oneshot::channel::<bool>();
        mode.overlay_state.pending_approval = Some(PendingApproval {
            tool_name: "read_file".to_string(),
            input_preview: "{}".to_string(),
            response_tx,
        });

        let overlay_enter = overlay_event_to_user_input(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        )));
        assert!(
            overlay_enter.is_none(),
            "enter in overlay keymap must not route to submit"
        );

        mode.on_user_input("overlay\nattempt".to_string(), &mut ctx);
        assert!(
            mode.overlay_active(),
            "overlay should remain active after non-decision input"
        );
        assert!(
            !mode
                .history_state
                .lines
                .iter()
                .any(|line| line == "> overlay\nattempt"),
            "overlay-focused input must not submit as a user prompt"
        );
    }

    #[test]
    fn history_stable_during_overlay() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        let mut editor = InputEditor::new();

        editor.input_state.buffer = "first".to_string();
        let _ = editor.submit();
        editor.input_state.buffer = "second".to_string();
        let _ = editor.submit();
        editor.input_state.buffer = "draft".to_string();
        editor.input_state.cursor = editor.input_state.buffer.len();

        editor.history_up();
        let before_overlay_buffer = editor.input_state.buffer.clone();
        let before_overlay_index = editor.input_state.history_index;
        let before_overlay_history_len = editor.input_state.history.len();

        let (response_tx, _response_rx) = tokio::sync::oneshot::channel::<bool>();
        mode.overlay_state.pending_approval = Some(PendingApproval {
            tool_name: "read_file".to_string(),
            input_preview: "{}".to_string(),
            response_tx,
        });
        assert!(mode.overlay_active());

        let up =
            overlay_event_to_user_input(Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)));
        let down = overlay_event_to_user_input(Event::Key(KeyEvent::new(
            KeyCode::Down,
            KeyModifiers::NONE,
        )));
        assert!(
            up.is_none(),
            "overlay keymap must consume history navigation"
        );
        assert!(
            down.is_none(),
            "overlay keymap must consume history navigation"
        );

        assert_eq!(editor.input_state.buffer, before_overlay_buffer);
        assert_eq!(editor.input_state.history_index, before_overlay_index);
        assert_eq!(editor.input_state.history.len(), before_overlay_history_len);

        mode.on_user_input("1".to_string(), &mut ctx);
        assert!(!mode.overlay_active(), "overlay should clear on decision");

        editor.history_down();
        assert_eq!(editor.input_state.history_index, None);
        assert_eq!(
            editor.input_state.buffer, "draft",
            "prompt draft must restore after overlay transition"
        );
    }

    #[tokio::test]
    async fn diff_overlay_scrolls() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();

        let patch_preview = [
            "@@ -1,3 +1,4".to_string(),
            " context line".to_string(),
            "-old value".to_string(),
            "+new value".to_string(),
            " context tail".to_string(),
            "-removed again".to_string(),
            "+added again".to_string(),
        ]
        .join("\n");

        let (approve_tx, approve_rx) = tokio::sync::oneshot::channel::<bool>();
        mode.overlay_state.pending_patch_approval = Some(PendingPatchApproval {
            patch_preview: patch_preview.clone(),
            scroll_offset: 0,
            response_tx: Some(approve_tx),
        });

        mode.on_frontend_event(
            UserInputEvent::Scroll {
                target: ScrollTarget::Overlay,
                action: ScrollAction::LineDown,
            },
            &mut ctx,
        );
        assert_eq!(
            mode.overlay_state
                .pending_patch_approval
                .as_ref()
                .map(|p| p.scroll_offset),
            Some(1),
            "down must advance diff overlay scroll"
        );

        mode.on_frontend_event(
            UserInputEvent::Scroll {
                target: ScrollTarget::Overlay,
                action: ScrollAction::PageDown(3),
            },
            &mut ctx,
        );
        assert_eq!(
            mode.overlay_state
                .pending_patch_approval
                .as_ref()
                .map(|p| p.scroll_offset),
            Some(4),
            "page down must advance by requested step"
        );

        mode.on_frontend_event(
            UserInputEvent::Scroll {
                target: ScrollTarget::Overlay,
                action: ScrollAction::End,
            },
            &mut ctx,
        );
        assert_eq!(
            mode.overlay_state
                .pending_patch_approval
                .as_ref()
                .map(|p| p.scroll_offset),
            Some(patch_preview.lines().count().saturating_sub(1)),
            "end must jump to last diff line"
        );

        mode.on_user_input("1".to_string(), &mut ctx);
        assert!(
            approve_rx.await.expect("patch approval should resolve"),
            "approve binding must resolve true"
        );
        assert!(
            !mode.patch_overlay_active(),
            "overlay must clear after approve decision"
        );

        let (deny_tx, deny_rx) = tokio::sync::oneshot::channel::<bool>();
        mode.overlay_state.pending_patch_approval = Some(PendingPatchApproval {
            patch_preview,
            scroll_offset: 2,
            response_tx: Some(deny_tx),
        });
        mode.on_user_input("n".to_string(), &mut ctx);
        assert!(
            !deny_rx.await.expect("patch denial should resolve"),
            "deny binding must resolve false"
        );
        assert!(
            !mode.patch_overlay_active(),
            "overlay must clear after deny decision"
        );
    }

    #[test]
    fn input_pane_expands_then_clamps_to_max_rows() {
        assert_eq!(input_rows_for_buffer("", 80), 1);

        let multiline = (0..12)
            .map(|idx| format!("line-{idx}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            input_rows_for_buffer(&multiline, 80),
            MAX_INPUT_PANE_ROWS as u16
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
    fn test_editor_history_stash_restore() {
        let mut editor = InputEditor::new();

        editor.input_state.buffer = "first".to_string();
        let _ = editor.submit();
        editor.input_state.buffer = "second".to_string();
        let _ = editor.submit();

        editor.input_state.buffer = "draft".to_string();
        editor.input_state.cursor = editor.input_state.buffer.len();

        editor.history_up();
        assert_eq!(editor.input_state.buffer, "second");
        assert_eq!(editor.input_state.history_index, Some(1));

        editor.history_down();
        assert_eq!(editor.input_state.history_index, None);
        assert_eq!(editor.input_state.buffer, "draft");
        assert_eq!(editor.input_state.cursor, "draft".len());
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

        mode.on_user_input("__VEX_INTERRUPT__".to_string(), &mut ctx);
        assert!(
            mode.history_state.turn_in_progress,
            "plain text matching old sentinel must be treated as normal user input"
        );

        mode.on_interrupt(&mut ctx);
        assert!(
            mode.history_state.turn_in_progress,
            "typed interrupt should keep turn active until TurnComplete drains"
        );
        assert!(
            mode.history_state.cancel_pending,
            "typed interrupt should arm cancel-pending state"
        );
        assert!(
            mode.history_state
                .lines
                .iter()
                .any(|line| line.contains("[turn cancellation requested]")),
            "cancel path should provide visible feedback"
        );

        mode.on_model_update(UiUpdate::TurnComplete, &mut ctx);
        assert!(!mode.history_state.turn_in_progress);
        assert!(!mode.history_state.cancel_pending);
    }

    #[test]
    fn test_stream_delta_ignored_without_active_turn_slot() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        mode.on_model_update(UiUpdate::StreamDelta("ghost delta".to_string()), &mut ctx);
        assert!(
            mode.history_state.lines.is_empty(),
            "stale stream deltas must be ignored after turn completion/cancel"
        );
    }

    #[test]
    fn test_cancel_pending_blocks_stream_delta_appends() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        mode.on_user_input("hello".to_string(), &mut ctx);
        mode.on_interrupt(&mut ctx);
        mode.on_model_update(UiUpdate::StreamDelta("stale".to_string()), &mut ctx);
        assert_eq!(mode.history_state.lines[0], "> hello");
        assert_eq!(mode.history_state.lines[1], "");
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
