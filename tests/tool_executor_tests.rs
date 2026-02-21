use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;
use vexcoder::tools::ToolExecutor;

#[test]
fn test_path_traversal_blocked() {
    // Integration scope: end-to-end public tool APIs reject traversal attempts.
    let temp = TempDir::new().expect("temp dir");
    let executor = ToolExecutor::new(temp.path().to_path_buf());

    assert!(executor.read_file("../../etc/passwd").is_err());
    assert!(executor.read_file("/etc/passwd").is_err());
    assert!(executor.read_file("..\\windows\\system32").is_err());
    assert!(executor.list_files(Some("../"), 10).is_err());
}

#[test]
fn test_filename_with_double_dots_allowed() {
    let temp = TempDir::new().expect("temp dir");
    let executor = ToolExecutor::new(temp.path().to_path_buf());

    executor
        .write_file("my..file.txt", "content")
        .expect("should allow legitimate '..' filename");

    let content = executor
        .read_file("my..file.txt")
        .expect("read double-dot filename");
    assert_eq!(content, "content");
}

#[test]
fn test_write_new_file() {
    let temp = TempDir::new().expect("temp dir");
    let executor = ToolExecutor::new(temp.path().to_path_buf());

    executor
        .write_file("new_dir/test.txt", "content")
        .expect("write file");

    let content = executor
        .read_file("new_dir/test.txt")
        .expect("read just-written file");
    assert_eq!(content, "content");
}

#[test]
fn test_edit_file_ambiguous() {
    let temp = TempDir::new().expect("temp dir");
    let executor = ToolExecutor::new(temp.path().to_path_buf());

    executor
        .write_file("test.txt", "foo\nfoo\n")
        .expect("seed file");

    let result = executor.edit_file("test.txt", "foo", "bar");
    assert!(result.is_err());
    assert!(result
        .expect_err("should reject ambiguous edits")
        .to_string()
        .contains("appears 2 times"));
}

#[test]
fn test_edit_file_rejects_whole_file_replacement() {
    let temp = TempDir::new().expect("temp dir");
    let executor = ToolExecutor::new(temp.path().to_path_buf());

    let original = "line one\nline two\n";
    executor
        .write_file("test.txt", original)
        .expect("seed file");

    let result = executor.edit_file("test.txt", original, "replaced\n");
    assert!(result.is_err());
    assert!(result
        .expect_err("full-file replacement should be rejected")
        .to_string()
        .contains("refuses full-file replacement"));
}

#[test]
fn test_edit_file_rejects_oversized_snippets() {
    let temp = TempDir::new().expect("temp dir");
    let executor = ToolExecutor::new(temp.path().to_path_buf());

    let mut original = String::new();
    for i in 0..120 {
        original.push_str(&format!("line {i}\n"));
    }
    executor
        .write_file("test.txt", &original)
        .expect("seed file for large edit");

    let result = executor.edit_file("test.txt", &original, "replacement\n");
    assert!(result.is_err());
    assert!(result
        .expect_err("oversized edit snippets should be rejected")
        .to_string()
        .contains("requires focused snippets"));
}

#[cfg(unix)]
#[test]
fn test_symlink_escape_is_blocked_for_file_tools() {
    // Integration scope: file-oriented tool calls must reject symlink escapes.
    use std::os::unix::fs::symlink;

    let workspace = TempDir::new().expect("workspace");
    let outside = TempDir::new().expect("outside");
    let executor = ToolExecutor::new(workspace.path().to_path_buf());

    fs::write(outside.path().join("secret.txt"), "secret\n").expect("seed outside file");
    symlink(
        outside.path().join("secret.txt"),
        workspace.path().join("link.txt"),
    )
    .expect("create file symlink");
    assert!(executor.read_file("link.txt").is_err());
    assert!(executor.edit_file("link.txt", "secret", "public").is_err());

    symlink(outside.path(), workspace.path().join("out")).expect("create dir symlink");
    assert!(executor.write_file("out/pwn.txt", "x").is_err());
    assert!(executor.list_files(Some("out"), 50).is_err());
}

#[test]
fn test_rename_file() {
    let temp = TempDir::new().expect("temp dir");
    let executor = ToolExecutor::new(temp.path().to_path_buf());

    executor
        .write_file("calculator.rs", "fn main() {}\n")
        .expect("seed source file");
    let result = executor
        .rename_file("calculator.rs", "cal.rs")
        .expect("rename file");

    assert!(result.contains("Renamed"));
    assert!(!temp.path().join("calculator.rs").exists());
    assert!(temp.path().join("cal.rs").exists());
}

#[test]
fn test_list_and_search_files() {
    let temp = TempDir::new().expect("temp dir");
    let executor = ToolExecutor::new(temp.path().to_path_buf());

    executor
        .write_file("src/cal.rs", "fn radical(n: f64) -> f64 { n.sqrt() }\n")
        .expect("write test file");
    executor
        .write_file("README.md", "calculator notes\n")
        .expect("write readme");

    let listed = executor
        .list_files(Some("."), 100)
        .expect("list files should succeed");
    assert!(listed.contains("src/"));
    assert!(listed.contains("README.md"));

    let listed_src = executor
        .list_files(Some("src"), 100)
        .expect("list src files should succeed");
    assert!(listed_src.contains("src/cal.rs"));

    let searched = executor
        .search_files("radical", Some("."), 20)
        .expect("search files should succeed");
    assert!(searched.contains("src/cal.rs:1"));
}

#[test]
fn test_search_files_treats_query_as_literal_not_regex() {
    let temp = TempDir::new().expect("temp dir");
    let executor = ToolExecutor::new(temp.path().to_path_buf());

    executor
        .write_file(
            "notes.txt",
            "fn print_dimmed_prompt_with_padding(&mut self) -> Result<()>\n",
        )
        .expect("write notes");

    let searched = executor
        .search_files(
            "fn print_dimmed_prompt_with_padding(&mut self) -> Result<()>",
            Some("."),
            20,
        )
        .expect("literal search should succeed");
    assert!(searched.contains("notes.txt:1"));
}

#[test]
fn test_git_tools_status_diff_add_commit_log_show() {
    let temp = TempDir::new().expect("temp dir");
    init_git_repo(temp.path());
    let executor = ToolExecutor::new(temp.path().to_path_buf());

    fs::write(temp.path().join("note.txt"), "line one\n").expect("write initial file");
    run_git(temp.path(), &["add", "--", "note.txt"]);
    run_git(
        temp.path(),
        &["commit", "-m", "initial commit", "--no-gpg-sign"],
    );

    fs::write(temp.path().join("note.txt"), "line one\nline two\n").expect("modify file");

    let status = executor.git_status(true, None).expect("git status");
    assert!(status.contains("note.txt"));

    let diff = executor
        .git_diff(false, Some("note.txt"))
        .expect("git diff");
    assert!(diff.contains("+line two"));

    let add_result = executor.git_add("note.txt").expect("git add");
    assert!(add_result.contains("Staged"));

    let staged_diff = executor
        .git_diff(true, Some("note.txt"))
        .expect("git diff --cached");
    assert!(staged_diff.contains("+line two"));

    let commit_output = executor.git_commit("update note").expect("git commit");
    assert!(commit_output.contains("update note"));

    let log_output = executor.git_log(5).expect("git log");
    assert!(log_output.contains("update note"));

    let show_output = executor.git_show("HEAD").expect("git show");
    assert!(show_output.contains("update note"));
}

fn init_git_repo(path: &Path) {
    run_git(path, &["init"]);
    run_git(path, &["config", "user.email", "vexcoder@example.com"]);
    run_git(path, &["config", "user.name", "vexcoder test"]);
}

fn run_git(path: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(path)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}
