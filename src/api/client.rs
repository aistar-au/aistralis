use crate::config::Config;
use crate::types::ApiMessage;
use anyhow::Result;
use bytes::Bytes;
use futures::{Stream, StreamExt};
use serde_json::json;
use std::pin::Pin;
#[cfg(test)]
use std::sync::Arc;

pub type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes>> + Send>>;

#[cfg(test)]
pub trait MockStreamProducer: Send + Sync {
    fn create_mock_stream(&self, messages: &[ApiMessage]) -> Result<ByteStream>;
}

#[derive(Clone)]
pub struct ApiClient {
    http: reqwest::Client,
    api_key: Option<String>,
    model: String,
    api_url: String,
    anthropic_version: String,
    #[cfg(test)]
    mock_stream_producer: Option<Arc<dyn MockStreamProducer>>,
}

impl ApiClient {
    pub fn new(config: &Config) -> Result<Self> {
        Ok(Self {
            http: reqwest::Client::new(),
            api_key: config.api_key.clone(),
            model: config.model.clone(),
            api_url: config.api_url.clone(),
            anthropic_version: config.anthropic_version.clone(),
            #[cfg(test)]
            mock_stream_producer: None,
        })
    }

    #[cfg(test)]
    pub fn new_mock(mock_producer: Arc<dyn MockStreamProducer>) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_key: None,
            model: "mock-model".to_string(),
            api_url: "http://localhost:8000/v1/messages".to_string(),
            anthropic_version: "2023-06-01".to_string(),
            mock_stream_producer: Some(mock_producer),
        }
    }

    pub async fn create_stream(&self, messages: &[ApiMessage]) -> Result<ByteStream> {
        #[cfg(test)]
        {
            if let Some(producer) = &self.mock_stream_producer {
                return producer.create_mock_stream(messages);
            }
        }

        let payload = json!({
            "model": self.model,
            "max_tokens": 4096,
            "stream": true,
            "system": "You are a helpful coding assistant that uses tools safely.",
            "messages": messages,
            "tool_choice": { "type": "auto" },
            "tools": tool_definitions(),
        });

        let mut request = self
            .http
            .post(&self.api_url)
            .header("content-type", "application/json")
            .json(&payload);
        if let Some(api_key) = &self.api_key {
            request = request.header("x-api-key", api_key);
        }
        if !self.anthropic_version.trim().is_empty() {
            request = request.header("anthropic-version", &self.anthropic_version);
        }

        let response = request.send().await?.error_for_status()?;

        let stream = response.bytes_stream().map(|item| item.map_err(Into::into));
        Ok(Box::pin(stream))
    }
}

fn tool_definitions() -> serde_json::Value {
    json!([
        {
            "name": "read_file",
            "description": "Read file content",
            "input_schema": {
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"]
            }
        },
        {
            "name": "write_file",
            "description": "Write file content",
            "input_schema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"]
            }
        },
        {
            "name": "edit_file",
            "description": "Edit file content by exact string replacement",
            "input_schema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "old_str": { "type": "string" },
                    "new_str": { "type": "string" }
                },
                "required": ["path", "old_str", "new_str"]
            }
        },
        {
            "name": "git_status",
            "description": "Show git repository status.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "short": { "type": "boolean" },
                    "path": { "type": "string" }
                }
            }
        },
        {
            "name": "git_diff",
            "description": "Show git diff for working tree or staged changes.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "cached": { "type": "boolean" },
                    "path": { "type": "string" }
                }
            }
        },
        {
            "name": "git_log",
            "description": "Show recent git commit history.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "max_count": { "type": "integer", "minimum": 1, "maximum": 100 }
                }
            }
        },
        {
            "name": "git_show",
            "description": "Show details for a git revision.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "revision": { "type": "string" }
                },
                "required": ["revision"]
            }
        },
        {
            "name": "git_add",
            "description": "Stage a file or directory for commit.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "git_commit",
            "description": "Create a commit with the provided message.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "message": { "type": "string" }
                },
                "required": ["message"]
            }
        }
    ])
}
