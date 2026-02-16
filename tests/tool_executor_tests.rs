use aistar::tools::ToolExecutor;
use tempfile::TempDir;

#[test]
fn test_path_traversal_blocked() {
    let temp = TempDir::new().unwrap();
    let executor = ToolExecutor::new(temp.path().to_path_buf());

    assert!(executor.read_file("../../etc/passwd").is_err());
    assert!(executor.read_file("/etc/passwd").is_err());
    assert!(executor.read_file("..\\windows\\system32").is_err());
}

#[test]
fn test_write_new_file() {
    let temp = TempDir::new().unwrap();
    let executor = ToolExecutor::new(temp.path().to_path_buf());

    executor.write_file("new_dir/test.txt", "content").unwrap();

    let content = executor.read_file("new_dir/test.txt").unwrap();
    assert_eq!(content, "content");
}

#[test]
fn test_edit_file_ambiguous() {
    let temp = TempDir::new().unwrap();
    let executor = ToolExecutor::new(temp.path().to_path_buf());

    executor.write_file("test.txt", "foo\nfoo\n").unwrap();

    let result = executor.edit_file("test.txt", "foo", "bar");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("appears 2 times"));
}
