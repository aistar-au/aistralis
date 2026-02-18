use crate::types::{ContentBlock, Delta, StreamEvent};
use anyhow::Result;
use serde::Deserialize;

#[derive(Default)]
pub struct StreamParser {
    buffer: String,
    openai_tools: Vec<OpenAiToolState>,
}

#[derive(Default, Clone)]
struct OpenAiToolState {
    id: String,
    name: String,
    pending_arguments: String,
    started: bool,
    stopped: bool,
}

#[derive(Debug, Deserialize)]
struct OpenAiChunk {
    #[serde(default)]
    choices: Vec<OpenAiChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    #[serde(default)]
    delta: OpenAiDelta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct OpenAiDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAiToolCallDelta>>,
}

#[derive(Debug, Default, Deserialize)]
struct OpenAiToolCallDelta {
    #[serde(default)]
    index: Option<usize>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<OpenAiFunctionDelta>,
}

#[derive(Debug, Default, Deserialize)]
struct OpenAiFunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

impl StreamParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn process(&mut self, chunk: &[u8]) -> Result<Vec<StreamEvent>> {
        self.buffer.push_str(&String::from_utf8_lossy(chunk));
        if self.buffer.contains('\r') {
            self.buffer = self.buffer.replace("\r\n", "\n");
        }

        let mut events = Vec::new();
        let mut start = 0;

        while let Some(end) = self.buffer[start..].find("\n\n") {
            let event_end = start + end + 2;
            let event_text = &self.buffer[start..start + end];

            let mut event_type = None;
            let mut data_lines = Vec::new();

            for line in event_text.lines() {
                if line.is_empty() || line.starts_with(':') {
                    continue;
                }
                if let Some(rest) = line.strip_prefix("event:") {
                    event_type = Some(rest.trim().to_string());
                } else if let Some(rest) = line.strip_prefix("data:") {
                    data_lines.push(rest.trim_start().to_string());
                }
            }

            if !data_lines.is_empty() {
                let json_data = data_lines.join("\n");
                let should_parse = if json_data == "[DONE]" {
                    true
                } else {
                    event_type.as_deref().is_none_or(|ty| ty != "ping")
                };

                if should_parse {
                    match serde_json::from_str::<StreamEvent>(&json_data) {
                        Ok(evt) => events.push(evt),
                        Err(anthropic_error) => {
                            if let Some(openai_events) = self.parse_openai_chunk(&json_data) {
                                events.extend(openai_events);
                            } else {
                                eprintln!("⚠️  SSE parse error: {}", anthropic_error);
                                eprintln!(
                                    "   Event type: {}",
                                    event_type.as_deref().unwrap_or("<none>")
                                );
                                eprintln!("   Data: {}", json_data);
                            }
                        }
                    }
                }
            }

            start = event_end;
        }

        if start > 0 {
            self.buffer.drain(..start);
        }

        Ok(events)
    }

    fn parse_openai_chunk(&mut self, json_data: &str) -> Option<Vec<StreamEvent>> {
        if json_data == "[DONE]" {
            let mut events = Vec::new();
            self.close_openai_tool_blocks(&mut events);
            return Some(events);
        }

        let chunk = serde_json::from_str::<OpenAiChunk>(json_data).ok()?;
        if chunk.choices.is_empty() {
            return Some(Vec::new());
        }

        let mut events = Vec::new();
        for choice in chunk.choices {
            if let Some(content) = choice.delta.content {
                events.push(StreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: Delta {
                        delta_type: Some("text_delta".to_string()),
                        text: Some(content),
                        partial_json: None,
                    },
                });
            }

            if let Some(tool_calls) = choice.delta.tool_calls {
                for tool_call in tool_calls {
                    self.apply_openai_tool_delta(tool_call, &mut events);
                }
            }

            if choice.finish_reason.is_some() {
                self.close_openai_tool_blocks(&mut events);
            }
        }

        Some(events)
    }

    fn apply_openai_tool_delta(
        &mut self,
        tool_call: OpenAiToolCallDelta,
        events: &mut Vec<StreamEvent>,
    ) {
        let block_index = tool_call.index.unwrap_or(0) + 1;
        self.ensure_openai_tool_state(block_index);
        let state = &mut self.openai_tools[block_index];

        if let Some(id) = tool_call.id {
            if !id.is_empty() {
                state.id = id;
            }
        }
        if let Some(function) = tool_call.function {
            if let Some(name) = function.name {
                if !name.is_empty() {
                    state.name = name;
                }
            }
            if let Some(arguments) = function.arguments {
                state.pending_arguments.push_str(&arguments);
            }
        }

        if !state.started && !state.name.is_empty() {
            let id = if state.id.is_empty() {
                format!("toolu_openai_{block_index}")
            } else {
                state.id.clone()
            };

            events.push(StreamEvent::ContentBlockStart {
                index: block_index,
                content_block: ContentBlock::ToolUse {
                    id,
                    name: state.name.clone(),
                    input: serde_json::Value::Object(serde_json::Map::new()),
                },
            });
            state.started = true;
        }

        if state.started && !state.pending_arguments.is_empty() {
            let partial_json = std::mem::take(&mut state.pending_arguments);
            events.push(StreamEvent::ContentBlockDelta {
                index: block_index,
                delta: Delta {
                    delta_type: Some("input_json_delta".to_string()),
                    text: None,
                    partial_json: Some(partial_json),
                },
            });
        }
    }

    fn ensure_openai_tool_state(&mut self, index: usize) {
        if self.openai_tools.len() <= index {
            self.openai_tools
                .resize_with(index + 1, OpenAiToolState::default);
        }
    }

    fn close_openai_tool_blocks(&mut self, events: &mut Vec<StreamEvent>) {
        for (index, state) in self.openai_tools.iter_mut().enumerate() {
            if index == 0 {
                continue;
            }
            if state.started && !state.stopped {
                events.push(StreamEvent::ContentBlockStop { index });
                state.stopped = true;
            }
        }
    }
}
