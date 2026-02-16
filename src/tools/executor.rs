use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Component, Path, PathBuf};

pub struct ToolExecutor {
    working_dir: PathBuf,
}

impl ToolExecutor {
    pub fn new(working_dir: PathBuf) -> Self {
        Self { working_dir }
    }

    fn resolve_path(&self, path: &str) -> Result<PathBuf> {
        if path.contains("..") || path.starts_with('/') || path.contains('\\') {
            bail!("Security error: path traversal detected: {}", path);
        }

        let requested = self.working_dir.join(path);
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_path_traversal_blocked() {
        let temp = TempDir::new().unwrap();
        let executor = ToolExecutor::new(temp.path().to_path_buf());

        assert!(executor.resolve_path("../../etc/passwd").is_err());
        assert!(executor.resolve_path("/etc/passwd").is_err());
        assert!(executor.resolve_path("..\\windows\\system32").is_err());
    }
}
