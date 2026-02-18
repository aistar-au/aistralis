use unicode_width::UnicodeWidthChar;

pub fn wrap_input_lines(input: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = vec![String::new()];
    let mut line_widths = vec![0usize];
    for ch in input.chars() {
        if ch == '\r' {
            continue;
        }
        if ch == '\n' {
            lines.push(String::new());
            line_widths.push(0);
            continue;
        }
        let ch_width = char_display_width(ch);
        let current_width = *line_widths.last().unwrap_or(&0);
        if current_width + ch_width > width && current_width > 0 {
            lines.push(String::new());
            line_widths.push(0);
        }
        if let Some(line) = lines.last_mut() {
            line.push(ch);
        }
        if let Some(line_width) = line_widths.last_mut() {
            *line_width += ch_width;
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

pub fn cursor_row_col(input: &str, cursor_byte: usize, width: usize) -> (usize, usize) {
    let width = width.max(1);
    let mut row = 0usize;
    let mut col = 0usize;
    let cursor_byte = clamp_to_char_boundary_left(input, cursor_byte);

    for (idx, ch) in input.char_indices() {
        if idx >= cursor_byte {
            break;
        }
        if ch == '\r' {
            continue;
        }
        if ch == '\n' {
            row += 1;
            col = 0;
            continue;
        }
        let ch_width = char_display_width(ch);
        if col + ch_width > width && col > 0 {
            row += 1;
            col = 0;
        }
        col += ch_width;
    }

    if col >= width {
        row += 1;
        col = 0;
    }

    (row, col)
}

pub fn truncate_to_display_width(text: &str, max_width: usize) -> String {
    let mut out = String::new();
    let mut used = 0usize;
    for ch in text.chars() {
        let ch_width = char_display_width(ch);
        if used + ch_width > max_width && used > 0 {
            break;
        }
        out.push(ch);
        used += ch_width;
    }
    out
}

pub fn char_display_width(ch: char) -> usize {
    UnicodeWidthChar::width(ch).unwrap_or(0)
}

pub fn display_width(text: &str) -> usize {
    text.chars().map(char_display_width).sum()
}

pub fn clamp_to_char_boundary_left(input: &str, cursor: usize) -> usize {
    let mut cursor = cursor.min(input.len());
    while cursor > 0 && !input.is_char_boundary(cursor) {
        cursor -= 1;
    }
    cursor
}
