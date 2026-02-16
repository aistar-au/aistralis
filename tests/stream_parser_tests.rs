use aistar::api::stream::StreamParser;

#[test]
fn test_fragmented_events() {
    let mut parser = StreamParser::new();

    let chunk1 = b"event: content_block_delta\ndata: {\"type\":\"content";
    let events1 = parser.process(chunk1).unwrap();
    assert_eq!(events1.len(), 0);

    let chunk2 = b"_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"Hi\"}}\n\n";
    let events2 = parser.process(chunk2).unwrap();
    assert_eq!(events2.len(), 1);
}

#[test]
fn test_parse_error_handling() {
    let mut parser = StreamParser::new();

    let chunk = b"event: message_start\ndata: {invalid json}\n\n";
    let events = parser.process(chunk).unwrap();
    assert_eq!(events.len(), 0);
}
