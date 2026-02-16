use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

pub struct ToolExecutor {
    working_dir: PathBuf,
}

impl ToolExecutor {
    pub fn new(working_dir: PathBuf) -> Self {
        Self { working_dir }
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
        if !normalized.starts_with(&self.working_dir) {
            bail!("Security error: path escapes working directory");
        }

        Ok(normalized)
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

        let new_content = content.replace(old_str, new_str);
        fs::write(resolved, new_content).context("Failed to edit file")
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
}

fn non_empty_trimmed(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
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
}
