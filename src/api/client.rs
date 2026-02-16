use crate::config::Config;
use crate::types::ApiMessage;
use anyhow::Result;
use bytes::Bytes;
use futures::{Stream, StreamExt};
use serde_json::json;
use std::pin::Pin;

pub type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes>> + Send>>;

#[derive(Clone)]
pub struct ApiClient {
    http: reqwest::Client,
    api_key: String,
    model: String,
}

impl ApiClient {
    pub fn new(config: &Config) -> Result<Self> {
        Ok(Self {
            http: reqwest::Client::new(),
            api_key: config.api_key.clone(),
            model: config.model.clone(),
        })
    }

    pub async fn create_stream(&self, messages: &[ApiMessage]) -> Result<ByteStream> {
        let payload = json!({
            "model": self.model,
            "max_tokens": 4096,
            "stream": true,
            "messages": messages,
            "tools": [
                {
                    "name": "read_file",
                    "description": "Read file content",
                    "input_schema": {
                        "type": "object",
                        "properties": { "path": {"type": "string"}},
                        "required": ["path"]
                    }
                },
                {
                    "name": "write_file",
                    "description": "Write file content",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "path": {"type": "string"},
                            "content": {"type": "string"}
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
                            "path": {"type": "string"},
                            "old_str": {"type": "string"},
                            "new_str": {"type": "string"}
                        },
                        "required": ["path", "old_str", "new_str"]
                    }
                }
            ]
        });

        let response = self
            .http
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&payload)
            .send()
            .await?
            .error_for_status()?;

        let stream = response.bytes_stream().map(|item| item.map_err(Into::into));
        Ok(Box::pin(stream))
    }
}
