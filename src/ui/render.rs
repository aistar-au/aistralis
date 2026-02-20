use crate::ui::input_metrics::{
    char_display_width, cursor_row_col, truncate_to_display_width, wrap_input_lines,
};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

pub enum OverlayModal<'a> {
    CommandConfirm {
        command_preview: &'a str,
    },
    PatchApprove {
        patch_preview: &'a str,
    },
    ToolPermission {
        tool_name: &'a str,
        input_preview: &'a str,
        auto_approve_enabled: bool,
    },
    Error {
        message: &'a str,
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
        OverlayModal::CommandConfirm { command_preview } => (
            "Command Confirm",
            Color::Cyan,
            vec![
                Line::from("Confirm command execution."),
                Line::from(""),
                Line::styled("Command", Style::default().add_modifier(Modifier::BOLD)),
                Line::from(command_preview.to_string()),
            ],
            "y/1 confirm   n/3/esc cancel",
        ),
        OverlayModal::PatchApprove { patch_preview } => (
            "Patch Approve",
            Color::Blue,
            vec![
                Line::from("Review and approve patch application."),
                Line::from(""),
                Line::styled("Patch", Style::default().add_modifier(Modifier::BOLD)),
                Line::from(patch_preview.to_string()),
            ],
            "y/1 approve   n/3/esc reject",
        ),
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
            for line in input_preview.lines().take(6) {
                body.push(Line::from(line.to_string()));
            }
            (
                "Tool Permission",
                Color::Yellow,
                body,
                "y/1 approve once   a/2 approve session   n/3/esc deny",
            )
        }
        OverlayModal::Error { message } => (
            "Error",
            Color::Red,
            vec![
                Line::styled(
                    "An error occurred.",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Line::from(""),
                Line::from(message.to_string()),
            ],
            "enter/esc dismiss",
        ),
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
            OverlayModal::CommandConfirm {
                command_preview: "cargo test --all-targets",
            },
            OverlayModal::PatchApprove {
                patch_preview: "diff --git a/src/app/mod.rs b/src/app/mod.rs",
            },
            OverlayModal::ToolPermission {
                tool_name: "exec_command",
                input_preview: "echo hi",
                auto_approve_enabled: false,
            },
            OverlayModal::Error {
                message: "permission denied",
            },
        ];

        for modal in modals {
            terminal
                .draw(|frame| render_overlay_modal(frame, modal))
                .expect("renderer should support every modal class");
        }
    }
}
