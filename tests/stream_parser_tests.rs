use aistar::api::stream::StreamParser;
use aistar::types::StreamEvent;

#[test]
fn test_fragmented_events() {
    let mut parser = StreamParser::new();

    let chunk1 = b"event: content_block_delta\ndata: {\"type\":\"content";
    let events1 = parser.process(chunk1).expect("first chunk parse");
    assert_eq!(events1.len(), 0);

    let chunk2 =
        b"_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hi\"}}\n\n";
    let events2 = parser.process(chunk2).expect("second chunk parse");
    assert_eq!(events2.len(), 1);
}

#[test]
fn test_parse_error_handling() {
    let mut parser = StreamParser::new();

    let chunk = b"event: message_start\ndata: {invalid json}\n\n";
    let events = parser
        .process(chunk)
        .expect("error handling should not fail parser");
    assert_eq!(events.len(), 0);
}

#[test]
fn test_partial_json_delta_is_parsed() {
    let mut parser = StreamParser::new();

    let chunk = b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"path\\\":\\\"src/\"}}\n\n";
    let events = parser
        .process(chunk)
        .expect("parser should parse input_json deltas");
    assert_eq!(events.len(), 1);

    match &events[0] {
        StreamEvent::ContentBlockDelta { index, delta } => {
            assert_eq!(*index, 1);
            assert_eq!(delta.partial_json.as_deref(), Some("{\"path\":\"src/"));
        }
        other => panic!("unexpected event: {other:?}"),
    }
}
