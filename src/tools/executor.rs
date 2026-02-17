use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

const MAX_EDIT_SNIPPET_CHARS: usize = 2_000;
const MAX_EDIT_SNIPPET_LINES: usize = 80;

pub struct ToolExecutor {
    working_dir: PathBuf,
    canonical_working_dir: PathBuf,
}

impl ToolExecutor {
    pub fn new(working_dir: PathBuf) -> Self {
        let canonical_working_dir =
            fs::canonicalize(&working_dir).unwrap_or_else(|_| working_dir.clone());
        Self {
            working_dir,
            canonical_working_dir,
        }
    }

    fn resolve_path(&self, path: &str) -> Result<PathBuf> {
        if path.starts_with('/') || path.contains('\\') {
            bail!("Security error: absolute or platform-specific path not allowed: {path}");
        }

        let relative_path = Path::new(path);
        for component in relative_path.components() {
            if matches!(component, Component::ParentDir) {
                bail!("Security error: path traversal detected: {path}");
            }
        }

        let requested = self.working_dir.join(relative_path);
        let normalized = self.normalize_path(&requested);
        self.ensure_path_is_within_workspace(&normalized)?;

        Ok(normalized)
    }

    fn ensure_path_is_within_workspace(&self, path: &Path) -> Result<()> {
        let guard_path = if path.exists() {
            path.to_path_buf()
        } else {
            self.nearest_existing_ancestor(path)
                .context("Security error: could not find an existing parent path")?
                .to_path_buf()
        };

        let canonical_guard = fs::canonicalize(&guard_path)
            .with_context(|| format!("Failed to canonicalize {}", guard_path.display()))?;
        if !canonical_guard.starts_with(&self.canonical_working_dir) {
            bail!(
                "Security error: path escapes working directory via symlink or traversal: {}",
                path.display()
            );
        }
        Ok(())
    }

    fn nearest_existing_ancestor<'a>(&self, path: &'a Path) -> Option<&'a Path> {
        let mut current = path;
        while !current.exists() {
            current = current.parent()?;
        }
        Some(current)
    }

    fn normalize_path(&self, path: &Path) -> PathBuf {
        let mut out = PathBuf::new();
        for component in path.components() {
            match component {
                Component::CurDir => {}
                Component::Normal(seg) => out.push(seg),
                Component::ParentDir => {
                    if out.components().count() > self.working_dir.components().count() {
                        out.pop();
                    }
                }
                Component::RootDir => out.push(component.as_os_str()),
                Component::Prefix(prefix) => out.push(prefix.as_os_str()),
            }
        }
        out
    }

    pub fn read_file(&self, path: &str) -> Result<String> {
        let resolved = self.resolve_path(path)?;
        fs::read_to_string(resolved).context("Failed to read file")
    }

    pub fn write_file(&self, path: &str, content: &str) -> Result<()> {
        let resolved = self.resolve_path(path)?;
        if let Some(parent) = resolved.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(resolved, content).context("Failed to write file")
    }

    pub fn edit_file(&self, path: &str, old_str: &str, new_str: &str) -> Result<()> {
        let resolved = self.resolve_path(path)?;
        let content = fs::read_to_string(&resolved)?;

        if old_str.trim().is_empty() {
            bail!("edit_file requires a non-empty old_str");
        }
        if old_str.chars().count() > MAX_EDIT_SNIPPET_CHARS
            || new_str.chars().count() > MAX_EDIT_SNIPPET_CHARS
            || old_str.lines().count() > MAX_EDIT_SNIPPET_LINES
            || new_str.lines().count() > MAX_EDIT_SNIPPET_LINES
        {
            bail!(
                "edit_file requires focused snippets; old_str/new_str are too large (max {} chars or {} lines each)",
                MAX_EDIT_SNIPPET_CHARS,
                MAX_EDIT_SNIPPET_LINES
            );
        }
        if old_str == content {
            bail!(
                "edit_file refuses full-file replacement; provide a focused old_str snippet instead"
            );
        }

        let occurrences = content.matches(old_str).count();
        if occurrences == 0 {
            bail!("String '{}' not found in file", old_str);
        }
        if occurrences > 1 {
            bail!(
                "String '{}' appears {} times; must be unique",
                old_str,
                occurrences
            );
        }

        let new_content = content.replacen(old_str, new_str, 1);
        fs::write(resolved, new_content).context("Failed to edit file")
    }

    pub fn rename_file(&self, old_path: &str, new_path: &str) -> Result<String> {
        let from = self.resolve_path(old_path)?;
        let to = self.resolve_path(new_path)?;

        if !from.exists() {
            bail!(
                "Failed to rename file: source '{}' does not exist",
                old_path
            );
        }
        if from == to {
            return Ok(format!("Source and target are the same: {old_path}"));
        }

        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).context("Failed to create destination directory")?;
        }
        fs::rename(&from, &to).context("Failed to rename file")?;
        Ok(format!("Renamed {} -> {}", old_path, new_path))
    }

    pub fn list_files(&self, path: Option<&str>, max_entries: usize) -> Result<String> {
        let root = self.resolve_optional_path(path)?;
        let limit = max_entries.clamp(1, 2000);
        let mut entries = Vec::new();

        if root.is_file() {
            entries.push(self.to_workspace_relative_display(&root));
        } else {
            let mut children: Vec<_> = fs::read_dir(&root)
                .with_context(|| format!("Failed to read directory {}", root.display()))?
                .collect::<std::result::Result<Vec<_>, _>>()
                .with_context(|| format!("Failed to list entries in {}", root.display()))?;
            children.sort_by_key(|entry| entry.path());

            for child in children {
                let name = child.file_name();
                let name = name.to_string_lossy();
                if should_skip_list_entry(root.as_path(), self.working_dir.as_path(), &name) {
                    continue;
                }

                let path = child.path();
                let is_dir = child
                    .file_type()
                    .with_context(|| format!("Failed to inspect {}", path.display()))?
                    .is_dir();
                let mut display = self.to_workspace_relative_display(&path);
                if is_dir {
                    display.push('/');
                }
                entries.push(display);
                if entries.len() >= limit {
                    break;
                }
            }
        }

        if entries.is_empty() {
            Ok("(no files found)".to_string())
        } else {
            Ok(entries.join("\n"))
        }
    }

    pub fn search_files(
        &self,
        query: &str,
        path: Option<&str>,
        max_results: usize,
    ) -> Result<String> {
        let query =
            non_empty_trimmed(query).context("search_files requires a non-empty 'query' field")?;
        let root = self.resolve_optional_path(path)?;
        let max_results = max_results.clamp(1, 200);

        match self.search_with_rg(query, &root, max_results) {
            Ok(result) => Ok(result),
            Err(error) => {
                if error.to_string().contains("Failed to execute rg command") {
                    self.search_fallback(query, &root, max_results)
                } else {
                    Err(error)
                }
            }
        }
    }

    pub fn git_status(&self, short: bool, path: Option<&str>) -> Result<String> {
        let mut args = vec!["status".to_string()];
        if short {
            args.push("--short".to_string());
        }
        if let Some(pathspec) = path.and_then(non_empty_trimmed) {
            args.push("--".to_string());
            args.push(self.sanitize_git_pathspec(pathspec)?);
        }
        self.run_git(args)
    }

    pub fn git_diff(&self, cached: bool, path: Option<&str>) -> Result<String> {
        let mut args = vec!["diff".to_string()];
        if cached {
            args.push("--cached".to_string());
        }
        if let Some(pathspec) = path.and_then(non_empty_trimmed) {
            args.push("--".to_string());
            args.push(self.sanitize_git_pathspec(pathspec)?);
        }
        self.run_git(args)
    }

    pub fn git_log(&self, max_count: usize) -> Result<String> {
        let count = max_count.clamp(1, 100);
        self.run_git(vec![
            "log".to_string(),
            "--oneline".to_string(),
            format!("-n{count}"),
        ])
    }

    pub fn git_show(&self, revision: &str) -> Result<String> {
        let revision = non_empty_trimmed(revision)
            .context("git_show requires a non-empty 'revision' field")?;
        self.run_git(vec![
            "show".to_string(),
            "--stat".to_string(),
            "--oneline".to_string(),
            revision.to_string(),
        ])
    }

    pub fn git_add(&self, path: &str) -> Result<String> {
        let pathspec = self.sanitize_git_pathspec(path)?;
        self.run_git(vec!["add".to_string(), "--".to_string(), pathspec])?;
        Ok(format!("Staged {path}"))
    }

    pub fn git_commit(&self, message: &str) -> Result<String> {
        let message = non_empty_trimmed(message)
            .context("git_commit requires a non-empty 'message' field")?;
        self.run_git(vec![
            "commit".to_string(),
            "-m".to_string(),
            message.to_string(),
            "--no-gpg-sign".to_string(),
        ])
    }

    fn sanitize_git_pathspec(&self, path: &str) -> Result<String> {
        let path = non_empty_trimmed(path).context("Path cannot be empty")?;
        if path == "." {
            return Ok(path.to_string());
        }
        let resolved = self.resolve_path(path)?;
        let relative = resolved
            .strip_prefix(&self.working_dir)
            .context("Path escapes working directory")?;
        Ok(relative.to_string_lossy().to_string())
    }

    fn run_git(&self, args: Vec<String>) -> Result<String> {
        let output = Command::new("git")
            .current_dir(&self.working_dir)
            .args(&args)
            .output()
            .context("Failed to execute git command")?;

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

        if !output.status.success() {
            let details = if stderr.is_empty() { stdout } else { stderr };
            bail!("git {} failed: {}", args.join(" "), details);
        }

        if stdout.is_empty() {
            Ok("OK".to_string())
        } else {
            Ok(stdout)
        }
    }

    fn resolve_optional_path(&self, path: Option<&str>) -> Result<PathBuf> {
        match path.and_then(non_empty_trimmed) {
            None => Ok(self.working_dir.clone()),
            Some(".") => Ok(self.working_dir.clone()),
            Some(value) => self.resolve_path(value),
        }
    }

    fn to_workspace_relative_display(&self, path: &Path) -> String {
        path.strip_prefix(&self.working_dir)
            .map(|relative| relative.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.to_string_lossy().to_string())
    }

    fn search_with_rg(&self, query: &str, root: &Path, max_results: usize) -> Result<String> {
        let mut search_path = self.to_workspace_relative_display(root);
        if search_path.is_empty() {
            search_path = ".".to_string();
        }
        let output = Command::new("rg")
            .current_dir(&self.working_dir)
            .arg("--line-number")
            .arg("--color")
            .arg("never")
            .arg("--smart-case")
            .arg("--max-count")
            .arg(max_results.to_string())
            .arg("--")
            .arg(query)
            .arg(search_path)
            .output()
            .context("Failed to execute rg command")?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if stdout.is_empty() {
                Ok("No matches found.".to_string())
            } else {
                Ok(stdout)
            }
        } else if output.status.code() == Some(1) {
            Ok("No matches found.".to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            bail!("search_files failed: {}", stderr);
        }
    }

    fn search_fallback(&self, query: &str, root: &Path, max_results: usize) -> Result<String> {
        let mut results = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        let case_sensitive = query.chars().any(char::is_uppercase);
        let lowered_query = query.to_lowercase();

        while let Some(path) = stack.pop() {
            if path.is_dir() {
                let mut children: Vec<_> = fs::read_dir(&path)
                    .with_context(|| format!("Failed to read directory {}", path.display()))?
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .with_context(|| format!("Failed to list entries in {}", path.display()))?;
                children.sort_by_key(|entry| entry.path());
                for child in children {
                    stack.push(child.path());
                }
                continue;
            }

            let Ok(content) = fs::read_to_string(&path) else {
                continue;
            };

            for (idx, line) in content.lines().enumerate() {
                let is_match = if case_sensitive {
                    line.contains(query)
                } else {
                    line.to_lowercase().contains(&lowered_query)
                };
                if is_match {
                    results.push(format!(
                        "{}:{}:{}",
                        self.to_workspace_relative_display(&path),
                        idx + 1,
                        line
                    ));
                    if results.len() >= max_results {
                        break;
                    }
                }
            }
            if results.len() >= max_results {
                break;
            }
        }

        if results.is_empty() {
            Ok("No matches found.".to_string())
        } else {
            Ok(results.join("\n"))
        }
    }
}

fn non_empty_trimmed(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn should_skip_list_entry(root: &Path, working_dir: &Path, name: &str) -> bool {
    if name.starts_with('.') {
        return true;
    }

    if root != working_dir {
        return false;
    }

    matches!(
        name,
        "target" | "node_modules" | "__pycache__" | ".venv" | "venv" | "build" | "dist"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use tempfile::TempDir;

    #[test]
    fn test_path_traversal_blocked() {
        let temp = TempDir::new().expect("temp dir");
        let executor = ToolExecutor::new(temp.path().to_path_buf());

        assert!(executor.resolve_path("../../etc/passwd").is_err());
        assert!(executor.resolve_path("/etc/passwd").is_err());
        assert!(executor.resolve_path("..\\windows\\system32").is_err());
    }

    #[test]
    fn test_filename_with_double_dots_allowed() {
        let temp = TempDir::new().expect("temp dir");
        let executor = ToolExecutor::new(temp.path().to_path_buf());

        assert!(executor.resolve_path("my..file.txt").is_ok());
        assert!(executor.resolve_path("v..2.0.md").is_ok());
    }

    #[test]
    fn test_path_traversal_prevention() {
        let workspace = env::current_dir().unwrap();
        let executor = ToolExecutor::new(workspace.clone());

        // This should return an Err, not a path to your root/etc
        let result = executor.resolve_path("../../etc/passwd");
        assert!(result.is_err(), "Security breach: Path traversal allowed!");
    }

    #[test]
    fn test_list_files_path_traversal_blocked() {
        let temp = TempDir::new().expect("temp dir");
        let executor = ToolExecutor::new(temp.path().to_path_buf());

        let result = executor.list_files(Some("../"), 10);
        assert!(result.is_err(), "Path traversal should be rejected");
    }
}
