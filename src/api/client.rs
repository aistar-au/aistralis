use crate::config::Config;
use crate::runtime::{is_local_endpoint_url, parse_bool_flag};
use crate::types::{ApiMessage, Content, ContentBlock};
use anyhow::Result;
use bytes::Bytes;
use futures::{Stream, StreamExt};
use serde_json::json;
use serde_json::Value;
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
    api_protocol: ApiProtocol,
    structured_tool_protocol: bool,
    #[cfg(test)]
    mock_stream_producer: Option<Arc<dyn MockStreamProducer>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApiProtocol {
    AnthropicMessages,
    OpenAiChatCompletions,
}

impl ApiClient {
    pub fn new(config: &Config) -> Result<Self> {
        let api_protocol = std::env::var("AISTAR_API_PROTOCOL")
            .ok()
            .and_then(parse_protocol)
            .unwrap_or_else(|| infer_api_protocol(&config.api_url));
        let structured_tool_protocol = std::env::var("AISTAR_STRUCTURED_TOOL_PROTOCOL")
            .ok()
            .and_then(parse_bool_flag)
            .unwrap_or(true);

        Ok(Self {
            http: reqwest::Client::new(),
            api_key: config.api_key.clone(),
            model: config.model.clone(),
            api_url: config.api_url.clone(),
            anthropic_version: config.anthropic_version.clone(),
            api_protocol,
            structured_tool_protocol,
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
            api_protocol: ApiProtocol::AnthropicMessages,
            structured_tool_protocol: true,
            mock_stream_producer: Some(mock_producer),
        }
    }

    pub fn supports_structured_tool_protocol(&self) -> bool {
        self.structured_tool_protocol
    }

    pub fn is_local_endpoint(&self) -> bool {
        is_local_endpoint_url(&self.api_url)
    }

    #[cfg(test)]
    pub fn with_structured_tool_protocol(mut self, enabled: bool) -> Self {
        self.structured_tool_protocol = enabled;
        self
    }

    pub async fn create_stream_with_cancel(
        &self,
        messages: &[ApiMessage],
        token: tokio_util::sync::CancellationToken,
    ) -> Result<ByteStream> {
        let _ = token;
        self.create_stream(messages).await
    }

    pub async fn create_stream(&self, messages: &[ApiMessage]) -> Result<ByteStream> {
        #[cfg(test)]
        {
            if let Some(producer) = &self.mock_stream_producer {
                return producer.create_mock_stream(messages);
            }
        }

        let system_prompt = "You are a coding assistant. Use tools for all filesystem facts and changes. Never claim a file was read/written/renamed/searched unless the corresponding tool call succeeded. Prefer search_files for targeted string matches and avoid full-file reads unless required. Use list_files/search_files/read_file before saying a file is missing or present. For edit_file, use a focused old_str snippet around the target change and avoid whole-file replacements; if an entire file rewrite is needed, use write_file instead. Always send non-empty string paths for file tools. Avoid redundant read/search loops: do not call the same tool repeatedly without new evidence.";
        let request_url = self.request_url();
        let max_tokens = resolve_max_tokens(&self.api_url);
        let payload = match self.api_protocol {
            ApiProtocol::AnthropicMessages => {
                let mut payload = json!({
                    "model": self.model,
                    "max_tokens": max_tokens,
                    "stream": true,
                    "system": system_prompt,
                    "messages": messages,
                });
                if self.structured_tool_protocol {
                    let payload_object = payload
                        .as_object_mut()
                        .expect("payload must be a JSON object");
                    payload_object.insert("tool_choice".to_string(), json!({ "type": "auto" }));
                    payload_object.insert("tools".to_string(), tool_definitions());
                }
                payload
            }
            ApiProtocol::OpenAiChatCompletions => {
                let mut payload = json!({
                    "model": self.model,
                    "max_tokens": max_tokens,
                    "stream": true,
                    "messages": openai_messages(messages, system_prompt),
                });
                if self.structured_tool_protocol {
                    let payload_object = payload
                        .as_object_mut()
                        .expect("payload must be a JSON object");
                    payload_object.insert("tool_choice".to_string(), json!("auto"));
                    payload_object.insert("tools".to_string(), tool_definitions_openai());
                }
                payload
            }
        };

        let mut request = self
            .http
            .post(request_url)
            .header("content-type", "application/json")
            .json(&payload);

        if std::env::var("AISTAR_DEBUG_PAYLOAD")
            .ok()
            .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        {
            eprintln!(
                "AISTAR_DEBUG_PAYLOAD request to {}:\n{}",
                self.request_url(),
                serde_json::to_string_pretty(&payload)
                    .unwrap_or_else(|_| "<payload serialization error>".to_string())
            );
        }

        match self.api_protocol {
            ApiProtocol::AnthropicMessages => {
                if let Some(api_key) = &self.api_key {
                    request = request.header("x-api-key", api_key);
                }
                if !self.anthropic_version.trim().is_empty() {
                    request = request.header("anthropic-version", &self.anthropic_version);
                }
            }
            ApiProtocol::OpenAiChatCompletions => {
                if let Some(api_key) = &self.api_key {
                    request = request.header("authorization", format!("Bearer {api_key}"));
                }
            }
        }

        let response = request.send().await?.error_for_status()?;

        let stream = response.bytes_stream().map(|item| item.map_err(Into::into));
        Ok(Box::pin(stream))
    }

    fn request_url(&self) -> String {
        match self.api_protocol {
            ApiProtocol::AnthropicMessages => self.api_url.clone(),
            ApiProtocol::OpenAiChatCompletions => {
                adapt_to_openai_chat_completions_url(&self.api_url)
            }
        }
    }
}

fn resolve_max_tokens(api_url: &str) -> u32 {
    if let Some(value) = std::env::var("AISTAR_MAX_TOKENS")
        .ok()
        .and_then(|v| v.trim().parse::<u32>().ok())
    {
        return value.clamp(128, 8192);
    }

    if is_local_endpoint_url(api_url) {
        1024
    } else {
        4096
    }
}

fn parse_protocol(value: String) -> Option<ApiProtocol> {
    match value.trim().to_ascii_lowercase().as_str() {
        "anthropic" | "anthropic_messages" | "messages" | "v1/messages" => {
            Some(ApiProtocol::AnthropicMessages)
        }
        "openai" | "chat" | "chat_completions" | "openai_chat_completions" => {
            Some(ApiProtocol::OpenAiChatCompletions)
        }
        _ => None,
    }
}

fn infer_api_protocol(api_url: &str) -> ApiProtocol {
    let normalized = api_url.trim().to_ascii_lowercase();
    if normalized.contains("/chat/completions") || normalized.ends_with("/v1") {
        ApiProtocol::OpenAiChatCompletions
    } else {
        ApiProtocol::AnthropicMessages
    }
}

fn adapt_to_openai_chat_completions_url(api_url: &str) -> String {
    let normalized = api_url.trim_end_matches('/');
    if normalized.ends_with("/chat/completions") {
        return normalized.to_string();
    }
    if let Some(prefix) = normalized.strip_suffix("/messages") {
        return format!("{prefix}/chat/completions");
    }
    if normalized.ends_with("/v1") {
        return format!("{normalized}/chat/completions");
    }
    normalized.to_string()
}

fn openai_messages(messages: &[ApiMessage], system_prompt: &str) -> Vec<Value> {
    let mut out = Vec::with_capacity(messages.len() + 1);
    out.push(json!({
        "role": "system",
        "content": system_prompt
    }));

    for message in messages {
        append_openai_message(&mut out, message);
    }

    out
}

fn append_openai_message(out: &mut Vec<Value>, message: &ApiMessage) {
    match (&message.role[..], &message.content) {
        (role, Content::Text(text)) => {
            out.push(json!({
                "role": role,
                "content": text
            }));
        }
        ("assistant", Content::Blocks(blocks)) => {
            let mut content = String::new();
            let mut tool_calls = Vec::new();

            for block in blocks {
                match block {
                    ContentBlock::Text { text } => content.push_str(text),
                    ContentBlock::ToolUse { id, name, input } => {
                        tool_calls.push(json!({
                            "id": id,
                            "type": "function",
                            "function": {
                                "name": name,
                                "arguments": tool_input_to_json_string(input),
                            }
                        }));
                    }
                    ContentBlock::ToolResult { .. } => {}
                }
            }

            let mut assistant_message = serde_json::Map::new();
            assistant_message.insert("role".to_string(), json!("assistant"));
            if content.is_empty() {
                assistant_message.insert("content".to_string(), Value::Null);
            } else {
                assistant_message.insert("content".to_string(), Value::String(content));
            }
            if !tool_calls.is_empty() {
                assistant_message.insert("tool_calls".to_string(), Value::Array(tool_calls));
            }
            out.push(Value::Object(assistant_message));
        }
        (role, Content::Blocks(blocks)) => {
            let mut pushed = false;
            for block in blocks {
                match block {
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        ..
                    } => {
                        out.push(json!({
                            "role": "tool",
                            "tool_call_id": tool_use_id,
                            "content": content
                        }));
                        pushed = true;
                    }
                    ContentBlock::Text { text } => {
                        out.push(json!({
                            "role": role,
                            "content": text
                        }));
                        pushed = true;
                    }
                    ContentBlock::ToolUse { .. } => {}
                }
            }

            if !pushed {
                out.push(json!({
                    "role": role,
                    "content": ""
                }));
            }
        }
    }
}

fn tool_input_to_json_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        _ => serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string()),
    }
}

fn tool_definitions_openai() -> Value {
    let anthropic = tool_definitions();
    let converted = anthropic
        .as_array()
        .map(|tools| {
            tools
                .iter()
                .map(|tool| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": tool.get("name").cloned().unwrap_or_else(|| json!("")),
                            "description": tool.get("description").cloned().unwrap_or_else(|| json!("")),
                            "parameters": tool
                                .get("input_schema")
                                .cloned()
                                .unwrap_or_else(|| json!({ "type": "object" })),
                        }
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Value::Array(converted)
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
            "description": "Edit existing file by replacing one exact, unique snippet (old_str -> new_str). Do not send entire-file replacements via this tool.",
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
            "name": "rename_file",
            "description": "Rename or move a file within the workspace.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "old_path": { "type": "string" },
                    "new_path": { "type": "string" }
                },
                "required": ["old_path", "new_path"]
            }
        },
        {
            "name": "list_files",
            "description": "List files and directories under a path.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "max_entries": { "type": "integer", "minimum": 1, "maximum": 2000 }
                }
            }
        },
        {
            "name": "list_directory",
            "description": "Alias for list_files. List files and directories under a path.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "max_entries": { "type": "integer", "minimum": 1, "maximum": 2000 }
                }
            }
        },
        {
            "name": "search_files",
            "description": "Search text across files and return matching lines.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "path": { "type": "string" },
                    "max_results": { "type": "integer", "minimum": 1, "maximum": 200 }
                },
                "required": ["query"]
            }
        },
        {
            "name": "search",
            "description": "Alias for search_files. Search text across files and return matching lines.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "path": { "type": "string" },
                    "max_results": { "type": "integer", "minimum": 1, "maximum": 200 }
                },
                "required": ["query"]
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn test_protocol_inference_defaults_to_anthropic_messages() {
        let protocol = infer_api_protocol("http://localhost:8000/v1/messages");
        assert_eq!(protocol, ApiProtocol::AnthropicMessages);
    }

    #[test]
    fn test_protocol_inference_detects_openai_chat() {
        let protocol = infer_api_protocol("http://localhost:8000/v1/chat/completions");
        assert_eq!(protocol, ApiProtocol::OpenAiChatCompletions);
    }

    #[test]
    fn test_openai_url_adapter_from_messages_endpoint() {
        let adapted = adapt_to_openai_chat_completions_url("http://localhost:8000/v1/messages");
        assert_eq!(adapted, "http://localhost:8000/v1/chat/completions");
    }

    #[test]
    fn test_openai_url_adapter_from_v1_base_endpoint() {
        let adapted = adapt_to_openai_chat_completions_url("http://localhost:8000/v1");
        assert_eq!(adapted, "http://localhost:8000/v1/chat/completions");
    }

    #[test]
    fn test_resolve_max_tokens_defaults_for_local() {
        let tokens = resolve_max_tokens("http://localhost:8000/v1/messages");
        assert_eq!(tokens, 1024);
    }

    #[test]
    fn test_tool_definitions_cover_execute_tool_dispatch_names() {
        let expected: BTreeSet<&str> = BTreeSet::from([
            "read_file",
            "write_file",
            "edit_file",
            "rename_file",
            "list_files",
            "list_directory",
            "search_files",
            "search",
            "git_status",
            "git_diff",
            "git_log",
            "git_show",
            "git_add",
            "git_commit",
        ]);

        let names: BTreeSet<String> = tool_definitions()
            .as_array()
            .expect("tool definitions must be an array")
            .iter()
            .filter_map(|tool| tool.get("name").and_then(|value| value.as_str()))
            .map(ToOwned::to_owned)
            .collect();

        let expected_owned: BTreeSet<String> = expected.iter().map(|s| s.to_string()).collect();
        assert_eq!(names, expected_owned);
    }

    #[test]
    fn test_openai_tool_definitions_match_base_tool_names() {
        let base_names: BTreeSet<String> = tool_definitions()
            .as_array()
            .expect("tool definitions must be an array")
            .iter()
            .filter_map(|tool| tool.get("name").and_then(|value| value.as_str()))
            .map(ToOwned::to_owned)
            .collect();

        let openai_names: BTreeSet<String> = tool_definitions_openai()
            .as_array()
            .expect("openai tool definitions must be an array")
            .iter()
            .filter_map(|tool| {
                tool.get("function")
                    .and_then(|function| function.get("name"))
                    .and_then(|name| name.as_str())
            })
            .map(ToOwned::to_owned)
            .collect();

        assert_eq!(openai_names, base_names);
    }
}
