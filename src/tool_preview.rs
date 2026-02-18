use crate::edit_diff::format_edit_hunks;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolPreviewStyle {
    Compact,
    Structured,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadFileSnapshotSummary {
    FirstRead {
        chars: usize,
        lines: usize,
    },
    Unchanged {
        chars: usize,
        lines: usize,
    },
    Changed {
        before_chars: usize,
        before_lines: usize,
        after_chars: usize,
        after_lines: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadFileSummaryMessageStyle {
    History,
    StreamEvent,
}

#[derive(Debug, Clone, Default)]
pub struct ReadFileSnapshotCache {
    // (content_hash, chars, lines) — hash is u64 from DefaultHasher.
    // DefaultHasher is non-deterministic across process restarts, which is
    // acceptable since this cache is per-process in-memory only.
    entries: HashMap<String, (u64, usize, usize)>,
}

impl ReadFileSnapshotCache {
    pub fn summarize(&mut self, path: &str, content: &str) -> ReadFileSnapshotSummary {
        let (after_chars, after_lines) = content_stats(content);
        let after_hash = hash_content(content);

        match self.entries.get(path).copied() {
            None => {
                self.entries
                    .insert(path.to_string(), (after_hash, after_chars, after_lines));
                ReadFileSnapshotSummary::FirstRead {
                    chars: after_chars,
                    lines: after_lines,
                }
            }
            Some((prev_hash, prev_chars, prev_lines)) if prev_hash == after_hash => {
                ReadFileSnapshotSummary::Unchanged {
                    chars: prev_chars,
                    lines: prev_lines,
                }
            }
            Some((_, before_chars, before_lines)) => {
                self.entries
                    .insert(path.to_string(), (after_hash, after_chars, after_lines));
                ReadFileSnapshotSummary::Changed {
                    before_chars,
                    before_lines,
                    after_chars,
                    after_lines,
                }
            }
        }
    }
}

fn hash_content(content: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    content.hash(&mut h);
    h.finish()
}

pub fn content_stats(content: &str) -> (usize, usize) {
    (
        content.chars().count(),
        content
            .lines()
            .count()
            .max(usize::from(!content.is_empty())),
    )
}

/// Extract the `path` field from a read_file tool input.
/// Returns `None` if the key is absent or not a string — callers supply the fallback.
pub fn read_file_path(input: &Value) -> Option<String> {
    input
        .get("path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

pub fn format_read_file_snapshot_message(
    path: &str,
    summary: ReadFileSnapshotSummary,
    style: ReadFileSummaryMessageStyle,
) -> String {
    match (style, summary) {
        (ReadFileSummaryMessageStyle::History, ReadFileSnapshotSummary::FirstRead { chars, lines }) => format!(
            "Read {path}: {chars} chars, {lines} lines. Full content omitted; use search_files for targeted string matches."
        ),
        (ReadFileSummaryMessageStyle::History, ReadFileSnapshotSummary::Unchanged { chars, lines }) => {
            format!("No changes since last read of {path} ({chars} chars, {lines} lines).")
        }
        (
            ReadFileSummaryMessageStyle::History,
            ReadFileSnapshotSummary::Changed {
                before_chars,
                before_lines,
                after_chars,
                after_lines,
            },
        ) => format!(
            "Read {path}: content changed ({before_chars} chars/{before_lines} lines -> {after_chars} chars/{after_lines} lines). Full content omitted; use search_files for targeted string matches."
        ),
        (
            ReadFileSummaryMessageStyle::StreamEvent,
            ReadFileSnapshotSummary::FirstRead { chars, lines },
        ) => format!("content: {chars} chars, {lines} lines (hidden)"),
        (
            ReadFileSummaryMessageStyle::StreamEvent,
            ReadFileSnapshotSummary::Unchanged { chars, lines },
        ) => format!("no changes since last read ({chars} chars, {lines} lines)"),
        (
            ReadFileSummaryMessageStyle::StreamEvent,
            ReadFileSnapshotSummary::Changed {
                before_chars,
                before_lines,
                after_chars,
                after_lines,
            },
        ) => format!(
            "content changed: {before_chars} chars/{before_lines} lines -> {after_chars} chars/{after_lines} lines"
        ),
    }
}

pub fn preview_lines(
    marker: Option<char>,
    text: &str,
    max_lines: usize,
    start_line: usize,
    indent: &str,
) -> String {
    if text.is_empty() {
        return match marker {
            Some(marker) => format!("{indent}{start_line} {marker} <empty>\n"),
            None => format!("{indent}{start_line}   <empty>\n"),
        };
    }

    let mut out = String::new();
    let lines: Vec<&str> = text.lines().collect();
    for (idx, line) in lines.iter().take(max_lines).enumerate() {
        let line_number = start_line + idx;
        match marker {
            Some(marker) => out.push_str(&format!("{indent}{line_number} {marker} {line}\n")),
            None => out.push_str(&format!("{indent}{line_number}   {line}\n")),
        }
    }
    if lines.len() > max_lines {
        out.push_str(&format!(
            "{indent}... ({} more lines)\n",
            lines.len() - max_lines
        ));
    }
    out
}

pub fn preview_edit_file_input(
    input: &serde_json::Value,
    summary_indent: &str,
    diff_indent: &str,
    diff_context_lines: usize,
) -> String {
    let path = input
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or("<missing>");
    let old_str = input.get("old_str").and_then(|v| v.as_str()).unwrap_or("");
    let new_str = input.get("new_str").and_then(|v| v.as_str()).unwrap_or("");

    let (old_chars, old_lines) = content_stats(old_str);
    let (new_chars, new_lines) = content_stats(new_str);

    let mut out = String::new();
    out.push_str(&format!("path: {path}\n"));
    out.push_str(&format!(
        "{summary_indent}change: {old_chars} chars/{old_lines} lines -> {new_chars} chars/{new_lines} lines\n"
    ));
    out.push_str(&format_edit_hunks(
        old_str,
        new_str,
        diff_indent,
        diff_context_lines,
    ));
    out
}

pub fn preview_write_file_input(
    input: &serde_json::Value,
    line_indent: &str,
    marker: Option<char>,
    max_lines: usize,
) -> String {
    let path = input
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or("<missing>");
    let content = input.get("content").and_then(|v| v.as_str()).unwrap_or("");
    let (chars, lines) = content_stats(content);

    let mut out = String::new();
    out.push_str(&format!("path: {path}\n"));
    out.push_str(&format!("content: {chars} chars, {lines} lines\n"));
    out.push_str(&preview_lines(marker, content, max_lines, 1, line_indent));
    out
}

pub fn preview_tool_input(
    tool_name: &str,
    input: &Value,
    style: ToolPreviewStyle,
    diff_context_lines: usize,
) -> String {
    match (style, tool_name) {
        (ToolPreviewStyle::Compact, "edit_file") => {
            preview_edit_file_input(input, "", "  ", diff_context_lines)
        }
        (ToolPreviewStyle::Structured, "edit_file") => {
            preview_edit_file_input(input, "", "    ", diff_context_lines)
        }
        (ToolPreviewStyle::Compact, "write_file") => {
            preview_write_file_input(input, "  ", Some('+'), usize::MAX)
        }
        (ToolPreviewStyle::Structured, "write_file") => {
            preview_write_file_input(input, "    ", Some('+'), usize::MAX)
        }
        (ToolPreviewStyle::Structured, "read_file") => {
            let path = input
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("<missing>");
            format!("path: {path}")
        }
        (ToolPreviewStyle::Structured, "rename_file") => {
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
        (ToolPreviewStyle::Structured, "list_files" | "list_directory") => {
            let path = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
            let max_entries = input
                .get("max_entries")
                .and_then(|v| v.as_u64())
                .unwrap_or(100);
            format!("path: {path}\nmax_entries: {max_entries}")
        }
        (ToolPreviewStyle::Structured, "search_files" | "search") => {
            let query = input
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("<missing>");
            let max_results = input
                .get("max_results")
                .and_then(|v| v.as_u64())
                .unwrap_or(30);

            let mut out = String::new();
            out.push_str(&format!("query: {query}\n"));
            if let Some(path) = input.get("path").and_then(|v| v.as_str()) {
                out.push_str(&format!("path: {path}\n"));
            }
            out.push_str(&format!("max_results: {max_results}"));
            out
        }
        (ToolPreviewStyle::Structured, _) => {
            if input.as_object().map(|obj| obj.is_empty()).unwrap_or(false) {
                "(no arguments)".to_string()
            } else {
                serde_json::to_string_pretty(input).unwrap_or_else(|_| input.to_string())
            }
        }
        (ToolPreviewStyle::Compact, _) => {
            serde_json::to_string_pretty(input).unwrap_or_else(|_| input.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preview_lines_with_and_without_marker() {
        assert_eq!(preview_lines(Some('+'), "", 10, 1, "  "), "  1 + <empty>\n");
        assert_eq!(
            preview_lines(None, "a\nb", 10, 1, "  "),
            "  1   a\n  2   b\n"
        );
    }

    #[test]
    fn test_snapshot_cache_states() {
        let mut cache = ReadFileSnapshotCache::default();

        let first = cache.summarize("a.rs", "abc");
        assert_eq!(
            first,
            ReadFileSnapshotSummary::FirstRead { chars: 3, lines: 1 }
        );

        let unchanged = cache.summarize("a.rs", "abc");
        assert_eq!(
            unchanged,
            ReadFileSnapshotSummary::Unchanged { chars: 3, lines: 1 }
        );

        let changed = cache.summarize("a.rs", "abcd");
        assert_eq!(
            changed,
            ReadFileSnapshotSummary::Changed {
                before_chars: 3,
                before_lines: 1,
                after_chars: 4,
                after_lines: 1,
            }
        );

        // After a change the cache must update: re-reading the post-change
        // content must yield Unchanged, not a second Changed.
        let after_change_repeat = cache.summarize("a.rs", "abcd");
        assert_eq!(
            after_change_repeat,
            ReadFileSnapshotSummary::Unchanged {
                chars: 4,
                lines: 1
            }
        );
    }

    #[test]
    fn test_read_file_path_extraction() {
        let with_path = serde_json::json!({ "path": "src/app/mod.rs" });
        assert_eq!(
            read_file_path(&with_path),
            Some("src/app/mod.rs".to_string())
        );

        let missing_path = serde_json::json!({ "query": "needle" });
        assert_eq!(read_file_path(&missing_path), None);

        // Non-string value must also return None.
        let non_string = serde_json::json!({ "path": 42 });
        assert_eq!(read_file_path(&non_string), None);

        // Call sites own the fallback wording.
        let fallback = read_file_path(&missing_path).unwrap_or_else(|| "<missing>".to_string());
        assert_eq!(fallback, "<missing>");
    }

    #[test]
    fn test_format_read_file_snapshot_message_styles() {
        let history = format_read_file_snapshot_message(
            "src/app/mod.rs",
            ReadFileSnapshotSummary::Unchanged {
                chars: 10,
                lines: 2,
            },
            ReadFileSummaryMessageStyle::History,
        );
        assert_eq!(
            history,
            "No changes since last read of src/app/mod.rs (10 chars, 2 lines)."
        );

        let stream = format_read_file_snapshot_message(
            "src/app/mod.rs",
            ReadFileSnapshotSummary::Changed {
                before_chars: 9,
                before_lines: 2,
                after_chars: 10,
                after_lines: 2,
            },
            ReadFileSummaryMessageStyle::StreamEvent,
        );
        assert_eq!(
            stream,
            "content changed: 9 chars/2 lines -> 10 chars/2 lines"
        );
    }
}
