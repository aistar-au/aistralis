use crate::api::client::{ByteStream, MockStreamProducer};
use crate::types::ApiMessage;
use anyhow::Result;
use bytes::Bytes;
use futures::stream;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct MockApiClient {
    responses: Arc<Mutex<Vec<Vec<String>>>>,
}

impl MockApiClient {
    pub fn new(responses: Vec<Vec<String>>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(responses)),
        }
    }
}

#[cfg(test)]
impl MockStreamProducer for MockApiClient {
    fn create_mock_stream(&self, _messages: &[ApiMessage]) -> Result<ByteStream> {
        let mut responses_guard = self.responses.lock().unwrap();
        if responses_guard.is_empty() {
            return Err(anyhow::anyhow!(
                "MockApiClient: No more responses configured"
            ));
        }
        let current_sse_chunks = responses_guard.remove(0);

        let sse_byte_chunks: Vec<Result<Bytes>> = current_sse_chunks
            .into_iter()
            .map(|s| {
                let framed = if s.ends_with("\n\n") {
                    s
                } else {
                    format!("{s}\n\n")
                };
                Ok(Bytes::from(framed))
            })
            .collect();

        Ok(Box::pin(stream::iter(sse_byte_chunks)))
    }
}
