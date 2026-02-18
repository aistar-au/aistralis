use crate::config::Config;
use crate::edit_diff::{format_edit_hunks, DEFAULT_EDIT_DIFF_CONTEXT_LINES};
use crate::runtime::context::RuntimeContext;
use crate::runtime::mode::RuntimeMode;
use crate::runtime::parse_bool_flag;
use crate::runtime::UiUpdate;
use crate::state::{
    ConversationManager, ConversationStreamUpdate, StreamBlock, ToolApprovalRequest, ToolStatus,
};
use crate::tool_preview::{
    content_stats, format_read_file_snapshot_message, preview_tool_input, read_file_path,
    ReadFileSnapshotCache, ReadFileSummaryMessageStyle, ToolPreviewStyle,
};
use crate::ui::input_metrics::{
    clamp_to_char_boundary_left, cursor_row_col, display_width, truncate_to_display_width,
    wrap_input_lines,
};
use anyhow::Result;
use crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers,
};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, size as terminal_size};
use std::collections::{HashMap, HashSet};
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::task;

const DEFAULT_THINKING_WRAP_WIDTH: usize = 96;
const DEFAULT_PROMPT_AREA_WIDTH: usize = 80;
const DEFAULT_STICKY_INPUT_ROWS: usize = 1;
const THINKING_BLOB_MAX_LINES: usize = 4;
const CURSOR_BLINK_INTERVAL: Duration = Duration::from_millis(530);
const DEFAULT_FRAME_INTERVAL: Duration = Duration::from_millis(16);
const DOUBLE_INTERRUPT_EXIT_WINDOW: Duration = Duration::from_millis(900);
const TUI_TICK_INTERVAL: Duration = Duration::from_millis(120);
const REPO_WIDGET_REFRESH_INTERVAL: Duration = Duration::from_millis(1500);
const MULTILINE_PROMPT_START: &str = "/paste";
const MULTILINE_PROMPT_END: &str = "/send";

struct HistoryState {
    messages: Vec<String>,
    scroll: usize,
}

struct InputState {
    buffer: String,
    cursor_byte: usize,
}

enum OverlayKind {
    ToolPermission(ToolApprovalRequest),
    Error(String),
}

struct OverlayState {
    kind: OverlayKind,
}

pub struct TuiMode {
    history: HistoryState,
    input: InputState,
    overlay: Option<OverlayState>,
    turn_in_progress: bool,
}

impl TuiMode {
    pub fn new() -> Self {
        Self {
            history: HistoryState {
                messages: Vec::new(),
                scroll: 0,
            },
            input: InputState {
                buffer: String::new(),
                cursor_byte: 0,
            },
            overlay: None,
            turn_in_progress: false,
        }
    }
}

impl RuntimeMode for TuiMode {
    fn on_user_input(&mut self, input: String, ctx: &mut RuntimeContext) {
        if self.overlay.is_some() {
            return;
        }
        if self.turn_in_progress {
            return;
        }

        self.turn_in_progress = true;
        let _ = input;
        let _ = ctx;
    }

    fn on_model_update(&mut self, update: UiUpdate, _ctx: &mut RuntimeContext) {
        match update {
            UiUpdate::StreamDelta(text) => {
                if let Some(last) = self.history.messages.last_mut() {
                    last.push_str(&text);
                } else {
                    self.history.messages.push(text);
                }
            }
            UiUpdate::ToolApprovalRequest(req) => {
                self.overlay = Some(OverlayState {
                    kind: OverlayKind::ToolPermission(req),
                });
            }
            UiUpdate::TurnComplete => {
                self.turn_in_progress = false;
            }
            UiUpdate::Error(msg) => {
                self.history.messages.push(format!("[error] {msg}"));
                self.turn_in_progress = false;
            }
            _ => {}
        }
    }

    fn is_turn_in_progress(&self) -> bool {
        self.turn_in_progress
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolPromptDecision {
    AcceptOnce,
    AcceptSession,
    CancelNewTask,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolPromptSurface {
    Tui,
    Stream,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineStyle {
    Normal,
    Add,
    Delete,
    Event,
    Thinking,
    Tool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockKind {
    Normal,
    Thinking,
    Response,
    Tool,
    Event,
}

#[derive(Debug, Clone)]
struct EditorSnapshot {
    buffer: String,
    cursor: usize,
}

#[derive(Debug, Clone)]
struct LineEditorState {
    buffer: String,
    cursor: usize,
    undo_stack: Vec<EditorSnapshot>,
    redo_stack: Vec<EditorSnapshot>,
    history_index: Option<usize>,
    history_stash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum EditorAction {
    None,
    Changed,
    Submit(String),
    Cancel,
    Interrupt,
    Suspend,
    Eof,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StreamInputAction {
    Cancel,
    Interrupt,
    Submit(String),
    Eof,
}

#[derive(Debug, Clone, Copy)]
struct StickyPromptLayout {
    width: usize,
    input_width: usize,
    top_row: usize,
    prompt_row: usize,
    bottom_row: usize,
    output_bottom_row: usize,
    input_rows: usize,
}

struct StreamPrinter {
    current_line: String,
    pending_tokens: String,
    active_blocks: Vec<StreamBlock>,
    activity_blob_count: usize,
    streamed_any_delta: bool,
    active_style: LineStyle,
    active_block: BlockKind,
    colors_enabled: bool,
    in_code_block: bool,
    code_line_number: usize,
    thinking_wrap_width: usize,
    cursor_visible: bool,
    cursor_blink_phase: bool,
    last_cursor_toggle: Instant,
    last_frame_render: Instant,
    frame_interval: Duration,
    cursor_drawn: bool,
    progressive_effects_enabled: bool,
    cursor_enabled: bool,
    frame_batching_enabled: bool,
    structured_blocks_enabled: bool,
    turn_active: bool,
    read_snapshots: ReadFileSnapshotCache,
    saw_response_block: bool,
    sticky_footer_enabled: bool,
    sticky_input_rows: usize,
    tool_status_history: HashMap<String, Vec<ToolStatus>>,
}

impl StreamPrinter {
    fn new() -> Self {
        let frame_interval = resolve_frame_interval();
        Self {
            current_line: String::new(),
            pending_tokens: String::new(),
            active_blocks: Vec::new(),
            activity_blob_count: 0,
            streamed_any_delta: false,
            active_style: LineStyle::Normal,
            active_block: BlockKind::Normal,
            colors_enabled: detect_color_support(),
            in_code_block: false,
            code_line_number: 1,
            thinking_wrap_width: resolve_thinking_wrap_width(),
            cursor_visible: false,
            cursor_blink_phase: false,
            last_cursor_toggle: Instant::now(),
            last_frame_render: Instant::now(),
            frame_interval,
            cursor_drawn: false,
            progressive_effects_enabled: !disable_progressive_effects(),
            cursor_enabled: !disable_cursor(),
            frame_batching_enabled: !disable_frame_batching(),
            structured_blocks_enabled: use_structured_blocks(),
            turn_active: false,
            read_snapshots: ReadFileSnapshotCache::default(),
            saw_response_block: false,
            sticky_footer_enabled: false,
            sticky_input_rows: DEFAULT_STICKY_INPUT_ROWS,
            tool_status_history: HashMap::new(),
        }
    }

    fn begin_turn(&mut self) {
        self.streamed_any_delta = false;
        self.current_line.clear();
        self.pending_tokens.clear();
        self.active_blocks.clear();
        self.activity_blob_count = 0;
        self.active_block = BlockKind::Normal;
        self.in_code_block = false;
        self.code_line_number = 1;
        self.cursor_visible = false;
        self.cursor_blink_phase = false;
        self.last_cursor_toggle = Instant::now();
        self.last_frame_render = Instant::now();
        self.cursor_drawn = false;
        self.thinking_wrap_width = resolve_thinking_wrap_width();
        self.turn_active = true;
        self.saw_response_block = false;
        self.tool_status_history.clear();
    }

    fn has_streamed_delta(&self) -> bool {
        self.streamed_any_delta
    }

    fn frame_interval(&self) -> Duration {
        self.frame_interval
    }

    fn saw_response_block(&self) -> bool {
        self.saw_response_block
    }

    fn buffer_token(&mut self, token: &str) -> Result<()> {
        if token.is_empty() {
            return Ok(());
        }

        self.streamed_any_delta = true;
        self.cursor_visible = self.cursor_enabled;
        if self.frame_batching_enabled {
            self.pending_tokens.push_str(token);
            return Ok(());
        }

        self.write_chunk(token)?;
        self.last_frame_render = Instant::now();
        Ok(())
    }

    fn should_flush_frame(&self) -> bool {
        self.frame_batching_enabled
            && !self.pending_tokens.is_empty()
            && self.last_frame_render.elapsed() >= self.frame_interval
    }

    fn flush_buffered_tokens(&mut self) -> Result<()> {
        if self.pending_tokens.is_empty() {
            return Ok(());
        }

        let tokens = std::mem::take(&mut self.pending_tokens);
        self.write_chunk(&tokens)?;
        self.last_frame_render = Instant::now();
        Ok(())
    }

    fn on_frame_tick(&mut self) -> Result<()> {
        if self.should_flush_frame() {
            self.flush_buffered_tokens()?;
        }
        Ok(())
    }

    fn on_cursor_tick(&mut self) -> Result<()> {
        if !self.cursor_visible || !self.cursor_enabled {
            return Ok(());
        }
        if self.last_cursor_toggle.elapsed() >= CURSOR_BLINK_INTERVAL {
            self.cursor_blink_phase = !self.cursor_blink_phase;
            self.last_cursor_toggle = Instant::now();
        }
        self.render_cursor()
    }

    fn write_chunk(&mut self, chunk: &str) -> Result<()> {
        self.clear_inline_cursor()?;
        for ch in chunk.chars() {
            if ch == '\r' {
                continue;
            }
            if ch == '\n' {
                self.finish_current_line()?;
                continue;
            }

            self.current_line.push(ch);
            self.streamed_any_delta = true;
        }

        if self.cursor_visible && self.cursor_enabled {
            self.render_cursor()?;
        }
        io::stdout().flush()?;
        Ok(())
    }

    fn end_turn(&mut self) -> Result<()> {
        self.flush_buffered_tokens()?;
        self.cursor_visible = false;
        self.turn_active = false;
        self.clear_inline_cursor()?;

        if !self.current_line.is_empty() {
            self.finish_current_line()?;
        }

        self.set_style(LineStyle::Normal);
        self.current_line.clear();
        self.pending_tokens.clear();
        io::stdout().flush()?;
        Ok(())
    }

    fn print_code_line_prefix(&mut self) {
        self.set_style(LineStyle::Normal);
        print!(
            "{}",
            format_code_line_prefix(self.code_line_number, self.colors_enabled)
        );
        self.code_line_number += 1;
    }

    fn print_prompt(&mut self) -> Result<()> {
        self.sticky_input_rows = DEFAULT_STICKY_INPUT_ROWS;
        self.clear_inline_cursor()?;
        self.ensure_newline()?;
        self.set_style(LineStyle::Normal);
        self.print_dimmed_prompt_with_padding()
    }

    fn print_multiline_prompt(&mut self) -> Result<()> {
        self.sticky_input_rows = DEFAULT_STICKY_INPUT_ROWS;
        self.clear_inline_cursor()?;
        self.ensure_newline()?;
        self.set_style(LineStyle::Normal);
        self.print_dimmed_prompt_with_padding()
    }

    fn print_dimmed_prompt_with_padding(&mut self) -> Result<()> {
        if sticky_prompt_enabled() {
            let layout = sticky_prompt_layout(self.sticky_input_rows);
            let fill = " ".repeat(layout.width);
            let prompt_fill = " ".repeat(layout.width.saturating_sub(2));
            let (style_start, style_end) = self.prompt_surface_style();
            self.sticky_footer_enabled = true;

            print!(
                "\x1b[1;{output_bottom_row}r\
                 \x1b[{top_row};1H{style_start}{fill}{style_end}\
                 \x1b[{prompt_row};1H{style_start}> {prompt_fill}{style_end}\
                 \x1b[{bottom_row};1H{style_start}{fill}{style_end}",
                output_bottom_row = layout.output_bottom_row,
                top_row = layout.top_row,
                prompt_row = layout.prompt_row,
                bottom_row = layout.bottom_row
            );
            for row in (layout.prompt_row + 1)..layout.bottom_row {
                print!("\x1b[{row};1H{style_start}{fill}{style_end}");
            }
            print!("\x1b[{};3H", layout.prompt_row);
        } else {
            self.reset_scroll_region()?;
            self.sticky_footer_enabled = false;
            if self.colors_enabled {
                print!("\x1b[2;90m> \x1b[0m");
            } else {
                print!("> ");
            }
        }
        io::stdout().flush()?;
        Ok(())
    }

    fn prompt_surface_style(&self) -> (&'static str, &'static str) {
        if self.colors_enabled {
            ("\x1b[2;90;48;5;236m", "\x1b[0m")
        } else {
            ("", "")
        }
    }

    fn render_prompt_editor(
        &mut self,
        input: &str,
        cursor_byte: usize,
        return_to_output: bool,
    ) -> Result<()> {
        if self.sticky_footer_enabled && sticky_prompt_enabled() {
            let mut wrapped = wrap_input_lines(
                input,
                sticky_prompt_layout(self.sticky_input_rows).input_width,
            );
            let (cursor_row, cursor_col) = cursor_row_col(
                input,
                cursor_byte,
                sticky_prompt_layout(self.sticky_input_rows).input_width,
            );
            let required_rows = wrapped.len().max(cursor_row + 1).max(1);
            if self.sticky_input_rows != required_rows {
                self.sticky_input_rows = required_rows;
                self.print_dimmed_prompt_with_padding()?;
            }

            let layout = sticky_prompt_layout(self.sticky_input_rows);
            wrapped = wrap_input_lines(input, layout.input_width);
            while wrapped.len() < layout.input_rows {
                wrapped.push(String::new());
            }

            let (style_start, style_end) = self.prompt_surface_style();
            for (idx, line) in wrapped.iter().enumerate().take(layout.input_rows) {
                let row = layout.prompt_row + idx;
                let prefix = if idx == 0 { "> " } else { "  " };
                let content = truncate_to_display_width(line, layout.input_width);
                let content_width = display_width(&content);
                let pad = " ".repeat(layout.input_width.saturating_sub(content_width));
                print!("\x1b[{row};1H{style_start}{prefix}{content}{pad}{style_end}");
            }

            let cursor_row = (layout.prompt_row + cursor_row).min(layout.bottom_row);
            let cursor_col = (3 + cursor_col).min(layout.width + 1);
            print!("\x1b[{cursor_row};{cursor_col}H");

            if return_to_output {
                print!("\x1b[{};1H", layout.output_bottom_row);
            }
            io::stdout().flush()?;
            return Ok(());
        }

        self.reset_scroll_region()?;
        let content = input.replace('\n', " ");
        let safe_cursor = clamp_to_char_boundary_left(input, cursor_byte);
        let before_cursor = input[..safe_cursor].replace('\n', " ");
        let cursor_col = 3 + display_width(&before_cursor);
        if self.colors_enabled {
            print!("\r\x1b[2K\x1b[2;90m> \x1b[0m{content}\r\x1b[{cursor_col}C");
        } else {
            print!("\r\x1b[2K> {content}\r\x1b[{cursor_col}C");
        }
        io::stdout().flush()?;
        Ok(())
    }

    fn move_cursor_to_output_region(&mut self) -> Result<()> {
        if self.sticky_footer_enabled && sticky_prompt_enabled() {
            let layout = sticky_prompt_layout(self.sticky_input_rows);
            print!("\x1b[{};1H", layout.output_bottom_row);
            io::stdout().flush()?;
        }
        Ok(())
    }

    fn reset_scroll_region(&mut self) -> Result<()> {
        if self.sticky_footer_enabled {
            print!("\x1b[r");
            self.sticky_footer_enabled = false;
            io::stdout().flush()?;
        }
        Ok(())
    }

    fn print_multiline_continuation_prompt(&mut self) -> Result<()> {
        if self.sticky_footer_enabled && sticky_prompt_enabled() {
            return self.print_dimmed_prompt_with_padding();
        }

        self.set_style(LineStyle::Normal);
        if self.colors_enabled {
            print!("\x1b[2;90m> \x1b[0m");
        } else {
            print!("> ");
        }
        io::stdout().flush()?;
        Ok(())
    }

    fn print_error(&mut self, message: &str) -> Result<()> {
        self.flush_buffered_tokens()?;
        self.clear_inline_cursor()?;
        self.set_style(LineStyle::Normal);
        if self.colors_enabled {
            println!("\x1b[31m* Error: {message}\x1b[0m");
        } else {
            println!("* Error: {message}");
        }
        io::stdout().flush()?;
        Ok(())
    }

    fn ensure_newline(&mut self) -> Result<()> {
        self.flush_buffered_tokens()?;
        self.clear_inline_cursor()?;
        self.set_style(LineStyle::Normal);
        if !self.current_line.is_empty() {
            self.finish_current_line()?;
        }
        io::stdout().flush()?;
        Ok(())
    }

    fn print_activity_header(&mut self, style: LineStyle, header: &str) -> Result<()> {
        self.ensure_newline()?;
        if self.activity_blob_count > 0 {
            println!();
        }
        self.set_style(style);
        println!("{header}");
        self.activity_blob_count += 1;
        io::stdout().flush()?;
        Ok(())
    }

    fn print_tool_approval_prompt(&mut self, name: &str, input_preview: &str) -> Result<()> {
        // In structured mode, waiting-approval details are already rendered by ToolStatus.
        let needs_context =
            !(self.structured_blocks_enabled && self.active_block == BlockKind::Tool);
        if needs_context {
            let title = format!("* Tool: {name}");
            self.print_activity_header(LineStyle::Tool, title.as_str())?;
            self.render_structured_preview_lines(input_preview)?;
        }

        self.print_activity_header(LineStyle::Event, "* Prompt")?;
        println!("  │ 1 accept and continue");
        println!("  │ 2 accept and continue (session)");
        println!("  └ 3 cancel and start new task");

        self.set_style(LineStyle::Normal);
        if self.colors_enabled {
            print!("\x1b[1m* Select > \x1b[0m");
        } else {
            print!("* Select > ");
        }
        io::stdout().flush()?;
        Ok(())
    }

    fn render_structured_preview_lines(&mut self, preview: &str) -> Result<()> {
        for (idx, line) in preview.lines().enumerate() {
            let prefix = if idx == 0 {
                "  └ "
            } else if is_numbered_preview_line(line) {
                ""
            } else {
                "    "
            };
            let style = match line_style(line, false, self.colors_enabled) {
                LineStyle::Add => LineStyle::Add,
                LineStyle::Delete => LineStyle::Delete,
                _ => LineStyle::Event,
            };
            self.set_style(style);
            println!("{prefix}{line}");
        }
        Ok(())
    }

    fn print_session_auto_approve_notice(&mut self) -> Result<()> {
        self.print_activity_header(LineStyle::Event, "* Prompt")?;
        println!("  └ session auto-approve enabled");
        self.set_style(LineStyle::Normal);
        io::stdout().flush()?;
        Ok(())
    }

    fn on_block_start(&mut self, index: usize, block: StreamBlock) -> Result<()> {
        if !self.structured_blocks_enabled {
            return Ok(());
        }

        let is_update = matches!(
            (self.active_blocks.get(index), &block),
            (
                Some(StreamBlock::ToolCall { id: previous_id, .. }),
                StreamBlock::ToolCall { id: next_id, .. },
            ) if previous_id == next_id
        );
        if index == 0 && !is_update && !self.active_blocks.is_empty() {
            self.active_blocks.clear();
        }
        while self.active_blocks.len() < index {
            self.active_blocks.push(StreamBlock::Thinking {
                content: String::new(),
                collapsed: true,
            });
        }
        if index < self.active_blocks.len() {
            self.active_blocks[index] = block.clone();
        } else {
            self.active_blocks.push(block.clone());
        }

        self.render_structured_block(&block, is_update)
    }

    fn on_block_delta(&mut self, index: usize, delta: &str) -> Result<()> {
        if !self.structured_blocks_enabled {
            return Ok(());
        }

        let mut should_buffer = false;
        if let Some(block) = self.active_blocks.get_mut(index) {
            match block {
                StreamBlock::Thinking { content, .. } => {
                    content.push_str(delta);
                    should_buffer = true;
                }
                StreamBlock::FinalText { content } => {
                    content.push_str(delta);
                    should_buffer = true;
                }
                StreamBlock::ToolCall { .. } | StreamBlock::ToolResult { .. } => {}
            }
            if should_buffer {
                return self.buffer_token(delta);
            }
            return Ok(());
        }

        self.buffer_token(delta)
    }

    fn on_block_complete(&mut self, _index: usize) {}

    fn render_structured_block(&mut self, block: &StreamBlock, is_update: bool) -> Result<()> {
        match block {
            StreamBlock::Thinking { .. } => {
                if is_update {
                    return Ok(());
                }
                self.print_activity_header(LineStyle::Thinking, "* Thinking")?;
                self.active_block = BlockKind::Thinking;
            }
            StreamBlock::ToolCall {
                id,
                name,
                input,
                status,
                ..
            } => {
                if !is_update {
                    let title = format!("* Tool: {name}");
                    self.print_activity_header(LineStyle::Tool, title.as_str())?;
                }
                self.render_tool_status(id, name, input, status)?;
                self.active_block = BlockKind::Tool;
            }
            StreamBlock::ToolResult {
                tool_call_id,
                output,
                is_error,
            } => {
                let tool_label = self.resolve_tool_label(tool_call_id);
                if *is_error {
                    self.ensure_newline()?;
                    self.set_style(LineStyle::Delete);
                    let first_line = output.lines().next().unwrap_or(output);
                    println!("- [tool_error] {tool_label}: {first_line}");
                } else if !self.render_code_tool_result(tool_call_id, output)? {
                    self.ensure_newline()?;
                    self.set_style(LineStyle::Add);
                    println!("+ [tool_result] {tool_label}");
                } else {
                    self.set_style(LineStyle::Normal);
                }
                self.active_block = BlockKind::Normal;
            }
            StreamBlock::FinalText { content } => {
                if !is_update {
                    self.print_activity_header(LineStyle::Normal, "* Response")?;
                    self.active_block = BlockKind::Response;
                    self.saw_response_block = true;
                }
                if !is_update && !content.is_empty() {
                    self.buffer_token(content)?;
                }
            }
        }

        self.streamed_any_delta = true;
        io::stdout().flush()?;
        Ok(())
    }

    fn render_tool_status(
        &mut self,
        tool_call_id: &str,
        name: &str,
        input: &serde_json::Value,
        status: &ToolStatus,
    ) -> Result<()> {
        let mut status_history = self
            .tool_status_history
            .remove(tool_call_id)
            .unwrap_or_default();
        if status_history.last() != Some(status) {
            status_history.push(status.clone());
        }
        let status_flow = format_tool_status_flow(&status_history);
        self.tool_status_history
            .insert(tool_call_id.to_string(), status_history);

        self.set_style(LineStyle::Event);
        match status {
            ToolStatus::Pending => {}
            ToolStatus::WaitingApproval => {
                let preview = structured_tool_input_preview(name, input);
                self.render_structured_preview_lines(&preview)?;
            }
            ToolStatus::Executing => {}
            ToolStatus::Complete | ToolStatus::Cancelled => println!("  └ status: {status_flow}"),
        }
        Ok(())
    }

    fn resolve_tool_label(&self, tool_call_id: &str) -> String {
        self.active_blocks
            .iter()
            .find_map(|block| match block {
                StreamBlock::ToolCall { id, name, .. } if id == tool_call_id => Some(name.clone()),
                _ => None,
            })
            .unwrap_or_else(|| tool_call_id.to_string())
    }

    fn resolve_tool_call(&self, tool_call_id: &str) -> Option<(&str, &serde_json::Value)> {
        self.active_blocks.iter().find_map(|block| match block {
            StreamBlock::ToolCall {
                id, name, input, ..
            } if id == tool_call_id => Some((name.as_str(), input)),
            _ => None,
        })
    }

    fn render_code_tool_result(&mut self, tool_call_id: &str, output: &str) -> Result<bool> {
        let Some((tool_name, input)) = self.resolve_tool_call(tool_call_id) else {
            return Ok(false);
        };
        let tool_name = tool_name.to_string();
        let input = input.clone();

        match tool_name.as_str() {
            "edit_file" => {
                let path = input
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<missing>")
                    .to_string();
                let old_str = input.get("old_str").and_then(|v| v.as_str()).unwrap_or("");
                let new_str = input.get("new_str").and_then(|v| v.as_str()).unwrap_or("");

                let header = format!("* edited {path}");
                self.print_activity_header(LineStyle::Tool, &header)?;
                self.set_style(LineStyle::Event);
                println!(
                    "    change: {} chars/{} lines -> {} chars/{} lines",
                    old_str.chars().count(),
                    old_str
                        .lines()
                        .count()
                        .max(usize::from(!old_str.is_empty())),
                    new_str.chars().count(),
                    new_str
                        .lines()
                        .count()
                        .max(usize::from(!new_str.is_empty()))
                );
                io::stdout().flush()?;
                let hunks =
                    format_edit_hunks(old_str, new_str, "    ", DEFAULT_EDIT_DIFF_CONTEXT_LINES);
                self.render_diff_lines(&hunks)?;
                Ok(true)
            }
            "write_file" => {
                let path = input
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<missing>")
                    .to_string();
                let content = input.get("content").and_then(|v| v.as_str()).unwrap_or("");

                let header = format!("* wrote {path}");
                self.print_activity_header(LineStyle::Tool, &header)?;
                self.render_blob_metadata("content", content)?;
                self.render_blob_numbered_lines(content, Some('+'))?;
                Ok(true)
            }
            "read_file" => {
                let path = read_file_path(&input).unwrap_or_else(|| "<missing>".to_string());
                let header = format!("* read {path}");
                self.print_activity_header(LineStyle::Tool, &header)?;
                self.set_style(LineStyle::Event);
                let summary = self.read_snapshots.summarize(&path, output);
                println!(
                    "    {}",
                    format_read_file_snapshot_message(
                        &path,
                        summary,
                        ReadFileSummaryMessageStyle::StreamEvent
                    )
                );
                io::stdout().flush()?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    fn render_blob_metadata(&mut self, label: &str, content: &str) -> Result<()> {
        let (char_count, line_count) = content_stats(content);
        self.set_style(LineStyle::Event);
        println!("    {label}: {} chars, {} lines", char_count, line_count);
        io::stdout().flush()?;
        Ok(())
    }

    fn render_blob_numbered_lines(
        &mut self,
        content: &str,
        diff_marker: Option<char>,
    ) -> Result<()> {
        if content.is_empty() {
            let line = match diff_marker {
                Some(marker) => format!("    1 {marker} <empty>"),
                None => "    1   <empty>".to_string(),
            };
            let style = if diff_marker.is_some() {
                line_style(&line, false, self.colors_enabled)
            } else {
                LineStyle::Event
            };
            self.set_style(style);
            println!("{line}");
            io::stdout().flush()?;
            return Ok(());
        }

        for (idx, source_line) in content.lines().enumerate() {
            let line_number = idx + 1;
            let line = match diff_marker {
                Some(marker) => format!("    {line_number} {marker} {source_line}"),
                None => format!("    {line_number}   {source_line}"),
            };
            let style = if diff_marker.is_some() {
                line_style(&line, false, self.colors_enabled)
            } else {
                LineStyle::Event
            };
            self.set_style(style);
            println!("{line}");
        }
        io::stdout().flush()?;
        Ok(())
    }

    fn render_diff_lines(&mut self, rendered: &str) -> Result<()> {
        for line in rendered.lines() {
            let style = match line_style(line, false, self.colors_enabled) {
                LineStyle::Add => LineStyle::Add,
                LineStyle::Delete => LineStyle::Delete,
                _ => LineStyle::Event,
            };
            self.set_style(style);
            println!("{line}");
        }
        io::stdout().flush()?;
        Ok(())
    }

    fn render_cursor(&mut self) -> Result<()> {
        if !self.cursor_enabled || !self.cursor_visible {
            return Ok(());
        }
        if self.current_line.is_empty() {
            return Ok(());
        }

        if self.cursor_blink_phase && !self.cursor_drawn {
            print!("|");
            self.cursor_drawn = true;
        } else if !self.cursor_blink_phase && self.cursor_drawn {
            self.clear_inline_cursor()?;
        }
        io::stdout().flush()?;
        Ok(())
    }

    fn clear_inline_cursor(&mut self) -> Result<()> {
        if self.cursor_drawn {
            print!("\x08 \x08");
            self.cursor_drawn = false;
            io::stdout().flush()?;
        }
        Ok(())
    }

    fn render_line(&mut self, line: &str) -> Result<bool> {
        if self.in_code_block {
            if line.trim_start().starts_with("```") {
                self.set_style(LineStyle::Normal);
                print!("{line}");
                return Ok(true);
            }

            self.print_code_line_prefix();
            let style = line_style(line, true, self.colors_enabled);
            self.set_style(style);
            print!("{line}");
            return Ok(true);
        }

        let inline_thinking_text = thinking_inline_text(line);
        if self.active_block == BlockKind::Thinking
            && (!looks_like_activity_line(line) || inline_thinking_text.is_some())
            && !line.trim_start().starts_with("```")
        {
            let source_text = inline_thinking_text.unwrap_or_else(|| line.trim_start().to_string());
            let wrapped = wrap_text_for_display(&source_text, self.thinking_wrap_width);
            let out_lines = format_thinking_segments(&wrapped);

            if out_lines.is_empty() {
                return Ok(false);
            }

            self.set_style(LineStyle::Thinking);
            print!("{}", out_lines.join("\n"));
            return Ok(true);
        }

        let trimmed = line.trim_start();

        if let Some(activity_line) = format_server_activity_line(trimmed) {
            self.set_style(LineStyle::Event);
            print!("{activity_line}");
            return Ok(true);
        }

        let mut output =
            normalize_existing_numbered_snippet_line(line).unwrap_or_else(|| line.to_string());
        if output.is_empty() {
            return Ok(false);
        }
        let is_response_like = matches!(self.active_block, BlockKind::Response | BlockKind::Event);
        if is_response_like
            && !is_numbered_preview_line(&output)
            && !output.trim_start().starts_with("+ [tool_result]")
            && !output.trim_start().starts_with("- [tool_error]")
        {
            output = format_progressive_response_line(&output);
        }
        let style = line_style(&output, false, self.colors_enabled);
        self.set_style(style);
        print!("{output}");
        Ok(true)
    }

    fn set_style(&mut self, style: LineStyle) {
        if !self.colors_enabled || self.active_style == style {
            self.active_style = style;
            return;
        }

        if self.active_style != LineStyle::Normal {
            print!("\x1b[0m");
        }

        if !self.progressive_effects_enabled {
            match style {
                LineStyle::Add => print!("\x1b[1;32m"),
                LineStyle::Delete => print!("\x1b[1;31m"),
                LineStyle::Event => print!("\x1b[2m"),
                LineStyle::Thinking => print!("\x1b[2;90m"),
                LineStyle::Tool => print!("\x1b[33m"),
                LineStyle::Normal => {}
            }
            self.active_style = style;
            return;
        }

        match (style, self.turn_active) {
            (LineStyle::Thinking, true) => print!("\x1b[2;90m"),
            (LineStyle::Thinking, false) => print!("\x1b[90m"),
            (LineStyle::Tool, true) => print!("\x1b[1;33m"),
            (LineStyle::Tool, false) => print!("\x1b[2;33m"),
            (LineStyle::Normal, true) => print!("\x1b[2m"),
            (LineStyle::Normal, false) => {}
            (LineStyle::Add, _) => print!("\x1b[1;32m"),
            (LineStyle::Delete, _) => print!("\x1b[1;31m"),
            (LineStyle::Event, _) => print!("\x1b[2m"),
        }
        self.active_style = style;
    }

    fn update_code_block_state_for_finished_line(&mut self, line: &str) {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            self.in_code_block = !self.in_code_block;
            if self.in_code_block {
                self.code_line_number = 1;
            }
        }
    }

    fn update_block_context_for_finished_line(&mut self, line: &str) {
        let trimmed = line.trim_start();
        if trimmed.starts_with("* Thinking") {
            self.active_block = BlockKind::Thinking;
        } else if trimmed.starts_with("* Response") {
            self.active_block = BlockKind::Response;
        } else if self.active_block == BlockKind::Thinking && thinking_inline_text(line).is_some() {
            // Keep tool-call markers folded inside the active thinking block.
        } else if trimmed.starts_with("* Tool") {
            self.active_block = BlockKind::Tool;
        } else if trimmed.starts_with("* Event: message_stop") {
            self.active_block = BlockKind::Normal;
        } else if trimmed.starts_with("* Event:") {
            self.active_block = BlockKind::Event;
        }
    }

    fn finish_current_line(&mut self) -> Result<()> {
        self.clear_inline_cursor()?;
        let line = std::mem::take(&mut self.current_line);
        let rendered = self.render_line(&line)?;
        if rendered {
            println!();
        }
        self.update_code_block_state_for_finished_line(&line);
        self.update_block_context_for_finished_line(&line);
        Ok(())
    }
}

pub struct App {
    update_rx: mpsc::UnboundedReceiver<UiUpdate>,
    message_tx: mpsc::UnboundedSender<String>,
    should_quit: bool,
    auto_approve_tools: bool,
    suppress_until_turn_complete: bool,
    stream_printer: StreamPrinter,
    input_history: Vec<String>,
    pending_command: Option<String>,
    last_interrupt_at: Option<Instant>,
    terminal: Option<crate::terminal::TerminalType>,
    input_buffer: String,
    cursor_position: usize,
    input_editor: LineEditorState,
    messages: Vec<String>,
    scroll_offset: usize,
    pending_tool_approval: Option<PendingToolApprovalState>,
    active_assistant_message: Option<usize>,
    turn_in_progress: bool,
    saw_structured_blocks_this_turn: bool,
    thinking_buffers: HashMap<usize, String>,
    final_text_block_indices: HashSet<usize>,
    thinking_block_message_indices: HashMap<usize, usize>,
    tool_names: HashMap<String, String>,
    tool_message_indices: HashMap<String, usize>,
    tool_status_history: HashMap<String, Vec<ToolStatus>>,
    repo_widget: Option<RepoWidgetState>,
    repo_widget_last_refresh: Instant,
}

struct PendingToolApprovalState {
    tool_name: String,
    input_preview: String,
    response_tx: Option<oneshot::Sender<bool>>,
}

#[derive(Debug, Clone)]
struct RepoWidgetState {
    name: String,
    branch: String,
    dirty: bool,
    changed_entries: usize,
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
                let (delta_tx, stream_forwarder) = {
                    let update_tx = update_tx.clone();
                    let (delta_tx, mut delta_rx) =
                        mpsc::unbounded_channel::<ConversationStreamUpdate>();
                    let handle = task::spawn(async move {
                        while let Some(delta) = delta_rx.recv().await {
                            let ui_update = match delta {
                                ConversationStreamUpdate::Delta(text) => {
                                    UiUpdate::StreamDelta(text)
                                }
                                ConversationStreamUpdate::BlockStart { index, block } => {
                                    UiUpdate::StreamBlockStart { index, block }
                                }
                                ConversationStreamUpdate::BlockDelta { index, delta } => {
                                    UiUpdate::StreamBlockDelta { index, delta }
                                }
                                ConversationStreamUpdate::BlockComplete { index } => {
                                    UiUpdate::StreamBlockComplete { index }
                                }
                                ConversationStreamUpdate::ToolApprovalRequest(request) => {
                                    UiUpdate::ToolApprovalRequest(request)
                                }
                            };
                            let _ = update_tx.send(ui_update);
                        }
                    });
                    (delta_tx, handle)
                };

                let response = mgr.send_message(content, Some(&delta_tx)).await;
                drop(mgr);
                drop(delta_tx);
                if let Err(join_error) = stream_forwarder.await {
                    let _ = update_tx.send(UiUpdate::Error(format!(
                        "Stream forwarding failed: {join_error}"
                    )));
                    continue;
                }

                match response {
                    Ok(response) => {
                        let _ = response;
                        let _ = update_tx.send(UiUpdate::TurnComplete);
                    }
                    Err(e) => {
                        let _ = update_tx.send(UiUpdate::Error(e.to_string()));
                    }
                }
            }
        });

        let terminal = if io::stdin().is_terminal() && io::stdout().is_terminal() {
            Some(crate::terminal::setup()?)
        } else {
            None
        };
        let repo_widget = detect_repo_widget();

        Ok(Self {
            update_rx,
            message_tx,
            should_quit: false,
            auto_approve_tools: false,
            suppress_until_turn_complete: false,
            stream_printer: StreamPrinter::new(),
            input_history: Vec::new(),
            pending_command: None,
            last_interrupt_at: None,
            terminal,
            input_buffer: String::new(),
            cursor_position: 0,
            input_editor: LineEditorState::with_initial(""),
            messages: Vec::new(),
            scroll_offset: 0,
            pending_tool_approval: None,
            active_assistant_message: None,
            turn_in_progress: false,
            saw_structured_blocks_this_turn: false,
            thinking_buffers: HashMap::new(),
            final_text_block_indices: HashSet::new(),
            thinking_block_message_indices: HashMap::new(),
            tool_names: HashMap::new(),
            tool_message_indices: HashMap::new(),
            tool_status_history: HashMap::new(),
            repo_widget,
            repo_widget_last_refresh: Instant::now(),
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        if self.terminal.is_some() {
            return self.run_tui().await;
        }

        while !self.should_quit {
            let content = if let Some(queued) = self.pending_command.take() {
                queued
            } else {
                self.stream_printer.print_prompt()?;
                let Some(input) = self.read_user_input()? else {
                    break;
                };
                input
            };
            let command = content.trim();
            if command.is_empty() {
                continue;
            }
            if is_escape_command(command) {
                continue;
            }
            if matches!(command, "q" | "quit" | "exit" | "/q" | "/quit" | "/exit") {
                self.should_quit = true;
                break;
            }
            if self.handle_local_command(command)? {
                self.remember_history(command);
                continue;
            }
            self.remember_history(command);

            self.drain_stale_updates();
            self.stream_printer.begin_turn();
            self.stream_printer.move_cursor_to_output_region()?;
            let _ = self.message_tx.send(content);
            let mut frame_ticker = tokio::time::interval(self.stream_printer.frame_interval());
            let mut cursor_ticker = tokio::time::interval(CURSOR_BLINK_INTERVAL);
            let mut queued_from_stream: Option<String> = None;
            let mut stream_editor = LineEditorState::with_initial("");
            let mut stream_input_mode = sticky_prompt_enabled();
            if stream_input_mode {
                enable_input_raw_mode()?;
                self.stream_printer.render_prompt_editor(
                    &stream_editor.buffer,
                    stream_editor.cursor_byte(),
                    true,
                )?;
            }

            loop {
                tokio::select! {
                    _ = frame_ticker.tick() => {
                        self.stream_printer.on_frame_tick()?;
                        if stream_input_mode {
                            if let Some(action) = self.poll_stream_input(&mut stream_editor)? {
                                match action {
                                    StreamInputAction::Cancel => {
                                        self.cancel_active_turn()?;
                                    }
                                    StreamInputAction::Interrupt => {
                                        if self.handle_interrupt()? {
                                            break;
                                        }
                                    }
                                    StreamInputAction::Submit(command) => {
                                        queued_from_stream = Some(command);
                                        self.cancel_active_turn()?;
                                    }
                                    StreamInputAction::Eof => {
                                        self.should_quit = true;
                                        self.cancel_active_turn()?;
                                    }
                                }
                            }
                        }
                    }
                    _ = cursor_ticker.tick() => {
                        self.stream_printer.on_cursor_tick()?;
                    }
                    _ = tokio::signal::ctrl_c() => {
                        if self.handle_interrupt()? {
                            break;
                        }
                    }
                    update = self.update_rx.recv() => {
                        match update {
                            Some(UiUpdate::StreamDelta(text)) => {
                                if self.suppress_until_turn_complete {
                                    continue;
                                }
                                self.stream_printer.buffer_token(&text)?;
                            }
                            Some(UiUpdate::StreamBlockStart { index, block }) => {
                                if self.suppress_until_turn_complete {
                                    continue;
                                }
                                self.stream_printer.on_block_start(index, block)?;
                            }
                            Some(UiUpdate::StreamBlockDelta { index, delta }) => {
                                if self.suppress_until_turn_complete {
                                    continue;
                                }
                                self.stream_printer.on_block_delta(index, &delta)?;
                            }
                            Some(UiUpdate::StreamBlockComplete { index }) => {
                                if self.suppress_until_turn_complete {
                                    continue;
                                }
                                self.stream_printer.on_block_complete(index);
                            }
                            Some(UiUpdate::ToolApprovalRequest(request)) => {
                                let mut response_tx = Some(request.response_tx);
                                if self.suppress_until_turn_complete {
                                    respond_tool_approval(&mut response_tx, false);
                                    continue;
                                }
                                if self.auto_approve_tools {
                                    respond_tool_approval(&mut response_tx, true);
                                    continue;
                                }

                                let mut approval_decision = ToolPromptDecision::CancelNewTask;
                                if stream_input_mode {
                                    if let Err(err) = disable_input_raw_mode() {
                                        self.suppress_until_turn_complete = true;
                                        respond_tool_approval(&mut response_tx, false);
                                        self.stream_printer.set_style(LineStyle::Normal);
                                        self.stream_printer.print_error(&format!(
                                            "Tool approval prompt setup failed: {err}"
                                        ))?;
                                        continue;
                                    }
                                    stream_input_mode = false;
                                }

                                if let Err(err) = self
                                    .stream_printer
                                    .flush_buffered_tokens()
                                    .and_then(|_| {
                                        self.stream_printer.print_tool_approval_prompt(
                                            &request.tool_name,
                                            &request.input_preview,
                                        )
                                    })
                                {
                                    self.suppress_until_turn_complete = true;
                                    respond_tool_approval(&mut response_tx, false);
                                    self.stream_printer.set_style(LineStyle::Normal);
                                    self.stream_printer.print_error(&format!(
                                        "Tool approval prompt failed: {err}"
                                    ))?;
                                } else {
                                    match read_tool_confirmation().await {
                                        Ok(decision) => approval_decision = decision,
                                        Err(err) => {
                                            self.suppress_until_turn_complete = true;
                                            respond_tool_approval(&mut response_tx, false);
                                            approval_decision = ToolPromptDecision::CancelNewTask;
                                            self.stream_printer.set_style(LineStyle::Normal);
                                            if let Err(print_err) = self.stream_printer.print_error(
                                                &format!("Tool approval input failed: {err}"),
                                            ) {
                                                eprintln!(
                                                    "tool approval error display failed: {print_err}"
                                                );
                                            }
                                        }
                                    }
                                }

                                if let Err(err) = self.apply_tool_prompt_decision(
                                    approval_decision,
                                    &mut response_tx,
                                    ToolPromptSurface::Stream,
                                ) {
                                    self.stream_printer.set_style(LineStyle::Normal);
                                    self.stream_printer
                                        .print_error(&format!("Tool approval notice failed: {err}"))?;
                                }
                                self.stream_printer.set_style(LineStyle::Normal);
                                if sticky_prompt_enabled() {
                                    match enable_input_raw_mode() {
                                        Ok(()) => {
                                            stream_input_mode = true;
                                            if let Err(err) = self.stream_printer.render_prompt_editor(
                                                &stream_editor.buffer,
                                                stream_editor.cursor_byte(),
                                                true,
                                            ) {
                                                stream_input_mode = false;
                                                let _ = disable_input_raw_mode();
                                                self.stream_printer.set_style(LineStyle::Normal);
                                                self.stream_printer.print_error(&format!(
                                                    "Prompt restore failed: {err}"
                                                ))?;
                                            }
                                        }
                                        Err(err) => {
                                            stream_input_mode = false;
                                            self.stream_printer.set_style(LineStyle::Normal);
                                            self.stream_printer.print_error(&format!(
                                                "Raw mode restore failed: {err}"
                                            ))?;
                                        }
                                    }
                                }
                            }
                            Some(UiUpdate::TurnComplete) => {
                                if self.suppress_until_turn_complete {
                                    self.suppress_until_turn_complete = false;
                                    if self.stream_printer.turn_active {
                                        self.stream_printer.end_turn()?;
                                    }
                                    break;
                                }
                                self.stream_printer.end_turn()?;
                                break;
                            }
                            Some(UiUpdate::Error(err)) => {
                                if self.suppress_until_turn_complete {
                                    self.suppress_until_turn_complete = false;
                                    if self.stream_printer.turn_active {
                                        self.stream_printer.end_turn()?;
                                    }
                                    break;
                                }
                                self.stream_printer.end_turn()?;
                                self.stream_printer.print_error(&err)?;
                                break;
                            }
                            None => {
                                self.should_quit = true;
                                break;
                            }
                        }
                    }
                }
            }

            if stream_input_mode {
                let _ = disable_input_raw_mode();
            }
            if let Some(command) = queued_from_stream {
                self.pending_command = Some(command);
            }
        }

        self.stream_printer.reset_scroll_region()?;
        Ok(())
    }

    async fn run_tui(&mut self) -> Result<()> {
        self.sync_input_surface_state();
        self.messages.clear();
        self.append_message("* Ready");
        self.append_message("  └ type /commands for shortcuts");
        self.append_message(String::new());

        let mut tick = tokio::time::interval(TUI_TICK_INTERVAL);
        while !self.should_quit {
            self.draw_tui_frame()?;
            self.process_tui_events()?;
            self.drain_tui_updates_nonblocking()?;

            tokio::select! {
                _ = tick.tick() => {}
                _ = tokio::signal::ctrl_c() => {
                    if self.handle_interrupt()? {
                        break;
                    }
                }
                update = self.update_rx.recv() => {
                    self.handle_tui_update(update)?;
                }
            }
        }

        Ok(())
    }

    fn draw_tui_frame(&mut self) -> Result<()> {
        self.refresh_repo_widget_if_stale(false);
        let status_line = self.status_line_text();
        let Some(terminal) = self.terminal.as_mut() else {
            return Ok(());
        };

        let input = self.input_buffer.clone();
        let cursor_position = self.cursor_position;
        let messages = self.messages.clone();
        let scroll_offset = self.scroll_offset;
        let approval_modal = self
            .pending_tool_approval
            .as_ref()
            .map(|approval| (approval.tool_name.clone(), approval.input_preview.clone()));
        let auto_approve_tools = self.auto_approve_tools;

        terminal.draw(|frame| {
            let size = frame.area();
            let input_width = size.width.saturating_sub(2).max(1) as usize;
            let input_rows = crate::ui::render::input_visual_rows(&input, input_width);
            let max_input_height = size.height.saturating_sub(4).max(3);
            let input_height = (input_rows as u16).clamp(1, max_input_height);
            let layout = ratatui::layout::Layout::default()
                .direction(ratatui::layout::Direction::Vertical)
                .constraints([
                    ratatui::layout::Constraint::Min(1),
                    ratatui::layout::Constraint::Length(1),
                    ratatui::layout::Constraint::Length(input_height),
                ])
                .split(size);

            crate::ui::render::render_messages(frame, layout[0], &messages, scroll_offset);
            crate::ui::render::render_status_line(frame, layout[1], &status_line);
            crate::ui::render::render_input(frame, layout[2], &input, cursor_position);

            if let Some((tool_name, input_preview)) = &approval_modal {
                crate::ui::render::render_tool_approval_modal(
                    frame,
                    tool_name,
                    input_preview,
                    auto_approve_tools,
                );
            }
        })?;

        Ok(())
    }

    fn process_tui_events(&mut self) -> Result<()> {
        while event::poll(Duration::from_millis(0))? {
            match event::read()? {
                Event::Paste(text) => {
                    if self.pending_tool_approval.is_none() && !text.is_empty() {
                        self.input_editor.insert_str(&text);
                        self.sync_input_surface_state();
                    }
                }
                Event::Key(key)
                    if key.kind == KeyEventKind::Press || key.kind == KeyEventKind::Repeat =>
                {
                    self.handle_tui_key_event(key)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn handle_tui_key_event(&mut self, key: KeyEvent) -> Result<()> {
        if self.pending_tool_approval.is_some() {
            let decision = tool_prompt_decision_from_key_code(key.code);
            if let Some(mut approval) = self.pending_tool_approval.take() {
                if let Some(decision) = decision {
                    self.apply_tool_prompt_decision(
                        decision,
                        &mut approval.response_tx,
                        ToolPromptSurface::Tui,
                    )?;
                } else {
                    self.pending_tool_approval = Some(approval);
                }
            }
            return Ok(());
        }

        match key.code {
            KeyCode::PageUp => {
                self.scroll_offset = self.scroll_offset.saturating_sub(3);
                return Ok(());
            }
            KeyCode::PageDown => {
                self.scroll_offset = self.scroll_offset.saturating_add(3);
                return Ok(());
            }
            _ => {}
        }

        match apply_editor_key_event(&mut self.input_editor, key, Some(&self.input_history)) {
            EditorAction::None => {}
            EditorAction::Changed => {
                self.clear_interrupt_window();
                self.sync_input_surface_state();
            }
            EditorAction::Submit(text) => self.submit_tui_input(text)?,
            EditorAction::Cancel => {
                if self.turn_in_progress {
                    self.cancel_active_turn()?;
                } else if !self.input_editor.buffer.is_empty() {
                    self.input_editor = LineEditorState::with_initial("");
                    self.sync_input_surface_state();
                } else {
                    self.should_quit = true;
                }
            }
            EditorAction::Interrupt => {
                if self.handle_interrupt()? {
                    self.should_quit = true;
                }
            }
            EditorAction::Suspend => {
                self.suspend_to_shell()?;
                self.sync_input_surface_state();
            }
            EditorAction::Eof => {
                self.should_quit = true;
            }
        }

        Ok(())
    }

    fn submit_tui_input(&mut self, content: String) -> Result<()> {
        let trimmed = content.trim().to_string();
        self.input_editor = LineEditorState::with_initial("");
        self.sync_input_surface_state();
        if trimmed.is_empty() {
            return Ok(());
        }

        if is_escape_command(&trimmed) {
            return Ok(());
        }
        if matches!(
            trimmed.as_str(),
            "q" | "quit" | "exit" | "/q" | "/quit" | "/exit"
        ) {
            self.should_quit = true;
            return Ok(());
        }
        if self.handle_local_command_tui(&trimmed)? {
            self.remember_history(&trimmed);
            return Ok(());
        }
        self.remember_history(&trimmed);

        self.start_new_tui_turn(&trimmed, content);
        Ok(())
    }

    fn start_new_tui_turn(&mut self, display_command: &str, content: String) {
        self.drain_stale_updates();
        self.suppress_until_turn_complete = false;
        self.turn_in_progress = true;
        self.saw_structured_blocks_this_turn = false;
        self.pending_tool_approval = None;
        self.active_assistant_message = None;
        self.thinking_buffers.clear();
        self.final_text_block_indices.clear();
        self.thinking_block_message_indices.clear();
        self.tool_names.clear();
        self.tool_message_indices.clear();
        self.tool_status_history.clear();

        self.append_message(format!("> {display_command}"));
        let placeholder_idx = self.messages.len();
        self.append_message(String::new());
        self.active_assistant_message = Some(placeholder_idx);

        let _ = self.message_tx.send(content);
    }

    fn handle_tui_update(&mut self, update: Option<UiUpdate>) -> Result<()> {
        match update {
            Some(UiUpdate::StreamDelta(text)) => {
                if self.suppress_until_turn_complete || self.saw_structured_blocks_this_turn {
                    return Ok(());
                }
                let idx = self.ensure_assistant_message();
                self.messages[idx].push_str(&text);
            }
            Some(UiUpdate::StreamBlockStart { index, block }) => {
                if self.suppress_until_turn_complete {
                    return Ok(());
                }
                self.saw_structured_blocks_this_turn = true;
                self.handle_stream_block_start(index, block);
            }
            Some(UiUpdate::StreamBlockDelta { index, delta }) => {
                if self.suppress_until_turn_complete {
                    return Ok(());
                }
                self.handle_stream_block_delta(index, &delta);
            }
            Some(UiUpdate::StreamBlockComplete { .. }) => {}
            Some(UiUpdate::ToolApprovalRequest(request)) => {
                let mut response_tx = Some(request.response_tx);
                if self.suppress_until_turn_complete {
                    respond_tool_approval(&mut response_tx, false);
                    return Ok(());
                }
                if self.auto_approve_tools {
                    respond_tool_approval(&mut response_tx, true);
                    return Ok(());
                }
                self.pending_tool_approval = Some(PendingToolApprovalState {
                    tool_name: request.tool_name,
                    input_preview: request.input_preview,
                    response_tx,
                });
            }
            Some(UiUpdate::TurnComplete) => {
                self.pending_tool_approval = None;
                if self.suppress_until_turn_complete {
                    self.suppress_until_turn_complete = false;
                    self.finish_tui_turn();
                    return Ok(());
                }
                self.finish_tui_turn();
            }
            Some(UiUpdate::Error(err)) => {
                self.pending_tool_approval = None;
                if self.suppress_until_turn_complete {
                    self.suppress_until_turn_complete = false;
                    self.finish_tui_turn();
                    return Ok(());
                }
                self.append_message(format!("* Error: {err}"));
                self.finish_tui_turn();
            }
            None => {
                self.should_quit = true;
            }
        }
        Ok(())
    }

    fn finish_tui_turn(&mut self) {
        self.turn_in_progress = false;
        self.active_assistant_message = None;
        self.saw_structured_blocks_this_turn = false;
        self.thinking_buffers.clear();
        self.final_text_block_indices.clear();
        self.thinking_block_message_indices.clear();
        self.tool_names.clear();
        self.tool_message_indices.clear();
        self.tool_status_history.clear();
    }

    fn handle_stream_block_start(&mut self, index: usize, block: StreamBlock) {
        match block {
            StreamBlock::Thinking { content, .. } => {
                self.thinking_buffers.insert(index, content);
                self.update_thinking_message(index);
            }
            StreamBlock::FinalText { content } => {
                self.final_text_block_indices.insert(index);
                let idx = self.ensure_assistant_message();
                if !content.is_empty() {
                    self.messages[idx].push_str(&content);
                }
            }
            StreamBlock::ToolCall {
                id,
                name,
                input,
                status,
            } => {
                self.update_tool_call_message(&id, &name, &input, &status);
            }
            StreamBlock::ToolResult {
                tool_call_id,
                output,
                is_error,
            } => {
                self.append_tool_result_message(&tool_call_id, &output, is_error);
            }
        }
    }

    fn handle_stream_block_delta(&mut self, index: usize, delta: &str) {
        if let Some(content) = self.thinking_buffers.get_mut(&index) {
            content.push_str(delta);
            self.update_thinking_message(index);
            return;
        }

        if self.final_text_block_indices.contains(&index) {
            let idx = self.ensure_assistant_message();
            self.messages[idx].push_str(delta);
        }
    }

    fn update_thinking_message(&mut self, index: usize) {
        let Some(content) = self.thinking_buffers.get(&index) else {
            return;
        };
        let wrapped = wrap_text_for_display(content, resolve_thinking_wrap_width());
        let lines = format_thinking_segments(&wrapped);
        if lines.is_empty() {
            return;
        }

        let message_index = self
            .thinking_block_message_indices
            .get(&index)
            .copied()
            .unwrap_or_else(|| {
                let idx = self.messages.len();
                self.append_message(String::new());
                self.thinking_block_message_indices.insert(index, idx);
                idx
            });
        self.messages[message_index] = lines.join("\n");
    }

    fn update_tool_call_message(
        &mut self,
        id: &str,
        name: &str,
        input: &serde_json::Value,
        status: &ToolStatus,
    ) {
        self.tool_names.insert(id.to_string(), name.to_string());
        let history = self.tool_status_history.entry(id.to_string()).or_default();
        if history.last() != Some(status) {
            history.push(status.clone());
        }
        let status_flow = format_tool_status_flow(history);

        let mut block = format!("tool: {name}\nstatus: {status_flow}");
        if matches!(status, ToolStatus::WaitingApproval) {
            let preview = structured_tool_input_preview(name, input);
            let preview = preview.lines().take(2).collect::<Vec<_>>().join("\n");
            if !preview.is_empty() {
                block.push('\n');
                block.push_str(&preview);
            }
        }

        let idx = self
            .tool_message_indices
            .get(id)
            .copied()
            .unwrap_or_else(|| {
                let idx = self.messages.len();
                self.append_message(String::new());
                self.tool_message_indices.insert(id.to_string(), idx);
                idx
            });
        self.messages[idx] = block;
    }

    fn append_tool_result_message(&mut self, tool_call_id: &str, output: &str, is_error: bool) {
        let tool_label = self
            .tool_names
            .get(tool_call_id)
            .cloned()
            .unwrap_or_else(|| tool_call_id.to_string());
        let mut lines = vec![if is_error {
            format!("tool error: {tool_label}")
        } else {
            format!("tool result: {tool_label}")
        }];
        for line in output.lines().take(4) {
            lines.push(line.to_string());
        }
        if output.lines().count() > 4 {
            lines.push("...".to_string());
        }
        self.append_message(lines.join("\n"));
    }

    fn ensure_assistant_message(&mut self) -> usize {
        if let Some(idx) = self.active_assistant_message {
            if idx < self.messages.len() {
                return idx;
            }
        }

        let idx = self.messages.len();
        self.append_message(String::new());
        self.active_assistant_message = Some(idx);
        idx
    }

    fn append_message<S: Into<String>>(&mut self, message: S) {
        self.messages.push(message.into());
    }

    fn refresh_repo_widget_if_stale(&mut self, force: bool) {
        if !force && self.repo_widget_last_refresh.elapsed() < REPO_WIDGET_REFRESH_INTERVAL {
            return;
        }
        self.repo_widget = detect_repo_widget();
        self.repo_widget_last_refresh = Instant::now();
    }

    fn status_line_text(&self) -> String {
        let mode = if self.pending_tool_approval.is_some() {
            "approval"
        } else if self.turn_in_progress && self.suppress_until_turn_complete {
            "cancelling"
        } else if self.turn_in_progress {
            "streaming"
        } else {
            "idle"
        };
        let approvals = if self.auto_approve_tools {
            "approve:auto"
        } else {
            "approve:manual"
        };
        let input_mode = if self.input_buffer.contains('\n') {
            "input:multiline"
        } else {
            "input:single"
        };
        let history = format!("history:{}", self.input_history.len());
        let repo = if let Some(repo) = &self.repo_widget {
            if repo.dirty {
                format!(
                    "repo:{}/{}*{}",
                    repo.name, repo.branch, repo.changed_entries
                )
            } else {
                format!("repo:{}/{} clean", repo.name, repo.branch)
            }
        } else {
            "repo:none".to_string()
        };
        format!("{mode} | {approvals} | {input_mode} | {history} | {repo}")
    }

    fn sync_input_surface_state(&mut self) {
        self.input_buffer = self.input_editor.buffer.clone();
        self.cursor_position = self.input_editor.cursor_byte();
    }

    fn drain_tui_updates_nonblocking(&mut self) -> Result<()> {
        loop {
            match self.update_rx.try_recv() {
                Ok(update) => self.handle_tui_update(Some(update))?,
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                    self.should_quit = true;
                    break;
                }
            }
        }
        Ok(())
    }

    fn handle_local_command_tui(&mut self, command: &str) -> Result<bool> {
        match command {
            "/commands" | "/help" => {
                self.append_message("* Commands");
                self.append_message("  └ /commands, /clear, /history, /repo, /ps, /quit");
                self.append_message("    multiline: Shift+Enter or Ctrl+J inserts newline");
                Ok(true)
            }
            "/clear" => {
                self.messages.clear();
                self.append_message("* Cleared");
                Ok(true)
            }
            "/history" => {
                self.append_message("* History");
                if self.input_history.is_empty() {
                    self.append_message("  └ no commands yet");
                } else {
                    let start = self.input_history.len().saturating_sub(20);
                    let history_slice: Vec<(usize, String)> = self
                        .input_history
                        .iter()
                        .enumerate()
                        .skip(start)
                        .map(|(idx, item)| (idx + 1, item.clone()))
                        .collect();
                    for (idx, item) in history_slice {
                        self.append_message(format!("  {:>3}. {}", idx, item));
                    }
                }
                Ok(true)
            }
            "/repo" => {
                self.refresh_repo_widget_if_stale(true);
                self.append_message("* Repo");
                if let Some(repo) = &self.repo_widget {
                    let dirty = if repo.dirty {
                        format!("dirty ({} changes)", repo.changed_entries)
                    } else {
                        "clean".to_string()
                    };
                    self.append_message(format!("  └ {}/{} {dirty}", repo.name, repo.branch));
                } else {
                    self.append_message("  └ not in a git repository");
                }
                Ok(true)
            }
            "/ps" => {
                self.append_message("* Shell: ps");
                match Command::new("ps")
                    .args(["-ax", "-o", "pid=,ppid=,stat=,etime=,command="])
                    .output()
                {
                    Ok(output) if output.status.success() => {
                        let text = String::from_utf8_lossy(&output.stdout);
                        let mut lines: Vec<String> =
                            text.lines().take(40).map(|line| line.to_string()).collect();
                        if text.lines().count() > 40 {
                            lines.push("... (truncated)".to_string());
                        }
                        self.append_message(lines.join("\n"));
                    }
                    Ok(output) => {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        let stderr = stderr.trim();
                        if stderr.is_empty() {
                            self.append_message(format!(
                                "failed to run ps (exit: {})",
                                output.status
                            ));
                        } else {
                            self.append_message(stderr.to_string());
                        }
                    }
                    Err(err) => {
                        self.append_message(format!("failed to run ps: {err}"));
                    }
                }
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    fn drain_stale_updates(&mut self) {
        while self.update_rx.try_recv().is_ok() {}
    }

    fn should_exit_on_interrupt(&mut self) -> bool {
        let now = Instant::now();
        let should_exit = self
            .last_interrupt_at
            .is_some_and(|last| now.duration_since(last) <= DOUBLE_INTERRUPT_EXIT_WINDOW);
        self.last_interrupt_at = Some(now);
        should_exit
    }

    fn clear_interrupt_window(&mut self) {
        self.last_interrupt_at = None;
    }

    fn clear_prompt_input(&mut self) {
        self.input_editor = LineEditorState::with_initial("");
        self.sync_input_surface_state();
    }

    fn apply_tool_prompt_decision(
        &mut self,
        decision: ToolPromptDecision,
        response_tx: &mut Option<oneshot::Sender<bool>>,
        surface: ToolPromptSurface,
    ) -> Result<()> {
        match decision {
            ToolPromptDecision::AcceptOnce => {
                respond_tool_approval(response_tx, true);
            }
            ToolPromptDecision::AcceptSession => {
                self.auto_approve_tools = true;
                respond_tool_approval(response_tx, true);
                match surface {
                    ToolPromptSurface::Tui => {
                        self.append_message("* Prompt");
                        self.append_message("  └ session auto-approve enabled");
                    }
                    ToolPromptSurface::Stream => {
                        self.stream_printer.print_session_auto_approve_notice()?;
                    }
                }
            }
            ToolPromptDecision::CancelNewTask => {
                self.suppress_until_turn_complete = true;
                respond_tool_approval(response_tx, false);
                if matches!(surface, ToolPromptSurface::Tui) {
                    self.append_message("* Prompt");
                    self.append_message("  └ tool denied; cancelling current turn");
                }
            }
        }
        Ok(())
    }

    fn suspend_to_shell(&mut self) -> Result<()> {
        let was_tui = self.terminal.take().is_some();
        if was_tui {
            let _ = crate::terminal::restore();
        } else {
            let _ = disable_input_raw_mode();
            let _ = self.stream_printer.reset_scroll_region();
        }

        #[cfg(unix)]
        {
            let pid = std::process::id().to_string();
            let _ = Command::new("kill").args(["-TSTP", pid.as_str()]).status();
        }

        if was_tui {
            self.terminal = Some(crate::terminal::setup()?);
        }

        Ok(())
    }

    fn handle_interrupt(&mut self) -> Result<bool> {
        if self.suppress_until_turn_complete {
            self.should_quit = true;
            return Ok(true);
        }

        if should_clear_prompt_on_interrupt(
            self.terminal.is_some(),
            self.turn_in_progress,
            self.input_editor.buffer.is_empty(),
        ) {
            self.clear_prompt_input();
            self.last_interrupt_at = Some(Instant::now());
            self.append_message("* Prompt");
            self.append_message("  └ cleared input; press Ctrl+C again to exit");
            return Ok(false);
        }

        if self.should_exit_on_interrupt() {
            self.should_quit = true;
            return Ok(true);
        }

        self.cancel_active_turn()?;
        Ok(false)
    }

    fn remember_history(&mut self, command: &str) {
        if command.trim().is_empty() {
            return;
        }
        self.clear_interrupt_window();
        if self
            .input_history
            .last()
            .is_some_and(|last| last == command)
        {
            return;
        }
        self.input_history.push(command.to_string());
        if self.input_history.len() > 400 {
            let drain_count = self.input_history.len() - 400;
            self.input_history.drain(..drain_count);
        }
    }

    fn handle_local_command(&mut self, command: &str) -> Result<bool> {
        if command != "/ps" {
            return Ok(false);
        }

        self.stream_printer
            .print_activity_header(LineStyle::Event, "* Shell: ps")?;
        self.stream_printer.set_style(LineStyle::Event);
        match Command::new("ps")
            .args(["-ax", "-o", "pid=,ppid=,stat=,etime=,command="])
            .output()
        {
            Ok(output) if output.status.success() => {
                let text = String::from_utf8_lossy(&output.stdout);
                let mut line_count = 0usize;
                for line in text.lines().take(40) {
                    println!("  {line}");
                    line_count += 1;
                }
                if text.lines().count() > line_count {
                    println!("  ... (truncated)");
                }
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stderr = stderr.trim();
                if stderr.is_empty() {
                    println!("  └ failed to run ps (exit: {})", output.status);
                } else {
                    println!("  └ {stderr}");
                }
            }
            Err(err) => {
                println!("  └ failed to run ps: {err}");
            }
        }
        self.stream_printer.set_style(LineStyle::Normal);
        io::stdout().flush()?;
        Ok(true)
    }

    fn cancel_active_turn(&mut self) -> Result<()> {
        if self.suppress_until_turn_complete {
            return Ok(());
        }

        if self.terminal.is_some() {
            if !self.turn_in_progress {
                self.append_message("* Prompt");
                self.append_message("  └ press Ctrl+C again to exit");
                return Ok(());
            }
            self.suppress_until_turn_complete = true;
            self.append_message("* Prompt");
            self.append_message("  └ cancelled current response");
            return Ok(());
        }

        self.suppress_until_turn_complete = true;
        if self.stream_printer.turn_active {
            self.stream_printer.end_turn()?;
        }
        self.stream_printer
            .print_activity_header(LineStyle::Event, "* Prompt")?;
        println!("  └ cancelled current response");
        self.stream_printer.set_style(LineStyle::Normal);
        Ok(())
    }

    fn poll_stream_input(
        &mut self,
        editor: &mut LineEditorState,
    ) -> Result<Option<StreamInputAction>> {
        while event::poll(Duration::from_millis(0))? {
            match event::read()? {
                Event::Paste(text) => {
                    if !text.is_empty() {
                        editor.insert_str(&text);
                        self.stream_printer.render_prompt_editor(
                            &editor.buffer,
                            editor.cursor_byte(),
                            true,
                        )?;
                    }
                }
                Event::Key(key)
                    if key.kind == KeyEventKind::Press || key.kind == KeyEventKind::Repeat =>
                {
                    match apply_editor_key_event(editor, key, Some(&self.input_history)) {
                        EditorAction::None => {}
                        EditorAction::Changed => {
                            self.clear_interrupt_window();
                            self.stream_printer.render_prompt_editor(
                                &editor.buffer,
                                editor.cursor_byte(),
                                true,
                            )?;
                        }
                        EditorAction::Cancel => return Ok(Some(StreamInputAction::Cancel)),
                        EditorAction::Interrupt => {
                            return Ok(Some(StreamInputAction::Interrupt));
                        }
                        EditorAction::Suspend => {
                            self.suspend_to_shell()?;
                            enable_input_raw_mode()?;
                            self.stream_printer.render_prompt_editor(
                                &editor.buffer,
                                editor.cursor_byte(),
                                true,
                            )?;
                        }
                        EditorAction::Eof => return Ok(Some(StreamInputAction::Eof)),
                        EditorAction::Submit(text) => {
                            if text.trim().is_empty() {
                                continue;
                            }
                            return Ok(Some(StreamInputAction::Submit(text)));
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(None)
    }

    fn read_user_input(&mut self) -> Result<Option<String>> {
        let Some(first_line) = self.read_user_line(true)? else {
            return Ok(None);
        };
        if first_line.trim() != MULTILINE_PROMPT_START {
            return Ok(Some(first_line));
        }

        self.stream_printer.print_multiline_prompt()?;
        let mut lines = Vec::new();
        loop {
            let Some(line) = self.read_user_line(false)? else {
                break;
            };
            if line.trim() == MULTILINE_PROMPT_END {
                break;
            }
            lines.push(line);
            self.stream_printer.print_multiline_continuation_prompt()?;
        }

        Ok(Some(lines.join("\n")))
    }

    fn read_user_line(&mut self, allow_history: bool) -> Result<Option<String>> {
        if !sticky_prompt_enabled() {
            return Ok(read_user_line_blocking()?.map(|raw| trim_line_endings(&raw)));
        }

        enable_input_raw_mode()?;
        let mut editor = LineEditorState::with_initial("");
        let result = (|| -> Result<Option<String>> {
            self.stream_printer.render_prompt_editor(
                &editor.buffer,
                editor.cursor_byte(),
                false,
            )?;
            loop {
                match event::read()? {
                    Event::Paste(text) => {
                        if !text.is_empty() {
                            editor.insert_str(&text);
                            self.stream_printer.render_prompt_editor(
                                &editor.buffer,
                                editor.cursor_byte(),
                                false,
                            )?;
                        }
                    }
                    Event::Key(key)
                        if key.kind == KeyEventKind::Press || key.kind == KeyEventKind::Repeat =>
                    {
                        match apply_editor_key_event(
                            &mut editor,
                            key,
                            allow_history.then_some(self.input_history.as_slice()),
                        ) {
                            EditorAction::None => {}
                            EditorAction::Changed => {
                                self.clear_interrupt_window();
                                self.stream_printer.render_prompt_editor(
                                    &editor.buffer,
                                    editor.cursor_byte(),
                                    false,
                                )?;
                            }
                            EditorAction::Submit(text) => {
                                self.clear_interrupt_window();
                                return Ok(Some(text));
                            }
                            EditorAction::Cancel => {
                                self.clear_interrupt_window();
                                return Ok(Some("\u{1b}".to_string()));
                            }
                            EditorAction::Interrupt => {
                                if self.should_exit_on_interrupt() {
                                    return Ok(None);
                                }
                                return Ok(Some("\u{1b}".to_string()));
                            }
                            EditorAction::Suspend => {
                                self.suspend_to_shell()?;
                                enable_input_raw_mode()?;
                                self.stream_printer.render_prompt_editor(
                                    &editor.buffer,
                                    editor.cursor_byte(),
                                    false,
                                )?;
                            }
                            EditorAction::Eof => return Ok(None),
                        }
                    }
                    _ => {}
                }
            }
        })();
        let _ = disable_input_raw_mode();
        result
    }
}

impl Drop for App {
    fn drop(&mut self) {
        if self.terminal.take().is_some() {
            let _ = crate::terminal::restore();
        }
    }
}

impl LineEditorState {
    fn with_initial(initial: &str) -> Self {
        Self {
            buffer: initial.to_string(),
            cursor: initial.len(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            history_index: None,
            history_stash: None,
        }
    }

    fn cursor_byte(&self) -> usize {
        clamp_to_char_boundary_left(&self.buffer, self.cursor)
    }

    fn snapshot(&self) -> EditorSnapshot {
        EditorSnapshot {
            buffer: self.buffer.clone(),
            cursor: clamp_to_char_boundary_left(&self.buffer, self.cursor),
        }
    }

    fn clamp_cursor_boundary(&mut self) {
        self.cursor = clamp_to_char_boundary_left(&self.buffer, self.cursor);
    }

    fn push_undo_snapshot(&mut self) {
        let snapshot = self.snapshot();
        if self
            .undo_stack
            .last()
            .is_some_and(|last| last.buffer == snapshot.buffer && last.cursor == snapshot.cursor)
        {
            return;
        }
        self.undo_stack.push(snapshot);
        if self.undo_stack.len() > 256 {
            let drain_count = self.undo_stack.len() - 256;
            self.undo_stack.drain(..drain_count);
        }
    }

    fn clear_history_navigation(&mut self) {
        self.history_index = None;
        self.history_stash = None;
    }

    fn prev_boundary(&self) -> usize {
        let cursor = clamp_to_char_boundary_left(&self.buffer, self.cursor);
        self.buffer[..cursor]
            .char_indices()
            .last()
            .map(|(idx, _)| idx)
            .unwrap_or(0)
    }

    fn next_boundary(&self) -> usize {
        let cursor = clamp_to_char_boundary_left(&self.buffer, self.cursor);
        if cursor >= self.buffer.len() {
            return self.buffer.len();
        }
        self.buffer[cursor..]
            .char_indices()
            .nth(1)
            .map(|(offset, _)| cursor + offset)
            .unwrap_or(self.buffer.len())
    }

    fn set_from_snapshot(&mut self, snapshot: EditorSnapshot) {
        self.buffer = snapshot.buffer;
        self.cursor = clamp_to_char_boundary_left(&self.buffer, snapshot.cursor);
    }

    fn set_buffer_to_end(&mut self, buffer: String) {
        self.buffer = buffer;
        self.cursor = self.buffer.len();
    }

    fn insert_char(&mut self, ch: char) {
        self.clamp_cursor_boundary();
        self.push_undo_snapshot();
        self.buffer.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
        self.redo_stack.clear();
        self.clear_history_navigation();
    }

    fn insert_str(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.clamp_cursor_boundary();
        self.push_undo_snapshot();
        let text = text.replace('\r', "");
        self.buffer.insert_str(self.cursor, &text);
        self.cursor += text.len();
        self.redo_stack.clear();
        self.clear_history_navigation();
    }

    fn backspace(&mut self) -> bool {
        self.clamp_cursor_boundary();
        if self.cursor == 0 {
            return false;
        }
        self.push_undo_snapshot();
        let prev = self.prev_boundary();
        self.buffer.replace_range(prev..self.cursor, "");
        self.cursor = prev;
        self.redo_stack.clear();
        self.clear_history_navigation();
        true
    }

    fn delete_forward(&mut self) -> bool {
        self.clamp_cursor_boundary();
        if self.cursor >= self.buffer.len() {
            return false;
        }
        self.push_undo_snapshot();
        let next = self.next_boundary();
        self.buffer.replace_range(self.cursor..next, "");
        self.redo_stack.clear();
        self.clear_history_navigation();
        true
    }

    fn move_left(&mut self) -> bool {
        self.clamp_cursor_boundary();
        if self.cursor == 0 {
            return false;
        }
        self.cursor = self.prev_boundary();
        true
    }

    fn move_right(&mut self) -> bool {
        self.clamp_cursor_boundary();
        if self.cursor >= self.buffer.len() {
            return false;
        }
        self.cursor = self.next_boundary();
        true
    }

    fn move_home(&mut self) -> bool {
        self.clamp_cursor_boundary();
        if self.cursor == 0 {
            return false;
        }
        self.cursor = 0;
        true
    }

    fn move_end(&mut self) -> bool {
        self.clamp_cursor_boundary();
        if self.cursor == self.buffer.len() {
            return false;
        }
        self.cursor = self.buffer.len();
        true
    }

    fn kill_to_start(&mut self) -> bool {
        self.clamp_cursor_boundary();
        if self.cursor == 0 {
            return false;
        }
        self.push_undo_snapshot();
        self.buffer.replace_range(..self.cursor, "");
        self.cursor = 0;
        self.redo_stack.clear();
        self.clear_history_navigation();
        true
    }

    fn kill_to_end(&mut self) -> bool {
        self.clamp_cursor_boundary();
        if self.cursor >= self.buffer.len() {
            return false;
        }
        self.push_undo_snapshot();
        self.buffer.replace_range(self.cursor.., "");
        self.redo_stack.clear();
        self.clear_history_navigation();
        true
    }

    fn undo(&mut self) -> bool {
        let Some(previous) = self.undo_stack.pop() else {
            return false;
        };
        self.redo_stack.push(self.snapshot());
        self.set_from_snapshot(previous);
        self.clear_history_navigation();
        true
    }

    fn redo(&mut self) -> bool {
        let Some(next) = self.redo_stack.pop() else {
            return false;
        };
        self.undo_stack.push(self.snapshot());
        self.set_from_snapshot(next);
        self.clear_history_navigation();
        true
    }

    fn history_up(&mut self, history: &[String]) -> bool {
        if history.is_empty() {
            return false;
        }

        let next_index = match self.history_index {
            None => {
                self.history_stash = Some(self.buffer.clone());
                history.len() - 1
            }
            Some(current) if current > 0 => current - 1,
            Some(_) => return false,
        };
        self.history_index = Some(next_index);
        self.set_buffer_to_end(history[next_index].clone());
        true
    }

    fn history_down(&mut self, history: &[String]) -> bool {
        let Some(current) = self.history_index else {
            return false;
        };

        if current + 1 < history.len() {
            let next_index = current + 1;
            self.history_index = Some(next_index);
            self.set_buffer_to_end(history[next_index].clone());
            return true;
        }

        self.history_index = None;
        let restored = self.history_stash.take().unwrap_or_default();
        self.set_buffer_to_end(restored);
        true
    }
}

fn should_clear_prompt_on_interrupt(
    is_tui: bool,
    turn_in_progress: bool,
    input_is_empty: bool,
) -> bool {
    is_tui && !turn_in_progress && !input_is_empty
}

fn apply_editor_key_event(
    editor: &mut LineEditorState,
    key: KeyEvent,
    history: Option<&[String]>,
) -> EditorAction {
    if key.code == KeyCode::Enter
        && (key.modifiers.contains(KeyModifiers::SHIFT)
            || key.modifiers.contains(KeyModifiers::ALT))
    {
        editor.insert_char('\n');
        return EditorAction::Changed;
    }

    if key.modifiers.contains(KeyModifiers::ALT)
        && matches!(key.code, KeyCode::Char('z') | KeyCode::Char('Z'))
    {
        return if editor.undo() {
            EditorAction::Changed
        } else {
            EditorAction::None
        };
    }

    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('c') => return EditorAction::Interrupt,
            KeyCode::Char('d') => {
                if editor.buffer.is_empty() {
                    return EditorAction::Eof;
                }
                return if editor.delete_forward() {
                    EditorAction::Changed
                } else {
                    EditorAction::None
                };
            }
            KeyCode::Char('a') => {
                return if editor.move_home() {
                    EditorAction::Changed
                } else {
                    EditorAction::None
                };
            }
            KeyCode::Char('e') => {
                return if editor.move_end() {
                    EditorAction::Changed
                } else {
                    EditorAction::None
                };
            }
            KeyCode::Char('k') => {
                return if editor.kill_to_end() {
                    EditorAction::Changed
                } else {
                    EditorAction::None
                };
            }
            KeyCode::Char('u') => {
                return if editor.kill_to_start() {
                    EditorAction::Changed
                } else {
                    EditorAction::None
                };
            }
            KeyCode::Char('j') => {
                editor.insert_char('\n');
                return EditorAction::Changed;
            }
            KeyCode::Char('z') => {
                return EditorAction::Suspend;
            }
            KeyCode::Char('y') => {
                return if editor.redo() {
                    EditorAction::Changed
                } else {
                    EditorAction::None
                };
            }
            _ => {}
        }
    }

    match key.code {
        KeyCode::Enter => EditorAction::Submit(editor.buffer.clone()),
        KeyCode::Esc => EditorAction::Cancel,
        KeyCode::Backspace => {
            if editor.backspace() {
                EditorAction::Changed
            } else {
                EditorAction::None
            }
        }
        KeyCode::Delete => {
            if editor.delete_forward() {
                EditorAction::Changed
            } else {
                EditorAction::None
            }
        }
        KeyCode::Left => {
            if editor.move_left() {
                EditorAction::Changed
            } else {
                EditorAction::None
            }
        }
        KeyCode::Right => {
            if editor.move_right() {
                EditorAction::Changed
            } else {
                EditorAction::None
            }
        }
        KeyCode::Home => {
            if editor.move_home() {
                EditorAction::Changed
            } else {
                EditorAction::None
            }
        }
        KeyCode::End => {
            if editor.move_end() {
                EditorAction::Changed
            } else {
                EditorAction::None
            }
        }
        KeyCode::Up => {
            if history.is_some_and(|entries| editor.history_up(entries)) {
                EditorAction::Changed
            } else {
                EditorAction::None
            }
        }
        KeyCode::Down => {
            if history.is_some_and(|entries| editor.history_down(entries)) {
                EditorAction::Changed
            } else {
                EditorAction::None
            }
        }
        KeyCode::Tab => {
            editor.insert_char('\t');
            EditorAction::Changed
        }
        KeyCode::Char(c)
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT) =>
        {
            editor.insert_char(c);
            EditorAction::Changed
        }
        _ => EditorAction::None,
    }
}

fn sticky_prompt_layout(requested_input_rows: usize) -> StickyPromptLayout {
    let (cols, rows) = terminal_size().unwrap_or((DEFAULT_PROMPT_AREA_WIDTH as u16, 24));
    let width = prompt_area_safe_width(cols);
    let rows = usize::from(rows.max(1));
    let max_input_rows = rows.saturating_sub(2).max(1);
    let input_rows = requested_input_rows.clamp(1, max_input_rows);
    let area_rows = input_rows + 2;
    let bottom_row = rows;
    let top_row = bottom_row
        .saturating_sub(area_rows.saturating_sub(1))
        .max(1);
    let prompt_row = (top_row + 1).min(bottom_row);
    let output_bottom_row = top_row.saturating_sub(1).max(1);
    StickyPromptLayout {
        width,
        input_width: width.saturating_sub(2).max(1),
        top_row,
        prompt_row,
        bottom_row,
        output_bottom_row,
        input_rows,
    }
}

fn format_tool_status_flow(history: &[ToolStatus]) -> String {
    let mut labels: Vec<&'static str> = Vec::new();
    for status in history {
        let label = match status {
            ToolStatus::Pending => "preparing",
            ToolStatus::WaitingApproval => "awaiting approval",
            ToolStatus::Executing => "running",
            ToolStatus::Complete => "done",
            ToolStatus::Cancelled => "cancelled",
        };
        if labels.last() != Some(&label) {
            labels.push(label);
        }
    }
    if labels.is_empty() {
        "pending".to_string()
    } else {
        labels.join(" · ")
    }
}

fn respond_tool_approval(
    response_tx: &mut Option<tokio::sync::oneshot::Sender<bool>>,
    approved: bool,
) {
    if let Some(tx) = response_tx.take() {
        let _ = tx.send(approved);
    }
}

fn trim_line_endings(input: &str) -> String {
    input.trim_end_matches(['\r', '\n']).to_string()
}

fn enable_input_raw_mode() -> Result<()> {
    enable_raw_mode()?;
    if let Err(err) = execute!(io::stdout(), EnableBracketedPaste) {
        let _ = disable_raw_mode();
        return Err(err.into());
    }
    Ok(())
}

fn disable_input_raw_mode() -> Result<()> {
    let _ = execute!(io::stdout(), DisableBracketedPaste);
    disable_raw_mode()?;
    Ok(())
}

fn read_user_line_blocking() -> Result<Option<String>> {
    let mut input = String::new();
    let bytes = io::stdin().read_line(&mut input)?;
    if bytes == 0 {
        Ok(None)
    } else {
        Ok(Some(input))
    }
}

fn tool_prompt_decision_from_key_code(code: KeyCode) -> Option<ToolPromptDecision> {
    match code {
        KeyCode::Char('1') | KeyCode::Char('y') | KeyCode::Char('Y') => {
            Some(ToolPromptDecision::AcceptOnce)
        }
        KeyCode::Char('2') | KeyCode::Char('a') | KeyCode::Char('A') => {
            Some(ToolPromptDecision::AcceptSession)
        }
        KeyCode::Char('3') | KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            Some(ToolPromptDecision::CancelNewTask)
        }
        _ => None,
    }
}

fn tool_prompt_decision_from_text(input: &str) -> Option<ToolPromptDecision> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Some(ToolPromptDecision::CancelNewTask);
    }
    if trimmed.starts_with('1') {
        return Some(ToolPromptDecision::AcceptOnce);
    }
    if trimmed.starts_with('2') {
        return Some(ToolPromptDecision::AcceptSession);
    }
    if trimmed.starts_with('3') {
        return Some(ToolPromptDecision::CancelNewTask);
    }

    let lowered = trimmed.to_ascii_lowercase();
    if lowered.starts_with('y') || lowered.starts_with("yes") {
        return Some(ToolPromptDecision::AcceptOnce);
    }
    if lowered.starts_with('a') || lowered.starts_with("always") {
        return Some(ToolPromptDecision::AcceptSession);
    }
    if lowered.starts_with('n') || lowered.starts_with("no") || lowered == "esc" {
        return Some(ToolPromptDecision::CancelNewTask);
    }
    None
}

async fn read_tool_confirmation() -> Result<ToolPromptDecision> {
    if sticky_prompt_enabled() {
        return task::spawn_blocking(|| -> Result<ToolPromptDecision> {
            enable_input_raw_mode()?;
            let decision = (|| -> Result<ToolPromptDecision> {
                loop {
                    match event::read()? {
                        Event::Key(event) if event.kind == KeyEventKind::Press => {
                            if let Some(decision) = tool_prompt_decision_from_key_code(event.code) {
                                println!();
                                io::stdout().flush()?;
                                return Ok(decision);
                            }
                        }
                        _ => {}
                    }
                }
            })();
            let _ = disable_input_raw_mode();
            decision
        })
        .await?;
    }

    loop {
        let Some(raw) = read_user_line_blocking()? else {
            return Ok(ToolPromptDecision::CancelNewTask);
        };
        let trimmed = raw.trim();
        if is_escape_command(trimmed) {
            return Ok(ToolPromptDecision::CancelNewTask);
        }
        if let Some(decision) = tool_prompt_decision_from_text(trimmed) {
            return Ok(decision);
        }
        print!("* Select > ");
        io::stdout().flush()?;
    }
}

fn line_style(line: &str, in_code_block: bool, colors_enabled: bool) -> LineStyle {
    if !colors_enabled {
        return LineStyle::Normal;
    }

    let trimmed = strip_optional_number_prefix(line);
    if in_code_block && trimmed.starts_with('+') && !trimmed.starts_with("+++") {
        LineStyle::Add
    } else if in_code_block && trimmed.starts_with('-') && !trimmed.starts_with("---") {
        LineStyle::Delete
    } else if has_number_prefix(line) && trimmed.starts_with('+') && !trimmed.starts_with("+++") {
        LineStyle::Add
    } else if has_number_prefix(line) && trimmed.starts_with('-') && !trimmed.starts_with("---") {
        LineStyle::Delete
    } else if trimmed.starts_with("+ [tool_result]") {
        LineStyle::Add
    } else if trimmed.starts_with("- [tool_error]") {
        LineStyle::Delete
    } else if trimmed.starts_with("* Thinking") {
        LineStyle::Thinking
    } else if trimmed.starts_with("* Tool") {
        LineStyle::Tool
    } else if trimmed.starts_with("* Event:") {
        LineStyle::Event
    } else {
        LineStyle::Normal
    }
}

fn strip_optional_number_prefix(line: &str) -> &str {
    let trimmed = line.trim_start();
    if let Some((left, right)) = trimmed.split_once('|') {
        let left = left.trim();
        if !left.is_empty() && left.chars().all(|c| c.is_ascii_digit()) {
            return right.trim_start();
        }
    }

    let digits = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 {
        let rest = &trimmed[digits..];
        if rest.starts_with(' ') {
            return rest.trim_start();
        }
    }
    trimmed
}

fn has_number_prefix(line: &str) -> bool {
    let trimmed = line.trim_start();
    if let Some((left, _)) = trimmed.split_once('|') {
        let left = left.trim();
        if !left.is_empty() && left.chars().all(|c| c.is_ascii_digit()) {
            return true;
        }
    }

    let digits = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return false;
    }
    trimmed[digits..].starts_with(' ')
}

fn format_code_line_prefix(line_number: usize, colors_enabled: bool) -> String {
    if colors_enabled {
        format!("  \x1b[1m{line_number}\x1b[0m ")
    } else {
        format!("  {line_number} ")
    }
}

fn normalize_existing_numbered_snippet_line(line: &str) -> Option<String> {
    if looks_like_activity_line(line) {
        return None;
    }

    let trimmed = line.trim_start();
    let leading_spaces = line.chars().take_while(|c| c.is_ascii_whitespace()).count();

    if let Some((left, right)) = trimmed.split_once('|') {
        let number = left.trim();
        if !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()) {
            return Some(format!("  {number} {}", right.trim_start()));
        }
    }

    let digit_count = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
    if digit_count == 0 || leading_spaces < 2 {
        return None;
    }

    let number = trimmed[..digit_count].trim();
    let rest = trimmed[digit_count..].trim_start();
    if rest.is_empty() {
        return None;
    }

    Some(format!("  {number} {rest}"))
}

fn run_git_capture(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn detect_repo_widget() -> Option<RepoWidgetState> {
    let top = run_git_capture(&["rev-parse", "--show-toplevel"])?;
    let root_name = Path::new(top.trim())
        .file_name()
        .map(|name| name.to_string_lossy().to_string())?;
    let branch = run_git_capture(&["rev-parse", "--abbrev-ref", "HEAD"])
        .unwrap_or_else(|| "detached".to_string());
    let status = run_git_capture(&["status", "--porcelain"]).unwrap_or_default();
    let changed_entries = status.lines().count();

    Some(RepoWidgetState {
        name: root_name,
        branch,
        dirty: changed_entries > 0,
        changed_entries,
    })
}

fn detect_color_support() -> bool {
    if std::env::var("AISTAR_FORCE_COLOR")
        .ok()
        .and_then(parse_bool_flag)
        .unwrap_or(false)
    {
        return true;
    }

    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }

    io::stdout().is_terminal()
}

fn sticky_prompt_enabled() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

fn disable_cursor() -> bool {
    std::env::var("AISTAR_DISABLE_CURSOR")
        .ok()
        .and_then(parse_bool_flag)
        .unwrap_or(false)
}

fn disable_frame_batching() -> bool {
    std::env::var("AISTAR_DISABLE_FRAME_BATCHING")
        .ok()
        .and_then(parse_bool_flag)
        .unwrap_or(false)
}

fn disable_progressive_effects() -> bool {
    std::env::var("AISTAR_DISABLE_PROGRESSIVE_EFFECTS")
        .ok()
        .and_then(parse_bool_flag)
        .unwrap_or(false)
}

fn use_structured_blocks() -> bool {
    std::env::var("AISTAR_USE_STRUCTURED_BLOCKS")
        .ok()
        .and_then(parse_bool_flag)
        .unwrap_or(true)
}

fn resolve_frame_interval() -> Duration {
    let ms = std::env::var("AISTAR_FRAME_INTERVAL_MS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(|v| v.clamp(4, 250))
        .unwrap_or(DEFAULT_FRAME_INTERVAL.as_millis() as u64);
    Duration::from_millis(ms)
}

fn structured_tool_input_preview(name: &str, input: &serde_json::Value) -> String {
    preview_tool_input(
        name,
        input,
        ToolPreviewStyle::Structured,
        DEFAULT_EDIT_DIFF_CONTEXT_LINES,
    )
}

#[cfg(test)]
fn structured_preview_lines(text: &str, diff_marker: Option<char>) -> String {
    crate::tool_preview::preview_lines(diff_marker, text, usize::MAX, 1, "    ")
}

fn looks_like_activity_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("* Thinking")
        || trimmed.starts_with("* Tool")
        || trimmed.starts_with("* Event:")
        || trimmed.starts_with("* Tool Execution:")
}

fn is_escape_command(input: &str) -> bool {
    input == "\u{1b}" || matches!(input, "esc" | "/esc" | "escape" | "/escape")
}

fn format_server_activity_line(line: &str) -> Option<String> {
    if let Some(rest) = line.strip_prefix("* Event:") {
        return Some(format!("  - event: {}", rest.trim()));
    }
    if let Some(rest) = line.strip_prefix("* Tool:") {
        return Some(format!("    1. tool: {}", rest.trim()));
    }
    None
}

fn format_progressive_response_line(line: &str) -> String {
    let leading_spaces = line.chars().take_while(|c| c.is_ascii_whitespace()).count();
    let inferred_level = (leading_spaces / 2).min(2);
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if is_separator_like(trimmed) {
        return String::new();
    }

    if let Some(rest) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("• "))
    {
        return format!("{}- {}", response_indent(inferred_level), rest.trim());
    }

    if let Some((num, rest)) = parse_numbered_list_marker(trimmed) {
        return format!(
            "{}{}. {}",
            response_indent(inferred_level.max(1)),
            num,
            rest.trim()
        );
    }

    if let Some((letter, rest)) = parse_lettered_list_marker(trimmed) {
        return format!(
            "{}{}. {}",
            response_indent(inferred_level.max(2)),
            letter.to_ascii_lowercase(),
            rest.trim()
        );
    }

    format!("{}- {}", response_indent(inferred_level), trimmed)
}

fn response_indent(level: usize) -> String {
    " ".repeat(2 + level * 2)
}

fn is_separator_like(text: &str) -> bool {
    text.len() >= 3
        && text
            .chars()
            .all(|ch| matches!(ch, '=' | '-' | '_' | '~' | '*' | '·'))
}

fn parse_numbered_list_marker(line: &str) -> Option<(&str, &str)> {
    let digit_count = line.chars().take_while(|c| c.is_ascii_digit()).count();
    if digit_count == 0 {
        return None;
    }
    let rest = &line[digit_count..];
    if !(rest.starts_with(". ") || rest.starts_with(") ")) {
        return None;
    }
    Some((&line[..digit_count], rest[2..].trim_start()))
}

fn parse_lettered_list_marker(line: &str) -> Option<(char, &str)> {
    let mut chars = line.chars();
    let letter = chars.next()?;
    if !letter.is_ascii_alphabetic() {
        return None;
    }
    let rest = &line[letter.len_utf8()..];
    if !(rest.starts_with(". ") || rest.starts_with(") ")) {
        return None;
    }
    Some((letter, rest[2..].trim_start()))
}

fn thinking_inline_text(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if let Some(tool) = trimmed.strip_prefix("* Tool:") {
        return Some(format!("Tool call:{}.", tool.trim()));
    }
    if let Some(event) = trimmed.strip_prefix("* Event: input_json#") {
        return Some(format!("Tool input stream: input_json#{}.", event.trim()));
    }
    if trimmed.starts_with("* Event: stop_reason=tool_use") {
        return Some("Assistant paused for tool execution.".to_string());
    }
    None
}

fn format_thinking_segments(segments: &[String]) -> Vec<String> {
    if segments.is_empty() {
        return Vec::new();
    }

    let mut display_lines: Vec<String> = segments
        .iter()
        .map(|segment| segment.trim())
        .filter(|segment| !segment.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    if display_lines.is_empty() {
        return Vec::new();
    }

    if display_lines.len() > THINKING_BLOB_MAX_LINES {
        let hidden = display_lines.len() - (THINKING_BLOB_MAX_LINES - 1);
        display_lines.truncate(THINKING_BLOB_MAX_LINES - 1);
        display_lines.push(format!("... (+{hidden} more)"));
    }

    let first = display_lines[0].clone();
    display_lines[0] = format!("+ leading: {first}");

    let mut out = Vec::with_capacity(display_lines.len() + 1);
    for (idx, segment) in display_lines.iter().enumerate() {
        let prefix = if idx + 1 == display_lines.len() {
            "  └ "
        } else {
            "  │ "
        };
        out.push(format!("{prefix}{segment}"));
    }
    out.push(String::new());
    out
}

fn wrap_text_for_display(text: &str, width: usize) -> Vec<String> {
    let clean = text.trim();
    if clean.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut current = String::new();

    for word in clean.split_whitespace() {
        if current.is_empty() {
            current.push_str(word);
            continue;
        }

        if current.len() + 1 + word.len() <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            out.push(std::mem::take(&mut current));
            current.push_str(word);
        }
    }

    if !current.is_empty() {
        out.push(current);
    }

    out
}

fn is_numbered_preview_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    let digits = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 && trimmed[digits..].starts_with(' ') {
        return true;
    }

    line.starts_with("  ...")
}

#[cfg(test)]
fn is_checklist_like(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with("- ") || trimmed.starts_with("• ") || trimmed.starts_with("* ") {
        return true;
    }

    let digits = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
    digits > 0
        && (trimmed[digits..].starts_with(". ")
            || trimmed[digits..].starts_with(") ")
            || trimmed[digits..].starts_with(" - "))
}

fn resolve_thinking_wrap_width() -> usize {
    if let Some(explicit) = std::env::var("AISTAR_THINKING_WRAP_WIDTH")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
    {
        return explicit.clamp(40, 300);
    }

    if let Ok((cols, _)) = terminal_size() {
        let usable = cols.saturating_sub(6) as usize;
        return usable.clamp(40, 300);
    }

    DEFAULT_THINKING_WRAP_WIDTH
}

fn prompt_area_safe_width(cols: u16) -> usize {
    usize::from(cols).clamp(20, 240).saturating_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn dummy_ctx<'a>(conversation: &'a mut ConversationManager) -> RuntimeContext<'a> {
        RuntimeContext { conversation }
    }

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
    fn test_line_style_feedback() {
        assert_eq!(line_style("+added", true, true), LineStyle::Add);
        assert_eq!(line_style("   +added", true, true), LineStyle::Add);
        assert_eq!(line_style("  12 | +added", true, true), LineStyle::Add);
        assert_eq!(line_style("-removed", true, true), LineStyle::Delete);
        assert_eq!(line_style("   -removed", true, true), LineStyle::Delete);
        assert_eq!(line_style("9| -removed", true, true), LineStyle::Delete);
        assert_eq!(line_style("    12 +added", false, true), LineStyle::Add);
        assert_eq!(
            line_style("    12 -removed", false, true),
            LineStyle::Delete
        );
        assert_eq!(
            line_style("+ [tool_result] read_file", false, true),
            LineStyle::Add
        );
        assert_eq!(
            line_style("- [tool_error] edit_file: failed", false, true),
            LineStyle::Delete
        );
        assert_eq!(line_style("* Thinking", false, true), LineStyle::Thinking);
        assert_eq!(
            line_style("* Tool: edit_file", false, true),
            LineStyle::Tool
        );
        assert_eq!(
            line_style("* Event: message_start", false, true),
            LineStyle::Event
        );
        assert_eq!(line_style("normal", false, true), LineStyle::Normal);
        assert_eq!(line_style("+added", true, false), LineStyle::Normal);
    }

    #[test]
    fn test_code_block_toggle() {
        let mut printer = StreamPrinter::new();
        printer.update_code_block_state_for_finished_line("```rust");
        assert!(printer.in_code_block);
        assert_eq!(printer.code_line_number, 1);

        printer.update_code_block_state_for_finished_line("```");
        assert!(!printer.in_code_block);
    }

    #[test]
    fn test_format_code_line_prefix_alignment() {
        assert_eq!(format_code_line_prefix(1, false), "  1 ");
        assert_eq!(format_code_line_prefix(604, false), "  604 ");
    }

    #[test]
    fn test_normalize_existing_numbered_snippet_line() {
        assert_eq!(
            normalize_existing_numbered_snippet_line("   12 | +hello").as_deref(),
            Some("  12 +hello")
        );
        assert_eq!(
            normalize_existing_numbered_snippet_line("604 | println!(\"ok\");").as_deref(),
            Some("  604 println!(\"ok\");")
        );
        assert_eq!(
            normalize_existing_numbered_snippet_line("    603 +        assert_eq!(x, y);")
                .as_deref(),
            Some("  603 +        assert_eq!(x, y);")
        );
        assert!(normalize_existing_numbered_snippet_line("* Event: message_start").is_none());
        assert!(normalize_existing_numbered_snippet_line("not a numbered line").is_none());
    }

    #[test]
    fn test_activity_and_escape_helpers() {
        assert!(looks_like_activity_line("* Thinking"));
        assert!(looks_like_activity_line("* Tool: read_file"));
        assert!(looks_like_activity_line("* Event: message_stop"));
        assert!(!looks_like_activity_line("normal line"));

        assert!(is_escape_command("\u{1b}"));
        assert!(is_escape_command("esc"));
        assert!(is_escape_command("/escape"));
        assert!(!is_escape_command("1"));
    }

    #[test]
    fn test_format_thinking_segments_last_line_uses_corner() {
        let lines = vec![
            "first".to_string(),
            "second".to_string(),
            "third".to_string(),
        ];
        assert_eq!(
            format_thinking_segments(&lines),
            vec![
                "  │ + leading: first".to_string(),
                "  │ second".to_string(),
                "  └ third".to_string(),
                String::new()
            ]
        );
    }

    #[test]
    fn test_format_thinking_segments_caps_to_four_lines_with_summary() {
        let lines = vec![
            "a".to_string(),
            "b".to_string(),
            "c".to_string(),
            "d".to_string(),
            "e".to_string(),
        ];
        assert_eq!(
            format_thinking_segments(&lines),
            vec![
                "  │ + leading: a".to_string(),
                "  │ b".to_string(),
                "  │ c".to_string(),
                "  └ ... (+2 more)".to_string(),
                String::new()
            ]
        );
    }

    #[test]
    fn test_trim_line_endings() {
        assert_eq!(trim_line_endings("hello\r\n"), "hello");
        assert_eq!(trim_line_endings("world\n"), "world");
    }

    #[test]
    fn test_wrap_text_for_display() {
        let wrapped = wrap_text_for_display(
            "this is a sentence that should wrap into multiple lines",
            16,
        );
        assert_eq!(
            wrapped,
            vec![
                "this is a".to_string(),
                "sentence that".to_string(),
                "should wrap into".to_string(),
                "multiple lines".to_string()
            ]
        );
    }

    #[test]
    fn test_is_numbered_preview_line() {
        assert!(is_numbered_preview_line("  12 - line"));
        assert!(is_numbered_preview_line("  ... (4 more lines)"));
        assert!(!is_numbered_preview_line("old_str: 12 chars"));
    }

    #[test]
    fn test_structured_tool_input_preview_edit_file_has_numbered_diff_lines() {
        let input = serde_json::json!({
            "path": "cal.rs",
            "old_str": "fn a() {}\nfn b() {}",
            "new_str": "fn a() {\n    1\n}\nfn b() {}"
        });

        let preview = structured_tool_input_preview("edit_file", &input);
        assert!(preview.contains("path: cal.rs"));
        assert!(preview.contains("change:"));
        assert!(preview.contains("@@ -1,2 +1,4 @@"));
        assert!(preview.contains("    1 - fn a() {}"));
        assert!(preview.contains("    1 + fn a() {"));
        assert!(preview.contains("    2   fn b() {}"));
    }

    #[test]
    fn test_structured_tool_input_preview_list_and_generic_are_concise() {
        let list_preview = structured_tool_input_preview("list_files", &serde_json::json!({}));
        assert!(list_preview.contains("path: ."));
        assert!(list_preview.contains("max_entries: 100"));

        let search_preview =
            structured_tool_input_preview("search", &serde_json::json!({"query":"radical"}));
        assert!(search_preview.contains("query: radical"));
        assert!(search_preview.contains("max_results: 30"));

        let generic_preview = structured_tool_input_preview("unknown_tool", &serde_json::json!({}));
        assert_eq!(generic_preview, "(no arguments)");
    }

    #[test]
    fn test_tool_approval_prompt_skips_duplicate_tool_context_in_structured_mode() {
        let mut printer = StreamPrinter::new();
        printer.colors_enabled = false;
        printer.structured_blocks_enabled = true;
        printer.active_block = BlockKind::Tool;

        printer
            .print_tool_approval_prompt("read_file", "path: cal.rs")
            .unwrap();

        // Only the prompt section should be rendered because tool context is already on screen.
        assert_eq!(printer.activity_blob_count, 1);
    }

    #[test]
    fn test_structured_preview_lines_empty_content() {
        assert_eq!(structured_preview_lines("", Some('+')), "    1 + <empty>\n");
        assert_eq!(structured_preview_lines("", None), "    1   <empty>\n");
    }

    #[test]
    fn test_content_stats() {
        assert_eq!(content_stats(""), (0, 0));
        assert_eq!(content_stats("a"), (1, 1));
        assert_eq!(content_stats("a\nb"), (3, 2));
    }

    #[test]
    fn test_thinking_inline_text() {
        assert_eq!(
            thinking_inline_text("* Tool: read_file").as_deref(),
            Some("Tool call:read_file.")
        );
        assert_eq!(
            thinking_inline_text("* Event: input_json#1").as_deref(),
            Some("Tool input stream: input_json#1.")
        );
        assert_eq!(
            thinking_inline_text("* Event: stop_reason=tool_use").as_deref(),
            Some("Assistant paused for tool execution.")
        );
        assert!(thinking_inline_text("* Event: message_start").is_none());
    }

    #[test]
    fn test_progressive_response_and_server_activity_formatting() {
        assert_eq!(
            format_progressive_response_line("plain text"),
            "  - plain text"
        );
        assert_eq!(
            format_progressive_response_line("- unordered"),
            "  - unordered"
        );
        assert_eq!(
            format_progressive_response_line("2. numbered"),
            "    2. numbered"
        );
        assert_eq!(
            format_progressive_response_line("B) nested"),
            "      b. nested"
        );
        assert_eq!(
            format_progressive_response_line("  - nested bullet"),
            "    - nested bullet"
        );
        assert_eq!(
            format_progressive_response_line("    3. nested number"),
            "      3. nested number"
        );
        assert_eq!(format_progressive_response_line("====----"), "");
        assert_eq!(response_indent(0), "  ");
        assert_eq!(response_indent(1), "    ");
        assert_eq!(response_indent(2), "      ");

        assert_eq!(
            format_server_activity_line("* Event: message_stop").as_deref(),
            Some("  - event: message_stop")
        );
        assert_eq!(
            format_server_activity_line("* Tool: read_file").as_deref(),
            Some("    1. tool: read_file")
        );
        assert!(format_server_activity_line("normal").is_none());
    }

    #[test]
    fn test_is_checklist_like() {
        assert!(is_checklist_like("- task"));
        assert!(is_checklist_like("1. task"));
        assert!(is_checklist_like("• task"));
        assert!(!is_checklist_like("plain sentence"));
    }

    #[test]
    fn test_frame_batching_buffers_and_flushes_tokens() {
        let mut printer = StreamPrinter::new();
        printer.frame_batching_enabled = true;
        printer.cursor_enabled = false;

        printer.buffer_token("Hello").unwrap();
        printer.buffer_token(" World").unwrap();
        assert_eq!(printer.pending_tokens, "Hello World");

        printer.flush_buffered_tokens().unwrap();
        assert!(printer.pending_tokens.is_empty());
    }

    #[test]
    fn test_cursor_blink_phase_toggles_on_tick() {
        let mut printer = StreamPrinter::new();
        printer.cursor_enabled = true;
        printer.cursor_visible = true;
        printer.current_line = "streaming".to_string();
        printer.last_cursor_toggle = Instant::now() - CURSOR_BLINK_INTERVAL;

        let before = printer.cursor_blink_phase;
        printer.on_cursor_tick().unwrap();
        assert_ne!(printer.cursor_blink_phase, before);
    }

    #[test]
    fn test_saw_response_block_flag_set_on_final_text_block() {
        let mut printer = StreamPrinter::new();
        printer.colors_enabled = false;
        printer.structured_blocks_enabled = true;
        printer.begin_turn();
        assert!(!printer.saw_response_block());

        printer
            .on_block_start(
                0,
                StreamBlock::FinalText {
                    content: "ok".to_string(),
                },
            )
            .unwrap();

        assert!(printer.saw_response_block());
    }

    #[test]
    fn test_block_start_reuse_new_tool_call_id_resets_round_blocks() {
        let mut printer = StreamPrinter::new();
        printer.structured_blocks_enabled = true;
        printer.active_blocks = vec![
            StreamBlock::ToolCall {
                id: "tool-old".to_string(),
                name: "read_file".to_string(),
                input: serde_json::json!({}),
                status: ToolStatus::WaitingApproval,
            },
            StreamBlock::Thinking {
                content: "stale".to_string(),
                collapsed: false,
            },
        ];

        printer
            .on_block_start(
                0,
                StreamBlock::ToolCall {
                    id: "tool-new".to_string(),
                    name: "read_file".to_string(),
                    input: serde_json::json!({}),
                    status: ToolStatus::Pending,
                },
            )
            .unwrap();

        assert_eq!(printer.active_blocks.len(), 1);
        match &printer.active_blocks[0] {
            StreamBlock::ToolCall { id, .. } => assert_eq!(id, "tool-new"),
            _ => panic!("expected tool call"),
        }
    }

    #[test]
    fn test_block_start_reuse_same_tool_call_id_keeps_existing_slots() {
        let mut printer = StreamPrinter::new();
        printer.structured_blocks_enabled = true;
        printer.active_blocks = vec![
            StreamBlock::ToolCall {
                id: "tool-1".to_string(),
                name: "read_file".to_string(),
                input: serde_json::json!({}),
                status: ToolStatus::WaitingApproval,
            },
            StreamBlock::Thinking {
                content: "keep".to_string(),
                collapsed: false,
            },
        ];

        printer
            .on_block_start(
                0,
                StreamBlock::ToolCall {
                    id: "tool-1".to_string(),
                    name: "read_file".to_string(),
                    input: serde_json::json!({}),
                    status: ToolStatus::Executing,
                },
            )
            .unwrap();

        assert_eq!(printer.active_blocks.len(), 2);
    }

    #[test]
    fn test_line_editor_history_and_undo_redo() {
        let history = vec!["first".to_string(), "second".to_string()];
        let mut editor = LineEditorState::with_initial("seed");

        assert!(editor.history_up(&history));
        assert_eq!(editor.buffer, "second");
        assert!(editor.history_up(&history));
        assert_eq!(editor.buffer, "first");
        assert!(editor.history_down(&history));
        assert_eq!(editor.buffer, "second");
        assert!(editor.history_down(&history));
        assert_eq!(editor.buffer, "seed");

        editor.insert_char('x');
        assert_eq!(editor.buffer, "seedx");
        assert!(editor.undo());
        assert_eq!(editor.buffer, "seed");
        assert!(editor.redo());
        assert_eq!(editor.buffer, "seedx");
    }

    #[test]
    fn test_apply_editor_key_event_submit_cancel_and_eof() {
        let mut editor = LineEditorState::with_initial("abc");
        let submit = apply_editor_key_event(
            &mut editor,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            None,
        );
        assert_eq!(submit, EditorAction::Submit("abc".to_string()));

        let cancel = apply_editor_key_event(
            &mut editor,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            None,
        );
        assert_eq!(cancel, EditorAction::Cancel);

        let interrupt = apply_editor_key_event(
            &mut editor,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            None,
        );
        assert_eq!(interrupt, EditorAction::Interrupt);

        let suspend = apply_editor_key_event(
            &mut editor,
            KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL),
            None,
        );
        assert_eq!(suspend, EditorAction::Suspend);

        let mut empty = LineEditorState::with_initial("");
        let eof = apply_editor_key_event(
            &mut empty,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL),
            None,
        );
        assert_eq!(eof, EditorAction::Eof);
    }

    #[test]
    fn test_apply_editor_key_event_multiline_shortcuts() {
        let mut shift_editor = LineEditorState::with_initial("hi");
        let shift_enter = apply_editor_key_event(
            &mut shift_editor,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT),
            None,
        );
        assert_eq!(shift_enter, EditorAction::Changed);
        assert_eq!(shift_editor.buffer, "hi\n");

        let mut ctrl_j_editor = LineEditorState::with_initial("ok");
        let ctrl_j = apply_editor_key_event(
            &mut ctrl_j_editor,
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL),
            None,
        );
        assert_eq!(ctrl_j, EditorAction::Changed);
        assert_eq!(ctrl_j_editor.buffer, "ok\n");
    }

    #[test]
    fn test_clamp_to_char_boundary_left_for_unicode() {
        let sample = "a😀b";
        assert_eq!(clamp_to_char_boundary_left(sample, 0), 0);
        assert_eq!(clamp_to_char_boundary_left(sample, 1), 1);
        assert_eq!(clamp_to_char_boundary_left(sample, 2), 1);
        assert_eq!(clamp_to_char_boundary_left(sample, 4), 1);
        assert_eq!(clamp_to_char_boundary_left(sample, 5), 5);
        assert_eq!(clamp_to_char_boundary_left(sample, 99), sample.len());
    }

    #[test]
    fn test_should_clear_prompt_on_interrupt_conditions() {
        assert!(should_clear_prompt_on_interrupt(true, false, false));
        assert!(!should_clear_prompt_on_interrupt(true, true, false));
        assert!(!should_clear_prompt_on_interrupt(true, false, true));
        assert!(!should_clear_prompt_on_interrupt(false, false, false));
    }

    #[test]
    fn test_prompt_cursor_wrap_and_status_flow_formatting() {
        let wrapped = wrap_input_lines("123456", 4);
        assert_eq!(wrapped, vec!["1234".to_string(), "56".to_string()]);
        assert_eq!(cursor_row_col("1234", 4, 4), (1, 0));

        let flow = format_tool_status_flow(&[
            ToolStatus::Pending,
            ToolStatus::Executing,
            ToolStatus::Executing,
            ToolStatus::Complete,
        ]);
        assert_eq!(flow, "preparing · running · done");
    }

    #[test]
    fn test_tool_prompt_decision_mappings_are_shared() {
        assert_eq!(
            tool_prompt_decision_from_key_code(KeyCode::Char('y')),
            Some(ToolPromptDecision::AcceptOnce)
        );
        assert_eq!(
            tool_prompt_decision_from_key_code(KeyCode::Char('A')),
            Some(ToolPromptDecision::AcceptSession)
        );
        assert_eq!(
            tool_prompt_decision_from_key_code(KeyCode::Esc),
            Some(ToolPromptDecision::CancelNewTask)
        );
        assert_eq!(
            tool_prompt_decision_from_text("yes"),
            Some(ToolPromptDecision::AcceptOnce)
        );
        assert_eq!(
            tool_prompt_decision_from_text("always"),
            Some(ToolPromptDecision::AcceptSession)
        );
        assert_eq!(
            tool_prompt_decision_from_text("no"),
            Some(ToolPromptDecision::CancelNewTask)
        );
    }

    #[test]
    fn test_ref_03_tui_mode_overlay_blocks_input() {
        use crate::api::{mock_client::MockApiClient, ApiClient};
        use crate::runtime::mode::RuntimeMode;
        use std::collections::HashMap;
        use std::sync::Arc;

        let mock_api_client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
        let mut conversation = ConversationManager::new_mock(mock_api_client, HashMap::new());
        let mut ctx = dummy_ctx(&mut conversation);

        let mut mode = TuiMode::new();
        assert!(!mode.is_turn_in_progress());

        let req = ToolApprovalRequest::test_stub();
        mode.on_model_update(UiUpdate::ToolApprovalRequest(req), &mut ctx);

        assert!(mode.overlay.is_some());
        mode.on_user_input("should be ignored".to_string(), &mut ctx);
        assert!(
            !mode.is_turn_in_progress(),
            "turn must not start while overlay is active"
        );

        mode.overlay = None;
        mode.on_user_input("now accepted".to_string(), &mut ctx);
        assert!(
            mode.is_turn_in_progress(),
            "turn should start after overlay cleared"
        );
    }
}
