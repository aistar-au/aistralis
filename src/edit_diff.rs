#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffKind {
    Equal,
    Delete,
    Insert,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DiffLine {
    kind: DiffKind,
    text: String,
    old_line: Option<usize>,
    new_line: Option<usize>,
}

pub const DEFAULT_EDIT_DIFF_CONTEXT_LINES: usize = 2;

pub fn format_edit_hunks(
    old_str: &str,
    new_str: &str,
    indent: &str,
    context_lines: usize,
) -> String {
    let old_lines = collect_lines(old_str);
    let new_lines = collect_lines(new_str);
    let diff_lines = build_diff_lines(&old_lines, &new_lines);
    let hunks = build_hunk_ranges(&diff_lines, context_lines);

    if hunks.is_empty() {
        if diff_lines.is_empty() {
            return format!("{indent}1   <empty>\n");
        }
        return format!("{indent}... no modified lines ...\n");
    }

    let mut out = String::new();
    for (index, (start, end)) in hunks.iter().copied().enumerate() {
        if index > 0 {
            out.push_str(&format!("{indent}...\n"));
        }

        let hunk_lines = &diff_lines[start..end];
        let old_start = hunk_lines
            .iter()
            .find_map(|line| line.old_line)
            .or_else(|| hunk_lines.iter().find_map(|line| line.new_line))
            .unwrap_or(1);
        let new_start = hunk_lines
            .iter()
            .find_map(|line| line.new_line)
            .or_else(|| hunk_lines.iter().find_map(|line| line.old_line))
            .unwrap_or(1);
        let old_count = hunk_lines
            .iter()
            .filter(|line| line.old_line.is_some())
            .count();
        let new_count = hunk_lines
            .iter()
            .filter(|line| line.new_line.is_some())
            .count();
        out.push_str(&format!(
            "{indent}@@ -{old_start},{old_count} +{new_start},{new_count} @@\n"
        ));

        for line in hunk_lines {
            let marker = match line.kind {
                DiffKind::Equal => ' ',
                DiffKind::Delete => '-',
                DiffKind::Insert => '+',
            };
            let line_number = line.old_line.or(line.new_line).unwrap_or(1);
            let text = if line.text.is_empty() {
                "<empty>"
            } else {
                line.text.as_str()
            };
            out.push_str(&format!("{indent}{line_number} {marker} {text}\n"));
        }
    }

    out
}

fn collect_lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        Vec::new()
    } else {
        text.lines().collect()
    }
}

fn build_diff_lines(old_lines: &[&str], new_lines: &[&str]) -> Vec<DiffLine> {
    let lcs = build_lcs_matrix(old_lines, new_lines);
    let mut out = Vec::with_capacity(old_lines.len() + new_lines.len());

    let mut old_index = 0usize;
    let mut new_index = 0usize;
    let mut old_line = 1usize;
    let mut new_line = 1usize;

    while old_index < old_lines.len() && new_index < new_lines.len() {
        if old_lines[old_index] == new_lines[new_index] {
            out.push(DiffLine {
                kind: DiffKind::Equal,
                text: old_lines[old_index].to_string(),
                old_line: Some(old_line),
                new_line: Some(new_line),
            });
            old_index += 1;
            new_index += 1;
            old_line += 1;
            new_line += 1;
        } else if lcs[old_index + 1][new_index] >= lcs[old_index][new_index + 1] {
            out.push(DiffLine {
                kind: DiffKind::Delete,
                text: old_lines[old_index].to_string(),
                old_line: Some(old_line),
                new_line: None,
            });
            old_index += 1;
            old_line += 1;
        } else {
            out.push(DiffLine {
                kind: DiffKind::Insert,
                text: new_lines[new_index].to_string(),
                old_line: None,
                new_line: Some(new_line),
            });
            new_index += 1;
            new_line += 1;
        }
    }

    while old_index < old_lines.len() {
        out.push(DiffLine {
            kind: DiffKind::Delete,
            text: old_lines[old_index].to_string(),
            old_line: Some(old_line),
            new_line: None,
        });
        old_index += 1;
        old_line += 1;
    }

    while new_index < new_lines.len() {
        out.push(DiffLine {
            kind: DiffKind::Insert,
            text: new_lines[new_index].to_string(),
            old_line: None,
            new_line: Some(new_line),
        });
        new_index += 1;
        new_line += 1;
    }

    out
}

fn build_lcs_matrix(old_lines: &[&str], new_lines: &[&str]) -> Vec<Vec<usize>> {
    let mut lcs = vec![vec![0usize; new_lines.len() + 1]; old_lines.len() + 1];

    for old_index in (0..old_lines.len()).rev() {
        for new_index in (0..new_lines.len()).rev() {
            lcs[old_index][new_index] = if old_lines[old_index] == new_lines[new_index] {
                lcs[old_index + 1][new_index + 1] + 1
            } else {
                lcs[old_index + 1][new_index].max(lcs[old_index][new_index + 1])
            };
        }
    }

    lcs
}

fn build_hunk_ranges(diff_lines: &[DiffLine], context_lines: usize) -> Vec<(usize, usize)> {
    let mut ranges: Vec<(usize, usize)> = Vec::new();

    for (index, line) in diff_lines.iter().enumerate() {
        if line.kind == DiffKind::Equal {
            continue;
        }

        let start = index.saturating_sub(context_lines);
        let end = (index + context_lines + 1).min(diff_lines.len());
        if let Some((_, previous_end)) = ranges.last_mut() {
            if start <= *previous_end {
                *previous_end = (*previous_end).max(end);
                continue;
            }
        }
        ranges.push((start, end));
    }

    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_edit_hunks_uses_compact_context() {
        let old_str = "a\nb\nc\nd\ne\nf";
        let new_str = "a\nb\nc changed\nd\ne\nf";

        let rendered = format_edit_hunks(old_str, new_str, "  ", 1);

        assert!(rendered.contains("@@ -2,3 +2,3 @@"));
        assert!(rendered.contains("  3 - c"));
        assert!(rendered.contains("  3 + c changed"));
        assert!(!rendered.contains("  1   a"));
        assert!(!rendered.contains("  6   f"));
    }

    #[test]
    fn test_format_edit_hunks_adds_gap_between_separate_hunks() {
        let old_str = "a\nb\nc\nd\ne\nf\ng\nh";
        let new_str = "a\nb changed\nc\nd\ne\nf\ng changed\nh";

        let rendered = format_edit_hunks(old_str, new_str, "  ", 1);

        assert!(rendered.matches("@@ ").count() >= 2);
        assert!(rendered.contains("  ..."));
    }

    #[test]
    fn test_format_edit_hunks_handles_empty_insert() {
        let rendered = format_edit_hunks("", "new line", "  ", 2);
        assert!(rendered.contains("@@ -1,0 +1,1 @@"));
        assert!(rendered.contains("  1 + new line"));
    }
}
