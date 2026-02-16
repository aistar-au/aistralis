use crate::types::StreamEvent;
use anyhow::Result;

#[derive(Default)]
pub struct StreamParser {
    buffer: String,
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
                    false
                } else {
                    event_type.as_deref().map_or(true, is_supported_event_type)
                };

                if should_parse {
                    match serde_json::from_str::<StreamEvent>(&json_data) {
                        Ok(evt) => events.push(evt),
                        Err(e) => {
                            eprintln!("⚠️  SSE parse error: {}", e);
                            eprintln!(
                                "   Event type: {}",
                                event_type.as_deref().unwrap_or("<none>")
                            );
                            eprintln!("   Data: {}", json_data);
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

    pub fn flush(&mut self) -> String {
        std::mem::take(&mut self.buffer)
    }
}

fn is_supported_event_type(event_type: &str) -> bool {
    matches!(
        event_type,
        "message_start"
            | "content_block_start"
            | "content_block_delta"
            | "content_block_stop"
            | "message_delta"
            | "message_stop"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sse_fragmentation() {
        let mut parser = StreamParser::new();
        let frag1 = "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello";
        let frag2 = "\"}}\n\n";

        let events1 = parser.process(frag1.as_bytes()).unwrap();
        assert_eq!(events1.len(), 0); // Should be empty, fragment held in buffer

        let events2 = parser.process(frag2.as_bytes()).unwrap();
        assert_eq!(events2.len(), 1); // Should now process the full event
    }
}
