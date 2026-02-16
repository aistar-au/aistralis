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
        let mut events = Vec::new();
        let mut start = 0;

        while let Some(end) = self.buffer[start..].find("\n\n") {
            let event_end = start + end + 2;
            let event_text = &self.buffer[start..event_end];

            let mut event_type = None;
            let mut data = None;

            for line in event_text.lines() {
                if let Some(rest) = line.strip_prefix("event: ") {
                    event_type = Some(rest.to_string());
                } else if let Some(rest) = line.strip_prefix("data: ") {
                    data = Some(rest.trim().to_string());
                }
            }

            if let (Some(evt_type), Some(json_data)) = (event_type, data) {
                if json_data == "[DONE]"
                    || !matches!(
                        evt_type.as_str(),
                        "message_start"
                            | "content_block_start"
                            | "content_block_delta"
                            | "content_block_stop"
                            | "message_delta"
                            | "message_stop"
                    )
                {
                    start = event_end;
                    continue;
                }

                match serde_json::from_str::<StreamEvent>(&json_data) {
                    Ok(evt) => events.push(evt),
                    Err(e) => {
                        eprintln!("⚠️  SSE parse error: {}", e);
                        eprintln!("   Event type: {}", evt_type);
                        eprintln!("   Data: {}", json_data);
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
