use aistar::tools::ToolExecutor;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

#[test]
fn test_path_traversal_blocked() {
    let temp = TempDir::new().expect("temp dir");
    let executor = ToolExecutor::new(temp.path().to_path_buf());

    assert!(executor.read_file("../../etc/passwd").is_err());
    assert!(executor.read_file("/etc/passwd").is_err());
    assert!(executor.read_file("..\\windows\\system32").is_err());
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
    run_git(path, &["config", "user.email", "aistar@example.com"]);
    run_git(path, &["config", "user.name", "aistar test"]);
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
