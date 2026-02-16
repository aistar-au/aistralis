use ratatui::{
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

pub fn render_input(frame: &mut Frame<'_>, area: Rect, input: &str, cursor_pos: usize) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Input")
        .style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let paragraph = Paragraph::new(input).style(Style::default().fg(Color::White));
    frame.render_widget(paragraph, inner);

    let cursor_x = inner.x.saturating_add(cursor_pos as u16);
    frame.set_cursor_position((cursor_x, inner.y));
}

pub fn render_messages(frame: &mut Frame<'_>, area: Rect, messages: &[String], scroll: usize) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Conversation")
        .style(Style::default().fg(Color::Green));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let visible_lines: Vec<_> = messages
        .iter()
        .skip(scroll)
        .take(inner.height as usize)
        .map(|msg| ratatui::text::Line::from(msg.clone()))
        .collect();

    let paragraph = Paragraph::new(visible_lines);
    frame.render_widget(paragraph, inner);
}
