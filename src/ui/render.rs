use crate::ui::input_metrics::{
    char_display_width, cursor_row_col, truncate_to_display_width, wrap_input_lines,
};
use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

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
    let window_start = cursor_row.saturating_add(1).saturating_sub(visible_rows);

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

pub fn render_messages(frame: &mut Frame<'_>, area: Rect, messages: &[String], scroll: usize) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let inner = area;

    let body = if messages.is_empty() {
        String::new()
    } else {
        messages.join("\n")
    };

    let paragraph = Paragraph::new(body)
        .style(Style::default().fg(Color::White))
        .wrap(Wrap { trim: false })
        .scroll((scroll as u16, 0));
    frame.render_widget(paragraph, inner);
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

pub fn render_tool_approval_modal(
    frame: &mut Frame<'_>,
    tool_name: &str,
    input_preview: &str,
    auto_approve_enabled: bool,
) {
    let size = frame.area();
    let width = size.width.clamp(44, 96);
    let height = size.height.clamp(10, 16);
    let x = size.x + (size.width.saturating_sub(width)) / 2;
    let y = size.y + (size.height.saturating_sub(height)) / 2;
    let area = Rect::new(x, y, width, height);
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!("Tool Approval: {tool_name}"))
        .style(Style::default().fg(Color::Yellow));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();
    if auto_approve_enabled {
        lines.push(Line::styled(
            "session auto-approve is ON",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ));
    }
    lines.push(Line::from("y/1 approve once   a/2 approve session"));
    lines.push(Line::from("n/3/esc deny and cancel current turn"));
    lines.push(Line::from(""));
    lines.push(Line::styled(
        "Preview",
        Style::default().add_modifier(Modifier::BOLD),
    ));
    for line in input_preview.lines().take(4) {
        lines.push(Line::from(line.to_string()));
    }

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: false }),
        inner,
    );
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
