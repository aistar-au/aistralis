use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::util::is_local_endpoint_url;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub api_key: Option<String>,
    pub model: String,
    pub api_url: String,
    pub anthropic_version: String,
    pub working_dir: PathBuf,
}

impl Config {
    pub fn load() -> Result<Self> {
        let api_url = std::env::var("ANTHROPIC_API_URL")
            .unwrap_or_else(|_| "https://api.anthropic.com/v1/messages".to_string());
        let api_key = std::env::var("ANTHROPIC_API_KEY").ok().and_then(|v| {
            if v.trim().is_empty() {
                None
            } else {
                Some(v)
            }
        });
        let model = std::env::var("ANTHROPIC_MODEL")
            .unwrap_or_else(|_| "claude-sonnet-4-5-20250929".to_string());
        let anthropic_version =
            std::env::var("ANTHROPIC_VERSION").unwrap_or_else(|_| "2023-06-01".to_string());

        Ok(Self {
            api_key,
            model,
            api_url,
            anthropic_version,
            working_dir: std::env::current_dir()?,
        })
    }

    pub fn validate(&self) -> Result<()> {
        if !self.api_url.starts_with("http://") && !self.api_url.starts_with("https://") {
            bail!(
                "Invalid ANTHROPIC_API_URL '{}': expected http:// or https:// URL",
                self.api_url
            );
        }

        let local_endpoint = self.is_local_endpoint();
        if !local_endpoint && self.api_key.is_none() {
            bail!(
                "ANTHROPIC_API_KEY must be set for non-local endpoints (url: '{}')",
                self.api_url
            );
        }

        if !local_endpoint && self.model.starts_with("local/") {
            bail!("Local models are only allowed for localhost endpoints");
        }

        if !local_endpoint && !self.model.starts_with("claude-") {
            bail!(
                "Invalid model name: '{}'. Expected a model starting with 'claude-'",
                self.model
            );
        }

        Ok(())
    }

    fn is_local_endpoint(&self) -> bool {
        is_local_endpoint_url(&self.api_url)
    }
}
