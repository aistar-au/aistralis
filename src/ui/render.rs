use crate::ui::input_metrics::{
    char_display_width, cursor_row_col, truncate_to_display_width, wrap_input_lines,
};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

pub enum OverlayModal<'a> {
    PatchApprove {
        patch_preview: &'a str,
        scroll_offset: usize,
        viewport_rows: usize,
    },
    ToolPermission {
        tool_name: &'a str,
        input_preview: &'a str,
        auto_approve_enabled: bool,
    },
}

pub fn input_visual_rows(input: &str, width: usize) -> usize {
    wrap_input_lines(input, width).len().max(1)
}

pub fn render_input(frame: &mut Frame<'_>, area: Rect, input: &str, cursor_byte: usize) {
    if area.height == 0 || area.width <= 2 {
        return;
    }
    let inner = area;

    let input_width = inner.width.saturating_sub(2).max(1) as usize;
    let lines = wrap_input_lines(input, input_width);
    let (cursor_row, cursor_col) = cursor_row_col(input, cursor_byte, input_width);
    let visible_rows = inner.height as usize;
    let window_start = input_window_start(cursor_row, visible_rows);

    let mut rendered = Vec::with_capacity(visible_rows);
    for offset in 0..visible_rows {
        let row_index = window_start + offset;
        let prefix = if row_index == 0 { "> " } else { "  " };
        let line = lines.get(row_index).cloned().unwrap_or_default();
        rendered.push(Line::from(format!("{prefix}{line}")));
    }

    frame.render_widget(
        Paragraph::new(rendered)
            .style(
                Style::default()
                    .fg(Color::Gray)
                    .bg(Color::Rgb(24, 24, 24))
                    .add_modifier(Modifier::DIM),
            )
            .wrap(Wrap { trim: false }),
        inner,
    );

    let cursor_y = inner
        .y
        .saturating_add(cursor_row.saturating_sub(window_start) as u16);
    let cursor_x = inner
        .x
        .saturating_add(2 + cursor_col as u16)
        .min(inner.x.saturating_add(inner.width.saturating_sub(1)));
    frame.set_cursor_position((cursor_x, cursor_y));
}

fn input_window_start(cursor_row: usize, visible_rows: usize) -> usize {
    cursor_row
        .saturating_add(1)
        .saturating_sub(visible_rows.max(1))
}

pub fn render_messages(frame: &mut Frame<'_>, area: Rect, messages: &[String], scroll: usize) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let inner = area;

    let logical_rows = expand_history_rows(messages);
    let line_number_width = logical_rows.len().max(1).to_string().len();
    let content_width = history_content_width(inner.width, line_number_width);
    let mut body: Vec<Line<'static>> = Vec::new();
    for (index, row) in logical_rows.iter().enumerate() {
        let row_style = history_row_style(row);
        let wrapped_segments = wrap_input_lines(row, content_width);
        for (segment_index, segment) in wrapped_segments.iter().enumerate() {
            body.push(format_history_row_segment(
                index + 1,
                line_number_width,
                segment,
                row_style,
                segment_index == 0,
            ));
        }
    }

    let paragraph =
        Paragraph::new(Text::from(body)).scroll((scroll.min(u16::MAX as usize) as u16, 0));
    frame.render_widget(paragraph, inner);
}

pub fn history_visual_line_count(messages: &[String], content_width: usize) -> usize {
    if messages.is_empty() {
        return 0;
    }

    let content_width = content_width.max(1);
    expand_history_rows(messages)
        .iter()
        .map(|row| wrap_input_lines(row, content_width).len().max(1))
        .sum()
}

pub fn history_content_width_for_area(messages: &[String], area: Rect) -> usize {
    let row_count = expand_history_rows(messages).len().max(1);
    let line_number_width = row_count.to_string().len();
    history_content_width(area.width, line_number_width)
}

fn history_content_width(area_width: u16, line_number_width: usize) -> usize {
    area_width
        .saturating_sub((line_number_width + 3) as u16)
        .max(1) as usize
}

fn expand_history_rows(messages: &[String]) -> Vec<String> {
    if messages.is_empty() {
        return Vec::new();
    }

    let mut rows = Vec::new();
    for message in messages {
        if message.is_empty() {
            rows.push(String::new());
            continue;
        }
        rows.extend(message.split('\n').map(ToOwned::to_owned));
    }
    rows
}

fn format_history_row_segment(
    line_number: usize,
    line_number_width: usize,
    row: &str,
    style: Style,
    show_line_number: bool,
) -> Line<'static> {
    let line_prefix = if show_line_number {
        format!("{line_number:>line_number_width$} | ")
    } else {
        format!("{:>line_number_width$} | ", "")
    };
    Line::from(vec![
        Span::styled(
            line_prefix,
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM),
        ),
        Span::styled(row.to_string(), style),
    ])
}

fn history_row_style(row: &str) -> Style {
    if row.starts_with('+') && !row.starts_with("+++") {
        Style::default().fg(Color::Green)
    } else if row.starts_with('-') && !row.starts_with("---") {
        Style::default().fg(Color::Red)
    } else if row.starts_with("@@") || row.starts_with("diff --git") || row.starts_with("index ") {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::White)
    }
}

pub fn render_status_line(frame: &mut Frame<'_>, area: Rect, status: &str) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    let text = truncate_line(status, area.width as usize);
    frame.render_widget(
        Paragraph::new(text).style(Style::default().fg(Color::DarkGray)),
        area,
    );
}

pub fn render_overlay_modal(frame: &mut Frame<'_>, modal: OverlayModal<'_>) {
    if frame.area().width == 0 || frame.area().height == 0 {
        return;
    }

    let (title, accent, body, shortcuts) = modal_content(modal);
    let preferred_height = (body.len() + 8) as u16;
    let area = centered_modal_area(frame.area(), preferred_height);
    frame.render_widget(Clear, area);

    let outer = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .style(Style::default().fg(accent));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(inner);
    let body_area = vertical[0];
    let shortcuts_area = vertical[1];

    let body_block = Block::default().borders(Borders::ALL).title("Body");
    let body_inner = body_block.inner(body_area);
    frame.render_widget(body_block, body_area);

    frame.render_widget(
        Paragraph::new(Text::from(body))
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: false }),
        body_inner,
    );

    frame.render_widget(
        Paragraph::new(shortcuts)
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::DarkGray)),
        shortcuts_area,
    );
}

fn modal_content(
    modal: OverlayModal<'_>,
) -> (&'static str, Color, Vec<Line<'static>>, &'static str) {
    match modal {
        OverlayModal::PatchApprove {
            patch_preview,
            scroll_offset,
            viewport_rows,
        } => {
            let lines: Vec<&str> = patch_preview.lines().collect();
            let start = scroll_offset.min(lines.len().saturating_sub(1));
            let visible = viewport_rows.max(1);
            let end = (start + visible).min(lines.len());

            let mut body = Vec::new();
            body.push(Line::from("Review and approve patch application."));
            body.push(Line::from(format!(
                "showing {}-{} of {}",
                if lines.is_empty() { 0 } else { start + 1 },
                end,
                lines.len()
            )));
            body.push(Line::from(""));
            body.push(Line::styled(
                "Patch",
                Style::default().add_modifier(Modifier::BOLD),
            ));
            for line in lines.iter().skip(start).take(visible) {
                body.push(styled_diff_line(line));
            }

            (
                "Patch Approve",
                Color::Blue,
                body,
                "y/1 approve   n/3/esc reject   up/down/pgup/pgdn/home/end scroll",
            )
        }
        OverlayModal::ToolPermission {
            tool_name,
            input_preview,
            auto_approve_enabled,
        } => {
            let mut body = Vec::new();
            body.push(Line::styled(
                format!("Tool: {tool_name}"),
                Style::default().add_modifier(Modifier::BOLD),
            ));
            if auto_approve_enabled {
                body.push(Line::styled(
                    "session auto-approve is ON",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ));
            }
            body.push(Line::from(""));
            body.push(Line::styled(
                "Preview",
                Style::default().add_modifier(Modifier::BOLD),
            ));
            let preview_lines: Vec<&str> = input_preview.lines().collect();
            let max_preview_lines = 6;
            for line in preview_lines.iter().take(max_preview_lines) {
                body.push(Line::from(line.to_string()));
            }
            if preview_lines.len() > max_preview_lines {
                body.push(Line::styled(
                    format!(
                        "... ({} more lines)",
                        preview_lines.len() - max_preview_lines
                    ),
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::DIM),
                ));
            }
            (
                "Tool Permission",
                Color::Yellow,
                body,
                "1 yes   2 allow this session   3/esc cancel",
            )
        }
    }
}

fn styled_diff_line(line: &str) -> Line<'static> {
    if line.starts_with('+') && !line.starts_with("+++") {
        Line::styled(line.to_string(), Style::default().fg(Color::Green))
    } else if line.starts_with('-') && !line.starts_with("---") {
        Line::styled(line.to_string(), Style::default().fg(Color::Red))
    } else if line.starts_with("@@") {
        Line::styled(line.to_string(), Style::default().fg(Color::Cyan))
    } else {
        Line::styled(line.to_string(), Style::default().fg(Color::Gray))
    }
}

fn centered_modal_area(size: Rect, preferred_height: u16) -> Rect {
    let width = size.width.clamp(44, 96);
    let max_height = size.height.clamp(8, 24);
    let height = preferred_height.clamp(8, max_height);
    let x = size.x + (size.width.saturating_sub(width)) / 2;
    let y = size.y + (size.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}

fn truncate_line(input: &str, width: usize) -> String {
    let width = width.max(1);
    let mut out = String::new();
    let mut used = 0usize;
    let mut truncated = false;

    for ch in input.chars() {
        let ch_width = char_display_width(ch);
        if used + ch_width > width {
            truncated = true;
            break;
        }
        out.push(ch);
        used += ch_width;
    }

    if truncated && width >= 4 {
        out = truncate_to_display_width(&out, width - 3);
        out.push_str("...");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn all_modals_use_unified_renderer() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).expect("test terminal");

        let modals = [
            OverlayModal::PatchApprove {
                patch_preview: "diff --git a/src/app/mod.rs b/src/app/mod.rs",
                scroll_offset: 0,
                viewport_rows: 8,
            },
            OverlayModal::ToolPermission {
                tool_name: "exec_command",
                input_preview: "echo hi",
                auto_approve_enabled: false,
            },
        ];

        for modal in modals {
            terminal
                .draw(|frame| render_overlay_modal(frame, modal))
                .expect("renderer should support every modal class");
        }
    }

    #[test]
    fn input_window_start_scrolls_once_cursor_exceeds_visible_rows() {
        assert_eq!(input_window_start(0, 4), 0);
        assert_eq!(input_window_start(3, 4), 0);
        assert_eq!(input_window_start(4, 4), 1);
        assert_eq!(input_window_start(7, 4), 4);
    }

    #[test]
    fn diff_line_semantics_are_styled_by_prefix() {
        let add = styled_diff_line("+added");
        let del = styled_diff_line("-removed");
        let hunk = styled_diff_line("@@ -1 +1 @@");
        let ctx = styled_diff_line(" context");

        assert_eq!(add.style.fg, Some(Color::Green));
        assert_eq!(del.style.fg, Some(Color::Red));
        assert_eq!(hunk.style.fg, Some(Color::Cyan));
        assert_eq!(ctx.style.fg, Some(Color::Gray));
    }

    #[test]
    fn history_visual_line_count_tracks_embedded_newlines() {
        let messages = vec![
            "first".to_string(),
            "line-a\nline-b".to_string(),
            String::new(),
        ];
        assert_eq!(history_visual_line_count(&messages, 80), 4);
    }

    #[test]
    fn history_visual_line_count_tracks_wrapped_rows() {
        let messages = vec!["123456".to_string()];
        assert_eq!(history_visual_line_count(&messages, 3), 2);
    }

    #[test]
    fn history_row_style_marks_diff_rows() {
        assert_eq!(history_row_style("+add").fg, Some(Color::Green));
        assert_eq!(history_row_style("-del").fg, Some(Color::Red));
        assert_eq!(history_row_style("@@ -1 +1 @@").fg, Some(Color::Cyan));
        assert_eq!(history_row_style("plain text").fg, Some(Color::White));
    }
}
