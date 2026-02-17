use crate::config::Config;
use crate::edit_diff::{format_edit_hunks, DEFAULT_EDIT_DIFF_CONTEXT_LINES};
use crate::state::{
    ConversationManager, ConversationStreamUpdate, StreamBlock, ToolApprovalRequest, ToolStatus,
};
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, size as terminal_size};
use std::io::{self, IsTerminal, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Mutex};
use tokio::task;

const THINKING_MAX_LINES: usize = 4;
const DEFAULT_THINKING_WRAP_WIDTH: usize = 96;
const CURSOR_BLINK_INTERVAL: Duration = Duration::from_millis(530);
const DEFAULT_FRAME_INTERVAL: Duration = Duration::from_millis(16);

pub enum UiUpdate {
    StreamDelta(String),
    StreamBlockStart { index: usize, block: StreamBlock },
    StreamBlockDelta { index: usize, delta: String },
    StreamBlockComplete { index: usize },
    ToolApprovalRequest(ToolApprovalRequest),
    TurnComplete(String),
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolPromptDecision {
    AcceptOnce,
    AcceptSession,
    CancelNewTask,
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
    thinking_rendered_lines: usize,
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
            thinking_rendered_lines: 0,
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
        self.thinking_rendered_lines = 0;
        self.cursor_visible = false;
        self.cursor_blink_phase = false;
        self.last_cursor_toggle = Instant::now();
        self.last_frame_render = Instant::now();
        self.cursor_drawn = false;
        self.thinking_wrap_width = resolve_thinking_wrap_width();
        self.turn_active = true;
    }

    fn has_streamed_delta(&self) -> bool {
        self.streamed_any_delta
    }

    fn frame_interval(&self) -> Duration {
        self.frame_interval
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
        self.clear_inline_cursor()?;
        self.set_style(LineStyle::Normal);
        if self.colors_enabled {
            print!("\x1b[2m> \x1b[0m");
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
            println!();
        }
        self.set_style(style);
        println!("{header}");
        self.activity_blob_count += 1;
        io::stdout().flush()?;
        Ok(())
    }

    fn print_tool_approval_prompt(&mut self, name: &str, input_preview: &str) -> Result<()> {
        let title = format!("* Tool Execution: {name}");
        self.print_activity_header(LineStyle::Tool, title.as_str())?;

        for (idx, line) in input_preview.lines().enumerate() {
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
                self.thinking_rendered_lines = 0;
            }
            StreamBlock::ToolCall {
                name,
                input,
                status,
                ..
            } => {
                if !is_update {
                    let title = format!("* Tool Execution: {name}");
                    self.print_activity_header(LineStyle::Tool, title.as_str())?;
                } else {
                    self.ensure_newline()?;
                }
                self.render_tool_status(name, input, status)?;
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
                    self.thinking_rendered_lines = 0;
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
        name: &str,
        input: &serde_json::Value,
        status: &ToolStatus,
    ) -> Result<()> {
        self.set_style(LineStyle::Event);
        match status {
            ToolStatus::Pending => {
                println!("  └ Preparing...");
            }
            ToolStatus::WaitingApproval => {
                let preview = structured_tool_input_preview(name, input);
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
            }
            ToolStatus::Executing => {
                println!("  └ Running...");
            }
            ToolStatus::Complete => {
                println!("  └ Done");
            }
            ToolStatus::Cancelled => {
                println!("  └ Cancelled");
            }
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
                let path = input
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<missing>")
                    .to_string();
                let header = format!("* read {path}");
                self.print_activity_header(LineStyle::Tool, &header)?;
                self.render_blob_metadata("content", output)?;
                self.render_blob_numbered_lines(output, None)?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    fn render_blob_metadata(&mut self, label: &str, content: &str) -> Result<()> {
        let line_count = content
            .lines()
            .count()
            .max(usize::from(!content.is_empty()));
        self.set_style(LineStyle::Event);
        println!(
            "    {label}: {} chars, {} lines",
            content.chars().count(),
            line_count
        );
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
            let mut out_lines = Vec::new();
            for segment in wrapped {
                let blob_line_index = self.thinking_rendered_lines % THINKING_MAX_LINES;
                let prefix = thinking_prefix(blob_line_index);
                out_lines.push(format!("{prefix}{segment}"));
                self.thinking_rendered_lines += 1;
            }

            if out_lines.is_empty() {
                return Ok(false);
            }

            self.set_style(LineStyle::Thinking);
            print!("{}", out_lines.join("\n"));
            return Ok(true);
        }

        let trimmed = line.trim_start();

        if trimmed.starts_with("* Event:") || trimmed.starts_with("* Tool:") {
            return Ok(false);
        }

        let output =
            normalize_existing_numbered_snippet_line(line).unwrap_or_else(|| line.to_string());
        if output.is_empty() {
            return Ok(false);
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
                self.thinking_rendered_lines = 0;
            }
        }
    }

    fn update_block_context_for_finished_line(&mut self, line: &str) {
        let trimmed = line.trim_start();
        if trimmed.starts_with("* Thinking") {
            self.active_block = BlockKind::Thinking;
            self.thinking_rendered_lines = 0;
        } else if trimmed.starts_with("* Response") {
            self.active_block = BlockKind::Response;
            self.thinking_rendered_lines = 0;
        } else if self.active_block == BlockKind::Thinking && thinking_inline_text(line).is_some() {
            // Keep tool-call markers folded inside the active thinking block.
        } else if trimmed.starts_with("* Tool") {
            self.active_block = BlockKind::Tool;
            self.thinking_rendered_lines = 0;
        } else if trimmed.starts_with("* Event: message_stop") {
            self.active_block = BlockKind::Normal;
            self.thinking_rendered_lines = 0;
        } else if trimmed.starts_with("* Event:") {
            self.active_block = BlockKind::Event;
            self.thinking_rendered_lines = 0;
        } else if self.active_block == BlockKind::Thinking
            && trimmed.is_empty()
            && !self.in_code_block
        {
            self.thinking_rendered_lines = 0;
        } else if self.active_block == BlockKind::Response
            && trimmed.is_empty()
            && !self.in_code_block
        {
            self.thinking_rendered_lines = 0;
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
                    let (delta_tx, mut delta_rx) =
                        mpsc::unbounded_channel::<ConversationStreamUpdate>();
                    task::spawn(async move {
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
            auto_approve_tools: false,
            suppress_until_turn_complete: false,
            stream_printer: StreamPrinter::new(),
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        while !self.should_quit {
            self.stream_printer.print_prompt()?;

            let Some(raw_input) = read_user_line().await? else {
                break;
            };
            let content = raw_input.trim().to_string();
            if content.is_empty() {
                continue;
            }
            if is_escape_command(content.as_str()) {
                continue;
            }
            if matches!(
                content.as_str(),
                "q" | "quit" | "exit" | "/q" | "/quit" | "/exit"
            ) {
                self.should_quit = true;
                break;
            }

            self.stream_printer.begin_turn();
            let _ = self.message_tx.send(content);
            let mut frame_ticker = tokio::time::interval(self.stream_printer.frame_interval());
            let mut cursor_ticker = tokio::time::interval(CURSOR_BLINK_INTERVAL);

            loop {
                tokio::select! {
                    _ = frame_ticker.tick() => {
                        self.stream_printer.on_frame_tick()?;
                    }
                    _ = cursor_ticker.tick() => {
                        self.stream_printer.on_cursor_tick()?;
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
                                if self.auto_approve_tools {
                                    let _ = request.response_tx.send(true);
                                    continue;
                                }

                                self.stream_printer.flush_buffered_tokens()?;
                                self.stream_printer.print_tool_approval_prompt(
                                    &request.tool_name,
                                    &request.input_preview,
                                )?;
                                let decision = read_tool_confirmation().await?;
                                match decision {
                                    ToolPromptDecision::AcceptOnce => {
                                        let _ = request.response_tx.send(true);
                                    }
                                    ToolPromptDecision::AcceptSession => {
                                        self.auto_approve_tools = true;
                                        self.stream_printer.print_session_auto_approve_notice()?;
                                        let _ = request.response_tx.send(true);
                                    }
                                    ToolPromptDecision::CancelNewTask => {
                                        self.suppress_until_turn_complete = true;
                                        let _ = request.response_tx.send(false);
                                    }
                                }
                                self.stream_printer.set_style(LineStyle::Normal);
                            }
                            Some(UiUpdate::TurnComplete(text)) => {
                                if self.suppress_until_turn_complete {
                                    self.suppress_until_turn_complete = false;
                                    self.stream_printer.end_turn()?;
                                    break;
                                }
                                if !self.stream_printer.has_streamed_delta() && !text.is_empty() {
                                    self.stream_printer.buffer_token(&text)?;
                                }
                                self.stream_printer.end_turn()?;
                                break;
                            }
                            Some(UiUpdate::Error(err)) => {
                                if self.suppress_until_turn_complete {
                                    self.suppress_until_turn_complete = false;
                                    self.stream_printer.end_turn()?;
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
        }

        Ok(())
    }
}

async fn read_user_line() -> Result<Option<String>> {
    task::spawn_blocking(|| -> Result<Option<String>> {
        let mut input = String::new();
        let bytes = io::stdin().read_line(&mut input)?;
        if bytes == 0 {
            Ok(None)
        } else {
            Ok(Some(input))
        }
    })
    .await?
}

async fn read_tool_confirmation() -> Result<ToolPromptDecision> {
    if io::stdin().is_terminal() {
        return task::spawn_blocking(|| -> Result<ToolPromptDecision> {
            enable_raw_mode()?;
            let decision = (|| -> Result<ToolPromptDecision> {
                loop {
                    match event::read()? {
                        Event::Key(event) if event.kind == KeyEventKind::Press => {
                            match event.code {
                                KeyCode::Char('1') => {
                                    print!("1");
                                    println!();
                                    io::stdout().flush()?;
                                    return Ok(ToolPromptDecision::AcceptOnce);
                                }
                                KeyCode::Char('2') => {
                                    print!("2");
                                    println!();
                                    io::stdout().flush()?;
                                    return Ok(ToolPromptDecision::AcceptSession);
                                }
                                KeyCode::Char('3') => {
                                    print!("3");
                                    println!();
                                    io::stdout().flush()?;
                                    return Ok(ToolPromptDecision::CancelNewTask);
                                }
                                KeyCode::Esc => {
                                    print!("esc");
                                    println!();
                                    io::stdout().flush()?;
                                    return Ok(ToolPromptDecision::CancelNewTask);
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                }
            })();
            let _ = disable_raw_mode();
            decision
        })
        .await?;
    }

    loop {
        let Some(raw) = read_user_line().await? else {
            return Ok(ToolPromptDecision::CancelNewTask);
        };
        let trimmed = raw.trim();
        if is_escape_command(trimmed) {
            return Ok(ToolPromptDecision::CancelNewTask);
        }
        if trimmed.starts_with('1') {
            return Ok(ToolPromptDecision::AcceptOnce);
        }
        if trimmed.starts_with('2') {
            return Ok(ToolPromptDecision::AcceptSession);
        }
        if trimmed.starts_with('3') {
            return Ok(ToolPromptDecision::CancelNewTask);
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

fn parse_bool_flag(value: String) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
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
    match name {
        "edit_file" => structured_preview_edit_file_input(input),
        "write_file" => structured_preview_write_file_input(input),
        "read_file" => structured_preview_read_file_input(input),
        "rename_file" => structured_preview_rename_file_input(input),
        _ => serde_json::to_string_pretty(input).unwrap_or_else(|_| input.to_string()),
    }
}

fn structured_preview_edit_file_input(input: &serde_json::Value) -> String {
    let path = input
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or("<missing>");
    let old_str = input.get("old_str").and_then(|v| v.as_str()).unwrap_or("");
    let new_str = input.get("new_str").and_then(|v| v.as_str()).unwrap_or("");

    let mut out = String::new();
    out.push_str(&format!("path: {path}\n"));
    out.push_str(&format!(
        "change: {} chars/{} lines -> {} chars/{} lines\n",
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
    ));
    out.push_str(&format_edit_hunks(
        old_str,
        new_str,
        "    ",
        DEFAULT_EDIT_DIFF_CONTEXT_LINES,
    ));
    out
}

fn structured_preview_write_file_input(input: &serde_json::Value) -> String {
    let path = input
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or("<missing>");
    let content = input.get("content").and_then(|v| v.as_str()).unwrap_or("");

    let mut out = String::new();
    out.push_str(&format!("path: {path}\n"));
    out.push_str(&format!(
        "content: {} chars, {} lines\n",
        content.chars().count(),
        content
            .lines()
            .count()
            .max(usize::from(!content.is_empty()))
    ));
    out.push_str(&structured_preview_lines(content, Some('+')));
    out
}

fn structured_preview_read_file_input(input: &serde_json::Value) -> String {
    let path = input
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or("<missing>");
    format!("path: {path}")
}

fn structured_preview_rename_file_input(input: &serde_json::Value) -> String {
    let old_path = input
        .get("old_path")
        .and_then(|v| v.as_str())
        .unwrap_or("<missing>");
    let new_path = input
        .get("new_path")
        .and_then(|v| v.as_str())
        .unwrap_or("<missing>");
    format!("old_path: {old_path}\nnew_path: {new_path}")
}

fn structured_preview_lines(text: &str, diff_marker: Option<char>) -> String {
    if text.is_empty() {
        return match diff_marker {
            Some(marker) => format!("    1 {marker} <empty>\n"),
            None => "    1   <empty>\n".to_string(),
        };
    }

    let mut out = String::new();
    for (idx, line) in text.lines().enumerate() {
        let line_number = idx + 1;
        match diff_marker {
            Some(marker) => out.push_str(&format!("    {line_number} {marker} {line}\n")),
            None => out.push_str(&format!("    {line_number}   {line}\n")),
        }
    }
    out
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

fn thinking_prefix(line_index: usize) -> &'static str {
    if line_index + 1 >= THINKING_MAX_LINES {
        "  └ "
    } else {
        "  │ "
    }
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
    fn test_thinking_prefix_shape() {
        assert_eq!(thinking_prefix(0), "  │ ");
        assert_eq!(thinking_prefix(1), "  │ ");
        assert_eq!(thinking_prefix(2), "  │ ");
        assert_eq!(thinking_prefix(3), "  └ ");
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
    fn test_structured_preview_lines_empty_content() {
        assert_eq!(structured_preview_lines("", Some('+')), "    1 + <empty>\n");
        assert_eq!(structured_preview_lines("", None), "    1   <empty>\n");
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
}
