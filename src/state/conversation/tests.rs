use super::*;
use crate::api::ApiClient;
use crate::state::{StreamBlock, ToolStatus};
use crate::tools::ToolOperator;
use crate::types::{ApiMessage, Content, ContentBlock};
use anyhow::Result;
use serde_json::json;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tokio::sync::mpsc;

#[test]
fn test_conversation_module_structure() {
    let _ = std::any::TypeId::of::<ConversationManager>();
    let _ = std::any::TypeId::of::<ConversationStreamUpdate>();
    let _ = std::any::TypeId::of::<ToolApprovalRequest>();

    assert!(Path::new("src/state/conversation/state.rs").exists());
    assert!(Path::new("src/state/conversation/core.rs").exists());
    assert!(Path::new("src/state/conversation/tools.rs").exists());
    assert!(Path::new("src/state/conversation/streaming.rs").exists());
    assert!(Path::new("src/state/conversation/history.rs").exists());
}

fn tagged_read_file_round(message_id: &str) -> Vec<String> {
    vec![
        format!(
            r#"event: message_start
data: {{"type":"message_start","message":{{"id":"{message_id}","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{{"input_tokens":10,"output_tokens":1}}}}}}"#
        ),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#
            .to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"I will read it.\n<function=read_file>\n<parameter=path>\nfile.txt\n</parameter>\n</function>"}}"#
            .to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}"#
            .to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#
            .to_string(),
    ]
}

fn plain_text_round(message_id: &str, text: &str) -> Vec<String> {
    vec![
        format!(
            r#"event: message_start
data: {{"type":"message_start","message":{{"id":"{message_id}","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{{"input_tokens":10,"output_tokens":1}}}}}}"#
        ),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#
            .to_string(),
        format!(
            r#"event: content_block_delta
data: {{"type":"content_block_delta","index":0,"delta":{{"type":"text_delta","text":"{text}"}}}}"#
        ),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}"#
            .to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#
            .to_string(),
    ]
}

#[test]
fn test_read_only_tool_round_helpers() {
    let read_round = vec![ContentBlock::ToolUse {
        id: "tool_1".to_string(),
        name: "read_file".to_string(),
        input: json!({"path":"src/app/mod.rs"}),
    }];
    assert!(is_read_only_tool_round(&read_round));

    let write_round = vec![ContentBlock::ToolUse {
        id: "tool_2".to_string(),
        name: "write_file".to_string(),
        input: json!({"path":"src/app/mod.rs","content":"x"}),
    }];
    assert!(!is_read_only_tool_round(&write_round));

    let sig_a = tool_round_signature(&read_round);
    let sig_b = tool_round_signature(&read_round);
    assert_eq!(sig_a, sig_b);

    let changed_read_round = vec![ContentBlock::ToolUse {
        id: "tool_3".to_string(),
        name: "read_file".to_string(),
        input: json!({"path":"src/state/conversation.rs"}),
    }];
    let sig_c = tool_round_signature(&changed_read_round);
    assert_ne!(sig_a, sig_c);
}

#[test]
fn test_tool_requires_confirmation_for_mutating_tools() {
    assert!(tool_requires_confirmation("write_file"));
    assert!(tool_requires_confirmation("edit_file"));
    assert!(tool_requires_confirmation("rename_file"));
    assert!(tool_requires_confirmation("git_add"));
    assert!(tool_requires_confirmation("git_commit"));

    assert!(!tool_requires_confirmation("read_file"));
    assert!(!tool_requires_confirmation("search_files"));
    assert!(!tool_requires_confirmation("list_files"));
    assert!(!tool_requires_confirmation("git_status"));
    assert!(!tool_requires_confirmation("git_diff"));
    assert!(!tool_requires_confirmation("git_log"));
    assert!(!tool_requires_confirmation("git_show"));
}

#[tokio::test]
async fn test_crit_01_protocol_flow() -> Result<()> {
    // ANCHOR: This test verifies the multi-turn conversation protocol.
    // It will PASS if the protocol is correctly implemented.
    //
    // The test should:
    // 1. Create a ConversationManager with a mock client
    // 2. Send a message that triggers tool use
    // 3. Verify the tool is executed
    // 4. Verify the final response incorporates tool results

    // Mock responses for the API client
    let first_response_sse = vec![
        r#"event: message_start
data: {"type": "message_start", "message": {"id": "msg_mock_01", "type": "message", "role": "assistant", "model": "mock-model", "content": [], "stop_reason": null, "stop_sequence": null, "usage": {"input_tokens": 10, "output_tokens": 1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type": "content_block_start", "index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type": "content_block_delta", "index":0,"delta":{"type":"text_delta","text":"Okay, I can help with that. "}}"#.to_string(),
        r#"event: content_block_start
data: {"type": "content_block_start", "index":1,"content_block":{"type":"tool_use","id":"toolu_mock_01", "name":"read_file","input":{}}}"#.to_string(),
        r#"event: content_block_delta
data: {"type": "content_block_delta", "index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\": \"file.txt\"}"}}"#.to_string(),
        r#"event: content_block_stop
data: {"type": "content_block_stop", "index":1}"#.to_string(),
        r#"event: message_delta
data: {"type": "message_delta", "delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":6}}"#.to_string(),
        r#"event: message_stop
data: {"type": "message_stop"}"#.to_string(),
    ];

    let second_response_sse = vec![
        r#"event: message_start
data: {"type": "message_start", "message": {"id": "msg_mock_02", "type": "message", "role": "assistant", "model": "mock-model", "content": [], "stop_reason": null, "stop_sequence": null, "usage": {"input_tokens": 10, "output_tokens": 1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type": "content_block_start", "index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type": "content_block_delta", "index":0,"delta":{"type":"text_delta","text":"The content of file.txt is 'Hello from file.txt'"}}"#.to_string(),
        r#"event: message_delta
data: {"type": "message_delta", "delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":10}}"#.to_string(),
        r#"event: message_stop
data: {"type": "message_stop"}"#.to_string(),
    ];

    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            first_response_sse,
            second_response_sse,
        ])));

    let mut mock_tool_responses = HashMap::new();
    mock_tool_responses.insert("file.txt".to_string(), "Hello from file.txt".to_string());

    let mut manager = ConversationManager::new_mock(mock_api_client, mock_tool_responses);

    let final_text = manager
        .send_message("What is in file.txt?".into(), None)
        .await?;

    assert!(final_text.contains("The content of file.txt is 'Hello from file.txt'"));

    // Verify the message history order
    let messages = &manager.api_messages;
    assert_eq!(messages.len(), 4);

    // Initial user message
    assert_eq!(messages[0].role, "user");
    if let Content::Text(text) = &messages[0].content {
        assert!(text.contains("What is in file.txt?"));
    }

    // Assistant message with tool_use
    assert_eq!(messages[1].role, "assistant");
    if let Content::Blocks(blocks) = &messages[1].content {
        assert_eq!(blocks.len(), 2);
        if let ContentBlock::Text { text } = &blocks[0] {
            assert!(text.contains("Okay, I can help with that."));
        }
        if let ContentBlock::ToolUse { id: _, name, input } = &blocks[1] {
            assert_eq!(name, "read_file");
            assert_eq!(input, &json!({ "path": "file.txt" }));
        }
    }

    // User message with tool_result
    assert_eq!(messages[2].role, "user");
    if let Content::Blocks(blocks) = &messages[2].content {
        assert_eq!(blocks.len(), 1);
        if let ContentBlock::ToolResult {
            tool_use_id: _,
            content,
            is_error,
        } = &blocks[0]
        {
            assert!(content.contains("Read file.txt:"));
            assert!(content.contains("Content for model context:"));
            assert!(content.contains("Hello from file.txt"));
            assert!(!is_error);
        }
    }

    // Final assistant message
    assert_eq!(messages[3].role, "assistant");
    if let Content::Blocks(blocks) = &messages[3].content {
        assert_eq!(blocks.len(), 1);
        if let ContentBlock::Text { text } = &blocks[0] {
            assert!(text.contains("The content of file.txt is 'Hello from file.txt'"));
        }
    }

    Ok(())
}

#[tokio::test]
async fn test_structured_text_only_round_streams_final_text_block() -> Result<()> {
    let response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_text_only_1","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"This is the final answer."}}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":8}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];

    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            response_sse,
        ])));

    let mut manager = ConversationManager::new_mock(mock_api_client, HashMap::new());
    let (tx, mut rx) = mpsc::unbounded_channel();

    let final_text = manager
        .send_message("say hi".to_string(), Some(&tx))
        .await?;
    assert_eq!(final_text, "This is the final answer.");

    drop(tx);

    let mut saw_thinking_start = false;
    let mut final_block_content = String::new();
    while let Ok(update) = rx.try_recv() {
        if let ConversationStreamUpdate::BlockStart { block, .. } = update {
            match block {
                StreamBlock::Thinking { .. } => saw_thinking_start = true,
                StreamBlock::FinalText { content } => final_block_content = content,
                StreamBlock::ToolCall { .. } | StreamBlock::ToolResult { .. } => {}
            }
        }
    }

    assert!(!saw_thinking_start);
    assert_eq!(final_block_content, "This is the final answer.");
    Ok(())
}

#[tokio::test]
async fn test_structured_tool_then_final_round_streams_thinking_then_final_text() -> Result<()> {
    let first_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_tool_then_final_1","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"I will read the file."}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_mock_round_1","name":"read_file","input":{}}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"file.txt\"}"}}"#.to_string(),
        r#"event: content_block_stop
data: {"type":"content_block_stop","index":1}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":10}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];

    let second_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_tool_then_final_2","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"The file says hello."}}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":7}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];

    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            first_response_sse,
            second_response_sse,
        ])));

    let mut mock_tool_responses = HashMap::new();
    mock_tool_responses.insert("file.txt".to_string(), "hello".to_string());
    let mut manager = ConversationManager::new_mock(mock_api_client, mock_tool_responses);

    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut saw_thinking_start = false;
    let mut saw_final_start = false;

    let tx_for_send = tx.clone();
    let mut send_future =
        std::pin::pin!(manager.send_message("read file".to_string(), Some(&tx_for_send)));
    let final_text = loop {
        tokio::select! {
            result = &mut send_future => break result?,
            maybe_update = rx.recv() => {
                let Some(update) = maybe_update else { continue; };
                match update {
                    ConversationStreamUpdate::BlockStart { block, .. } => {
                        match block {
                            StreamBlock::Thinking { .. } => saw_thinking_start = true,
                            StreamBlock::FinalText { .. } => saw_final_start = true,
                            StreamBlock::ToolCall { .. } | StreamBlock::ToolResult { .. } => {}
                        }
                    }
                    ConversationStreamUpdate::ToolApprovalRequest(request) => {
                        let _ = request.response_tx.send(true);
                    }
                    ConversationStreamUpdate::Delta(_)
                    | ConversationStreamUpdate::BlockDelta { .. }
                    | ConversationStreamUpdate::BlockComplete { .. } => {}
                }
            }
        }
    };
    assert_eq!(final_text, "The file says hello.");
    drop(tx);

    while let Ok(update) = rx.try_recv() {
        if let ConversationStreamUpdate::BlockStart { block, .. } = update {
            match block {
                StreamBlock::Thinking { .. } => saw_thinking_start = true,
                StreamBlock::FinalText { .. } => saw_final_start = true,
                StreamBlock::ToolCall { .. } | StreamBlock::ToolResult { .. } => {}
            }
        }
    }

    assert!(saw_thinking_start);
    assert!(saw_final_start);
    Ok(())
}

#[test]
fn test_parse_tagged_tool_calls() {
    let text = r#"I can do this.
<function=write_file>
<parameter=path>
cal.rs
</parameter>
<parameter=content>
fn main() {}
</parameter>
</function>"#;

    let calls = parse_tagged_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "write_file");
    assert_eq!(calls[0].input["path"], "cal.rs");
    assert_eq!(calls[0].input["content"], "fn main() {}");
}

#[test]
fn test_parse_tagged_tool_calls_without_parameters() {
    let text = "Checking files.\n<function=list_files></function>";
    let calls = parse_tagged_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "list_files");
    assert_eq!(calls[0].input, json!({}));
}

#[test]
fn test_parse_tagged_tool_calls_with_missing_closing_tags() {
    let text = r#"I'll check it.
<function=read_file>
<parameter=path>
cal.js
"#;
    let calls = parse_tagged_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "read_file");
    assert_eq!(calls[0].input["path"], "cal.js");
}

#[test]
fn test_truncate_for_history() {
    let text = "abcdefghij";
    let truncated = truncate_for_history(text, 40);
    assert_eq!(truncated, text);

    let truncated = truncate_for_history(text, 5);
    assert!(truncated.len() <= 5);

    let long_text = "abcdefghijklmnopqrstuvwxyz0123456789";
    let truncated_with_marker = truncate_for_history(long_text, 30);
    assert!(truncated_with_marker.contains("[truncated"));
}

#[test]
fn test_truncate_for_history_preserves_tail_context() {
    let text = "head-aaaa-bbbb-cccc-dddd-eeee-ffff-gggg-tail";
    let truncated = truncate_for_history(text, 40);
    assert!(truncated.contains("[truncated"));
    assert!(truncated.contains("head"));
    assert!(truncated.contains("tail"));
}

#[test]
fn test_required_tool_string_validation() {
    let input = json!({ "path": " cal.rs " });
    assert_eq!(
        required_tool_string(&input, "read_file", "path").unwrap(),
        "cal.rs"
    );

    let missing = json!({});
    assert!(required_tool_string(&missing, "read_file", "path").is_err());
}

#[test]
fn test_default_tool_approval_enabled_prefers_remote_only() {
    assert!(default_tool_approval_enabled(false));
    assert!(!default_tool_approval_enabled(true));
}

#[test]
fn test_missing_mutating_location_prompt_requires_explicit_paths() {
    let edit_missing = json!({
        "old_str": "a",
        "new_str": "b"
    });
    let edit_with_path = json!({
        "file_path": "src/calculator.rs",
        "old_str": "a",
        "new_str": "b"
    });
    let rename_missing = json!({
        "old_path": "src/a.rs"
    });
    let rename_ready = json!({
        "from": "src/a.rs",
        "to": "src/b.rs"
    });

    assert!(missing_mutating_location_prompt("edit_file", &edit_missing).is_some());
    assert!(missing_mutating_location_prompt("edit_file", &edit_with_path).is_none());
    assert!(missing_mutating_location_prompt("rename_file", &rename_missing).is_some());
    assert!(missing_mutating_location_prompt("rename_file", &rename_ready).is_none());
    assert!(missing_mutating_location_prompt("read_file", &json!({"path":"x"})).is_none());
}

#[test]
fn test_read_only_user_request_detection_and_mutating_guard() {
    assert!(is_read_only_user_request("show me calculator.rs"));
    assert!(is_read_only_user_request("what is in src/runtime/loop.rs"));
    assert!(!is_read_only_user_request(
        "add a new function and commit it"
    ));

    let guard = mutating_tool_read_only_conflict_prompt("show the git diff", "write_file");
    assert!(
        guard.is_some(),
        "mutating call should be blocked for read-only request"
    );
    assert!(
        guard.unwrap().contains("No file changes were made"),
        "guard copy should be explicit about mutation safety"
    );

    assert!(
        mutating_tool_read_only_conflict_prompt("add calculator.rs", "write_file").is_none(),
        "explicit mutating intent should not be blocked"
    );
}

#[test]
fn test_builtin_supported_git_tools_response_lists_only_supported_tools() {
    let response = builtin_supported_git_tools_response("what other git tools can you call")
        .expect("expected capability response");
    assert!(response.contains("git_status"));
    assert!(response.contains("git_diff"));
    assert!(response.contains("git_log"));
    assert!(response.contains("git_show"));
    assert!(response.contains("git_add"));
    assert!(response.contains("git_commit"));
    assert!(!response.contains("git_clone"));
    assert!(!response.contains("git_init"));
    assert!(builtin_supported_git_tools_response("show the git diff").is_none());
}

#[test]
fn test_env_bool_off_is_false_across_state_paths() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    std::env::set_var("VEX_STREAM_LOCAL_TOOL_EVENTS", "off");
    std::env::set_var("VEX_STREAM_SERVER_EVENTS", "off");
    std::env::set_var("VEX_TOOL_CONFIRM", "off");

    assert!(!stream_local_tool_events_enabled());
    assert!(!stream_server_events_enabled());
    assert!(!tool_approval_enabled(false));

    std::env::remove_var("VEX_STREAM_LOCAL_TOOL_EVENTS");
    std::env::remove_var("VEX_STREAM_SERVER_EVENTS");
    std::env::remove_var("VEX_TOOL_CONFIRM");
}

#[test]
fn test_format_tool_result_for_history_read_file_diff_and_repeat() {
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let mut manager = ConversationManager::new_mock(mock_api_client, HashMap::new());
    let input = serde_json::json!({ "path": "cal.rs" });

    let first = manager.format_tool_result_for_history(
        "read_file",
        &input,
        &Ok("line1\nline2".to_string()),
    );
    assert!(first.contains("Read cal.rs:"));
    assert!(first.contains("Content for model context:"));
    assert!(first.contains("line1\nline2"));

    let second = manager.format_tool_result_for_history(
        "read_file",
        &input,
        &Ok("line1\nline2".to_string()),
    );
    assert!(second.contains("No changes since last read"));

    let third = manager.format_tool_result_for_history(
        "read_file",
        &input,
        &Ok("line1\nline2 changed".to_string()),
    );
    assert!(third.contains("content changed"));
    assert!(third.contains("Content for model context:"));
    assert!(third.contains("line1\nline2 changed"));

    // After a change the cache must update, so the same content read again
    // must be classified as Unchanged — not another Changed.
    let fourth = manager.format_tool_result_for_history(
        "read_file",
        &input,
        &Ok("line1\nline2 changed".to_string()),
    );
    assert!(
        fourth.contains("No changes since last read"),
        "expected Unchanged after re-reading the post-change content, got: {fourth}"
    );
}

#[tokio::test]
async fn test_text_tagged_tool_call_executes_as_fallback_for_local_endpoint() -> Result<()> {
    let first_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_mock_10","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"I'll read it.\n<function=read_file>\n<parameter=path>\nfile.txt\n</parameter>\n</function>"}}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];
    let second_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_mock_11","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Read complete: Hello from fallback."}}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];

    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            first_response_sse,
            second_response_sse,
        ])));

    let mut mock_tool_responses = HashMap::new();
    mock_tool_responses.insert("file.txt".to_string(), "Hello from fallback.".to_string());
    let mut manager = ConversationManager::new_mock(mock_api_client, mock_tool_responses);

    let final_text = manager.send_message("Read file".into(), None).await?;
    assert!(final_text.contains("Read complete: Hello from fallback."));

    let messages = &manager.api_messages;
    assert!(
        messages.iter().any(|message| {
            if message.role != "assistant" {
                return false;
            }
            match &message.content {
                Content::Text(text) => {
                    text.contains("I'll read it.") && text.contains("<function=read_file>")
                }
                _ => false,
            }
        }),
        "expected fallback parser to persist text-protocol tool call markup"
    );
    assert!(
        messages.iter().any(|message| {
            message.role == "user"
                && matches!(
                    &message.content,
                    Content::Text(text) if text.contains("tool_result read_file")
                )
        }),
        "expected fallback parser to execute read_file and append tool_result text"
    );

    Ok(())
}

#[tokio::test]
async fn test_text_tagged_tool_call_emits_structured_tool_blocks_for_fallback() -> Result<()> {
    let first_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_mock_fallback_20","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"I will read it.\n<function=read_file>\n<parameter=path>\nfile.txt\n</parameter>\n</function>"}}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];
    let second_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_mock_fallback_21","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"done"}}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];

    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            first_response_sse,
            second_response_sse,
        ])));
    let mut mock_tool_responses = HashMap::new();
    mock_tool_responses.insert("file.txt".to_string(), "Hello from fallback.".to_string());
    let mut manager = ConversationManager::new_mock(mock_api_client, mock_tool_responses);

    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut saw_tool_call_block = false;
    let tx_for_send = tx.clone();
    let mut send_future =
        std::pin::pin!(manager.send_message("Read file".to_string(), Some(&tx_for_send)));
    let _final_text = loop {
        tokio::select! {
            result = &mut send_future => break result?,
            maybe_update = rx.recv() => {
                let Some(update) = maybe_update else { continue; };
                match update {
                    ConversationStreamUpdate::BlockStart { block, .. } => {
                        if matches!(block, StreamBlock::ToolCall { ref name, .. } if name == "read_file") {
                            saw_tool_call_block = true;
                        }
                    }
                    ConversationStreamUpdate::ToolApprovalRequest(request) => {
                        let _ = request.response_tx.send(true);
                    }
                    ConversationStreamUpdate::Delta(_)
                    | ConversationStreamUpdate::BlockDelta { .. }
                    | ConversationStreamUpdate::BlockComplete { .. } => {}
                }
            }
        }
    };

    drop(tx);
    while let Ok(update) = rx.try_recv() {
        if let ConversationStreamUpdate::BlockStart { block, .. } = update {
            if matches!(block, StreamBlock::ToolCall { ref name, .. } if name == "read_file") {
                saw_tool_call_block = true;
            }
        }
    }

    assert!(saw_tool_call_block);
    Ok(())
}

#[tokio::test]
async fn test_openai_stream_tool_call_round_trip() -> Result<()> {
    let first_response_sse = vec![
        r#"data: {"id":"chatcmpl_mock_1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","content":"I'll read it. "},"finish_reason":null}]}"#.to_string(),
        r#"data: {"id":"chatcmpl_mock_1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_mock_1","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"file.txt\"}"}}]},"finish_reason":"tool_calls"}]}"#.to_string(),
        "data: [DONE]".to_string(),
    ];

    let second_response_sse = vec![
        r#"data: {"id":"chatcmpl_mock_2","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","content":"The content is Hello from OpenAI stream."},"finish_reason":"stop"}]}"#.to_string(),
        "data: [DONE]".to_string(),
    ];

    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            first_response_sse,
            second_response_sse,
        ])));

    let mut mock_tool_responses = HashMap::new();
    mock_tool_responses.insert(
        "file.txt".to_string(),
        "Hello from OpenAI stream.".to_string(),
    );
    let mut manager = ConversationManager::new_mock(mock_api_client, mock_tool_responses);

    let final_text = manager.send_message("Read file".into(), None).await?;
    assert!(final_text.contains("Hello from OpenAI stream."));

    let messages = &manager.api_messages;
    assert_eq!(messages.len(), 4);
    assert_eq!(messages[1].role, "assistant");
    if let Content::Blocks(blocks) = &messages[1].content {
        assert!(blocks.iter().any(
            |block| matches!(block, ContentBlock::ToolUse { name, .. } if name == "read_file")
        ));
    } else {
        panic!("expected assistant blocks");
    }

    Ok(())
}

#[tokio::test]
async fn test_mutating_tool_prompts_approval_when_tool_confirm_env_is_off() -> Result<()> {
    let _env_lock = crate::test_support::ENV_LOCK.lock().await;
    std::env::set_var("VEX_TOOL_CONFIRM", "off");

    let first_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_mut_01","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_mut_01","name":"write_file","input":{"path":"calculator.rs","content":"fn main() {}\n"}}}"#.to_string(),
        r#"event: content_block_stop
data: {"type":"content_block_stop","index":0}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":4}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];

    let second_response_sse = plain_text_round("msg_mut_02", "No changes were applied.");
    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            first_response_sse,
            second_response_sse,
        ])));
    let mut manager = ConversationManager::new_mock(mock_api_client, HashMap::new());

    let (tx, mut rx) = mpsc::unbounded_channel();
    let approval_task = tokio::spawn(async move {
        let mut saw_approval_request = false;
        while let Some(update) = rx.recv().await {
            if let ConversationStreamUpdate::ToolApprovalRequest(request) = update {
                saw_approval_request = true;
                let _ = request.response_tx.send(false);
            }
        }
        saw_approval_request
    });
    let final_text = manager
        .send_message("create calculator.rs".to_string(), Some(&tx))
        .await?;
    drop(tx);
    let saw_approval_request = approval_task.await?;

    std::env::remove_var("VEX_TOOL_CONFIRM");
    assert!(saw_approval_request);
    assert!(final_text.contains("No changes were applied."));
    let tool_result_message = manager
        .api_messages
        .iter()
        .find(|message| {
            message.role == "user"
                && matches!(message.content, Content::Blocks(_))
                && message_contains_tool_result(message)
        })
        .expect("expected tool_result message in history");
    if let Content::Blocks(blocks) = &tool_result_message.content {
        assert!(blocks.iter().any(|block| matches!(
            block,
            ContentBlock::ToolResult {
                tool_use_id,
                is_error: true,
                ..
            } if tool_use_id == "toolu_mut_01"
        )));
    } else {
        panic!("expected tool_result blocks");
    }
    Ok(())
}

#[tokio::test]
async fn test_edit_file_missing_path_returns_clarification_instead_of_looping() -> Result<()> {
    let first_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_missing_path_01","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_missing_path_01","name":"edit_file","input":{"old_text":"x","new_text":"y"}}}"#.to_string(),
        r#"event: content_block_stop
data: {"type":"content_block_stop","index":0}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":3}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];

    let second_response_sse =
        plain_text_round("msg_missing_path_02", "Please provide a target file path.");
    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            first_response_sse,
            second_response_sse,
        ])));
    let mut manager = ConversationManager::new_mock(mock_api_client, HashMap::new());

    let final_text = manager
        .send_message("please edit".to_string(), None)
        .await?;
    assert!(final_text.contains("target file path"));
    let tool_result_message = manager
        .api_messages
        .iter()
        .find(|message| {
            message.role == "user"
                && matches!(message.content, Content::Blocks(_))
                && message_contains_tool_result(message)
        })
        .expect("expected tool_result message in history");
    if let Content::Blocks(blocks) = &tool_result_message.content {
        assert!(blocks
            .iter()
            .any(|block| matches!(block, ContentBlock::ToolResult { is_error: true, .. })));
    } else {
        panic!("expected tool_result blocks");
    }
    Ok(())
}

#[tokio::test]
async fn test_tool_execution_error_sets_error_status() -> Result<()> {
    let first_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_tool_error_01","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_error_01","name":"read_file","input":{}}}"#.to_string(),
        r#"event: content_block_stop
data: {"type":"content_block_stop","index":0}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":3}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];
    let second_response_sse = plain_text_round("msg_tool_error_02", "Handled read error.");
    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            first_response_sse,
            second_response_sse,
        ])));
    let mut manager = ConversationManager::new_mock(mock_api_client, HashMap::new());

    let (tx, mut rx) = mpsc::unbounded_channel();
    let final_text = manager
        .send_message("read missing path".to_string(), Some(&tx))
        .await?;
    assert!(final_text.contains("Handled read error."));
    drop(tx);

    let mut saw_error_status = false;
    while let Ok(update) = rx.try_recv() {
        if let ConversationStreamUpdate::BlockStart {
            block: StreamBlock::ToolCall { id, status, .. },
            ..
        } = update
        {
            if id == "toolu_error_01" && status == ToolStatus::Error {
                saw_error_status = true;
            }
        }
    }
    assert!(
        saw_error_status,
        "tool execution failure must emit ToolStatus::Error"
    );
    Ok(())
}

#[tokio::test]
async fn test_multi_tool_round_collects_results_after_approval_denial() -> Result<()> {
    let _env_lock = crate::test_support::ENV_LOCK.lock().await;
    std::env::set_var("VEX_TOOL_CONFIRM", "off");

    let first_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_multi_01","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_multi_mut","name":"write_file","input":{"path":"calculator.rs","content":"fn main() {}\n"}}}"#.to_string(),
        r#"event: content_block_stop
data: {"type":"content_block_stop","index":0}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_multi_read","name":"read_file","input":{"path":"file.txt"}}}"#.to_string(),
        r#"event: content_block_stop
data: {"type":"content_block_stop","index":1}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":6}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];
    let second_response_sse = plain_text_round("msg_multi_02", "Handled both tool outcomes.");
    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            first_response_sse,
            second_response_sse,
        ])));
    let mut mock_tool_responses = HashMap::new();
    mock_tool_responses.insert("file.txt".to_string(), "hello".to_string());
    let mut manager = ConversationManager::new_mock(mock_api_client, mock_tool_responses);

    let (tx, mut rx) = mpsc::unbounded_channel();
    let approval_task = tokio::spawn(async move {
        let mut saw_approval_request = false;
        while let Some(update) = rx.recv().await {
            if let ConversationStreamUpdate::ToolApprovalRequest(request) = update {
                saw_approval_request = true;
                let _ = request.response_tx.send(false);
            }
        }
        saw_approval_request
    });
    let final_text = manager
        .send_message("run mixed tools".to_string(), Some(&tx))
        .await?;
    drop(tx);
    let saw_approval_request = approval_task.await?;
    std::env::remove_var("VEX_TOOL_CONFIRM");

    assert!(
        saw_approval_request,
        "expected mutating tool approval request"
    );
    assert!(final_text.contains("Handled both tool outcomes."));

    let tool_result_message = manager
        .api_messages
        .iter()
        .find(|message| {
            message.role == "user"
                && matches!(message.content, Content::Blocks(_))
                && message_contains_tool_result(message)
        })
        .expect("expected tool_result message in history");
    if let Content::Blocks(blocks) = &tool_result_message.content {
        assert!(blocks.iter().any(|block| matches!(
            block,
            ContentBlock::ToolResult {
                tool_use_id,
                is_error: true,
                ..
            } if tool_use_id == "toolu_multi_mut"
        )));
        assert!(blocks.iter().any(|block| matches!(
            block,
            ContentBlock::ToolResult {
                tool_use_id,
                is_error: false,
                ..
            } if tool_use_id == "toolu_multi_read"
        )));
    } else {
        panic!("expected tool_result blocks");
    }
    Ok(())
}

#[tokio::test]
async fn test_read_only_request_blocks_mutating_tool_without_approval_prompt() -> Result<()> {
    let _env_lock = crate::test_support::ENV_LOCK.lock().await;
    std::env::set_var("VEX_TOOL_CONFIRM", "off");

    let first_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_readonly_guard_01","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_readonly_guard_01","name":"write_file","input":{"path":"calculator.rs","content":"fn main() {}\n"}}}"#.to_string(),
        r#"event: content_block_stop
data: {"type":"content_block_stop","index":0}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":5}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];
    let second_response_sse = plain_text_round(
        "msg_readonly_guard_02",
        "Read-only request handled without file mutation.",
    );
    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            first_response_sse,
            second_response_sse,
        ])));
    let mut manager = ConversationManager::new_mock(mock_api_client, HashMap::new());

    let (tx, mut rx) = mpsc::unbounded_channel();
    let approval_task = tokio::spawn(async move {
        let mut saw_approval_request = false;
        while let Some(update) = rx.recv().await {
            if matches!(update, ConversationStreamUpdate::ToolApprovalRequest(_)) {
                saw_approval_request = true;
            }
        }
        saw_approval_request
    });
    let final_text = manager
        .send_message("show me calculator.rs".to_string(), Some(&tx))
        .await?;
    drop(tx);
    let saw_approval_request = approval_task.await?;
    std::env::remove_var("VEX_TOOL_CONFIRM");

    assert!(
        !saw_approval_request,
        "read-only request guard should block mutating tool before approval overlay"
    );
    assert!(final_text.contains("Read-only request handled"));

    let tool_result_message = manager
        .api_messages
        .iter()
        .find(|message| {
            message.role == "user"
                && matches!(message.content, Content::Blocks(_))
                && message_contains_tool_result(message)
        })
        .expect("expected tool_result message in history");
    if let Content::Blocks(blocks) = &tool_result_message.content {
        assert!(blocks.iter().any(|block| matches!(
            block,
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error: true,
            } if tool_use_id == "toolu_readonly_guard_01" && content.contains("appears read-only")
        )));
    } else {
        panic!("expected tool_result blocks");
    }

    Ok(())
}

#[tokio::test]
async fn test_git_tool_capability_query_short_circuits_without_api_round() -> Result<()> {
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let mut manager = ConversationManager::new_mock(mock_api_client, HashMap::new());

    let response = manager
        .send_message("what other git tools can you call".to_string(), None)
        .await?;

    assert!(response.contains("git_status"));
    assert!(response.contains("git_diff"));
    assert!(response.contains("git_log"));
    assert!(response.contains("git_show"));
    assert!(response.contains("git_add"));
    assert!(response.contains("git_commit"));
    assert_eq!(
        manager.api_messages.len(),
        2,
        "capability response should not call API or create extra rounds"
    );

    Ok(())
}

#[tokio::test]
async fn test_local_text_protocol_tool_round_trip() -> Result<()> {
    let first_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_mock_local_10","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"I will read it.\n<function=read_file>\n<parameter=path>\nfile.txt\n</parameter>\n"}}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];
    let second_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_mock_local_11","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Tool result consumed."}}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];

    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            first_response_sse,
            second_response_sse,
        ])))
        .with_structured_tool_protocol(false);

    let mut mock_tool_responses = HashMap::new();
    mock_tool_responses.insert(
        "file.txt".to_string(),
        "Hello local text protocol.".to_string(),
    );
    let mut manager = ConversationManager::new_mock(mock_api_client, mock_tool_responses);

    let final_text = manager.send_message("Read file".into(), None).await?;
    assert!(final_text.contains("Tool result consumed."));

    let messages = &manager.api_messages;
    assert!(
        messages.iter().any(|message| {
            if message.role != "assistant" {
                return false;
            }
            match &message.content {
                Content::Text(text) => {
                    text.contains("I will read it.") && text.contains("<function=read_file>")
                }
                _ => false,
            }
        }),
        "expected fallback to preserve rendered text-protocol tool call in history"
    );
    assert!(
        messages.iter().any(|message| {
            message.role == "user"
                && matches!(
                    &message.content,
                    Content::Text(text) if text.contains("tool_result read_file")
                )
        }),
        "expected text protocol tool_result payload to be appended for the next round"
    );

    Ok(())
}

#[tokio::test]
async fn test_local_endpoint_retries_once_when_tool_evidence_required() -> Result<()> {
    let first_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_retry_01","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":8,"output_tokens":2}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Let me check that."}}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":3}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];
    let second_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_retry_02","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":8,"output_tokens":2}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"<function=read_file>\n<parameter=path>\nCargo.toml\n</parameter>\n</function>"}}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":3}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];
    let third_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_retry_03","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":8,"output_tokens":2}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Tool-backed summary complete."}}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":3}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];

    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            first_response_sse,
            second_response_sse,
            third_response_sse,
        ])));
    let mut mock_tool_responses = HashMap::new();
    mock_tool_responses.insert(
        "Cargo.toml".to_string(),
        "[package]\nname = \"vexcoder\"".to_string(),
    );
    let mut manager = ConversationManager::new_mock(mock_api_client, mock_tool_responses);

    let final_text = manager
        .send_message("how many files are in this tree".to_string(), None)
        .await?;
    assert!(final_text.contains("Tool-backed summary complete."));

    let correction_count = manager
        .api_messages
        .iter()
        .filter(|message| {
            message.role == "user"
                && matches!(
                    &message.content,
                    Content::Text(text) if text.contains("did not execute any tool call")
                )
        })
        .count();
    assert_eq!(
        correction_count, 1,
        "expected exactly one corrective tool-use retry message"
    );

    Ok(())
}

#[tokio::test]
async fn test_local_endpoint_retry_only_once_when_model_stays_toolless() -> Result<()> {
    let first_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_retry_once_01","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":8,"output_tokens":2}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Let me inspect that."}}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":3}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];
    let second_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_retry_once_02","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":8,"output_tokens":2}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Still no tool call."}}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":3}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];
    let third_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_retry_once_03","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":8,"output_tokens":2}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Still no tool call."}}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":3}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];

    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            first_response_sse,
            second_response_sse,
            third_response_sse,
        ])));
    let mut manager = ConversationManager::new_mock(mock_api_client, HashMap::new());

    let final_text = manager
        .send_message("show me the file count".to_string(), None)
        .await?;
    assert!(final_text.contains("Still no tool call."));
    assert!(
        final_text.contains("[loop guard]"),
        "tool-evidence-required prompts must return guard text when model stays toolless"
    );

    let correction_count = manager
        .api_messages
        .iter()
        .filter(|message| {
            message.role == "user"
                && matches!(
                    &message.content,
                    Content::Text(text) if text.contains("did not execute any tool call")
                )
        })
        .count();
    assert_eq!(
        correction_count, 2,
        "retry message should be inserted twice"
    );

    Ok(())
}

#[tokio::test]
async fn test_repeated_read_only_round_injects_nudge_then_recovers() -> Result<()> {
    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            tagged_read_file_round("msg_loop_nudge_01"),
            tagged_read_file_round("msg_loop_nudge_02"),
            tagged_read_file_round("msg_loop_nudge_03"),
            plain_text_round("msg_loop_nudge_04", "Done after loop correction."),
        ])));
    let mut mock_tool_responses = HashMap::new();
    mock_tool_responses.insert("file.txt".to_string(), "loop sample".to_string());
    let mut manager = ConversationManager::new_mock(mock_api_client, mock_tool_responses);

    let final_text = manager.send_message("read file".to_string(), None).await?;
    assert!(final_text.contains("Done after loop correction."));

    let nudge_count = manager
        .api_messages
        .iter()
        .filter(|message| {
            message.role == "user"
                && matches!(
                    &message.content,
                    Content::Text(text) if text.contains("Do not repeat identical tool calls")
                )
        })
        .count();
    assert_eq!(nudge_count, 1, "expected exactly one loop-correction nudge");

    Ok(())
}

#[tokio::test]
async fn test_repeated_read_only_round_returns_guard_message_instead_of_error() -> Result<()> {
    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            tagged_read_file_round("msg_loop_guard_01"),
            tagged_read_file_round("msg_loop_guard_02"),
            tagged_read_file_round("msg_loop_guard_03"),
            tagged_read_file_round("msg_loop_guard_04"),
        ])));
    let mut mock_tool_responses = HashMap::new();
    mock_tool_responses.insert("file.txt".to_string(), "loop sample".to_string());
    let mut manager = ConversationManager::new_mock(mock_api_client, mock_tool_responses);

    let final_text = manager.send_message("read file".to_string(), None).await?;
    assert!(final_text.contains("[loop guard]"));
    assert!(final_text.contains("Repeated identical read/search tool calls"));

    Ok(())
}

#[tokio::test]
async fn test_repeated_mutating_round_returns_guard_message_instead_of_looping() -> Result<()> {
    let mutating_round = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_mut_loop_01","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_mut_loop_01","name":"edit_file","input":{"path":"src/calculator.rs","old_str":"","new_str":"x"}}}"#.to_string(),
        r#"event: content_block_stop
data: {"type":"content_block_stop","index":0}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":3}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];

    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            mutating_round.clone(),
            mutating_round,
        ])));
    let mut manager = ConversationManager::new_mock(mock_api_client, HashMap::new());

    let final_text = manager
        .send_message("edit calculator".to_string(), None)
        .await?;
    assert!(final_text.contains("[loop guard]"));
    assert!(final_text.contains("Repeated identical mutating tool calls"));
    Ok(())
}

#[tokio::test]
async fn test_tool_use_without_input_then_partial_json_executes_write_file() -> Result<()> {
    let temp = TempDir::new()?;

    let first_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_mock_20","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Saving now."}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_mock_write_1","name":"write_file"}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"cal.rs\",\"content\":\"fn main() {}\\n\"}"}}"#.to_string(),
        r#"event: content_block_stop
data: {"type":"content_block_stop","index":1}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":12}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];

    let second_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_mock_21","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Saved cal.rs."}}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":5}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];

    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            first_response_sse,
            second_response_sse,
        ])));

    let executor = ToolOperator::new(temp.path().to_path_buf());
    let mut manager = ConversationManager::new(mock_api_client, executor);

    let final_text = manager
        .send_message("create calculator".to_string(), None)
        .await?;
    assert!(final_text.contains("Saved cal.rs."));

    let written = std::fs::read_to_string(temp.path().join("cal.rs"))?;
    assert_eq!(written, "fn main() {}\n");

    Ok(())
}

#[tokio::test]
async fn test_execute_tool_edit_file_empty_path_rejected_before_executor() -> Result<()> {
    let temp = TempDir::new()?;
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let executor = ToolOperator::new(temp.path().to_path_buf());
    let manager = ConversationManager::new(mock_api_client, executor);

    let err = manager
        .execute_tool_with_timeout(
            "edit_file",
            &json!({
                "path": "",
                "old_str": "old",
                "new_str": "new"
            }),
            Duration::from_secs(1),
        )
        .await
        .expect_err("empty path should be rejected");
    assert!(err.to_string().contains("non-empty 'path'"));
    Ok(())
}

#[tokio::test]
async fn test_execute_tool_edit_file_accepts_alias_argument_names() -> Result<()> {
    let temp = TempDir::new()?;
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let executor = ToolOperator::new(temp.path().to_path_buf());
    let manager = ConversationManager::new(mock_api_client, executor);

    let target = temp.path().join("src").join("calculator.rs");
    std::fs::create_dir_all(target.parent().expect("target parent exists"))?;
    std::fs::write(&target, "pub fn calc() -> i32 { 1 }\n")?;

    let result = manager
        .execute_tool_with_timeout(
            "edit_file",
            &json!({
                "file_path": "src/calculator.rs",
                "old_text": "1",
                "new_text": "2"
            }),
            Duration::from_secs(1),
        )
        .await?;
    assert!(result.contains("Updated snippet in src/calculator.rs"));

    let updated = std::fs::read_to_string(&target)?;
    assert!(updated.contains("2"));
    Ok(())
}

#[tokio::test]
async fn test_execute_tool_edit_file_delete_summary_is_clear() -> Result<()> {
    let temp = TempDir::new()?;
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let executor = ToolOperator::new(temp.path().to_path_buf());
    let manager = ConversationManager::new(mock_api_client, executor);

    let target = temp.path().join("src").join("calculator.rs");
    std::fs::create_dir_all(target.parent().expect("target parent exists"))?;
    std::fs::write(&target, "pub fn sqrt() {}\n// keep\n")?;

    let result = manager
        .execute_tool_with_timeout(
            "edit_file",
            &json!({
                "path": "src/calculator.rs",
                "old_str": "pub fn sqrt() {}\n",
                "new_str": ""
            }),
            Duration::from_secs(1),
        )
        .await?;

    assert!(result.contains("Deleted snippet in src/calculator.rs"));
    assert_eq!(std::fs::read_to_string(&target)?, "// keep\n");
    Ok(())
}

#[test]
fn test_append_incremental_suffix_snapshot_streaming() {
    let mut content = String::new();
    let a = append_incremental_suffix(&mut content, "Hello");
    let b = append_incremental_suffix(&mut content, "Hello world");
    let c = append_incremental_suffix(&mut content, "Hello world");
    let d = append_incremental_suffix(&mut content, "Hello world!");

    assert_eq!(a, "Hello");
    assert_eq!(b, " world");
    assert_eq!(c, "");
    assert_eq!(d, "!");
    assert_eq!(content, "Hello world!");
}

#[test]
fn test_append_incremental_suffix_handles_unicode_char_boundaries() {
    let mut content = String::new();
    let first = append_incremental_suffix(&mut content, "•");
    let second = append_incremental_suffix(&mut content, "• item");

    assert_eq!(first, "•");
    assert_eq!(second, " item");
    assert_eq!(content, "• item");
}

#[test]
fn test_append_incremental_suffix_keeps_short_suffix_repeat_as_new_delta() {
    let mut content = "The contents of the file are:\n".to_string();
    let appended = append_incremental_suffix(&mut content, "are:\n");
    assert_eq!(appended, "are:\n");
    assert_eq!(content, "The contents of the file are:\nare:\n");
}

#[test]
fn test_prune_message_history_reanchors_to_user() {
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let executor = ToolOperator::new(std::path::PathBuf::from("."));
    let mut manager = ConversationManager::new(mock_api_client, executor);

    manager.api_messages = vec![
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("u0".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("a0".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("a1".to_string()),
        },
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("u1".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("a2".to_string()),
        },
    ];

    manager.prune_message_history(3);

    assert_eq!(manager.api_messages.len(), 2);
    assert_eq!(manager.api_messages[0].role, "user");
    assert_eq!(manager.api_messages[1].role, "assistant");
}

#[test]
fn test_prune_message_history_preserving_keeps_turn_user_anchor() {
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let mut manager = ConversationManager::new_mock(mock_api_client, HashMap::new());

    manager.api_messages = vec![
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("anchor user prompt".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("round 1".to_string()),
        },
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("tool_result read_file:\nA".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("round 2".to_string()),
        },
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("tool_result read_file:\nB".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("round 3".to_string()),
        },
    ];

    let anchor_index = 0usize;
    let new_anchor = manager.prune_message_history_preserving(4, anchor_index);
    assert_eq!(
        new_anchor, 0,
        "anchor should be retained at index 0 after pruning"
    );
    assert!(
        matches!(
            &manager.api_messages.first().map(|m| (&m.role, &m.content)),
            Some((role, Content::Text(text))) if role.as_str() == "user" && text == "anchor user prompt"
        ),
        "turn anchor user prompt must be preserved during loop pruning"
    );
}

#[test]
fn test_prune_message_history_preserving_allows_prune_when_anchor_is_far_behind() {
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let mut manager = ConversationManager::new_mock(mock_api_client, HashMap::new());

    manager.api_messages = vec![
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("anchor user prompt".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("a0".to_string()),
        },
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("u1".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("a1".to_string()),
        },
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("u2".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("a2".to_string()),
        },
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("u3".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("a3".to_string()),
        },
    ];

    let new_anchor = manager.prune_message_history_preserving(4, 0);
    assert_eq!(
        manager.api_messages.len(),
        4,
        "pruning should proceed when anchor is far behind target window"
    );
    assert_eq!(new_anchor, 0);
    assert_eq!(manager.api_messages[0].role, "user");
    match &manager.api_messages[0].content {
        Content::Text(text) => assert_eq!(text, "u2"),
        _ => panic!("expected user text content"),
    }
}

#[test]
fn test_upsert_turn_block_emits_padding_block_starts() {
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let mut manager = ConversationManager::new_mock(mock_api_client, HashMap::new());
    let (tx, mut rx) = mpsc::unbounded_channel();

    manager.upsert_turn_block(
        2,
        StreamBlock::ToolCall {
            id: "toolu_pad_01".to_string(),
            name: "read_file".to_string(),
            input: json!({"path": "file.txt"}),
            status: ToolStatus::Pending,
        },
        Some(&tx),
    );
    drop(tx);

    let mut started_indices = Vec::new();
    while let Ok(update) = rx.try_recv() {
        if let ConversationStreamUpdate::BlockStart { index, block } = update {
            started_indices.push(index);
            if index <= 1 {
                assert!(matches!(
                    block,
                    StreamBlock::Thinking {
                        content,
                        collapsed: true
                    } if content.is_empty()
                ));
            }
        }
    }

    assert_eq!(
        started_indices,
        vec![0, 1, 2],
        "padding blocks and target block must each emit BlockStart"
    );
}

#[test]
fn test_prune_message_history_clears_if_no_user_remains() {
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let executor = ToolOperator::new(std::path::PathBuf::from("."));
    let mut manager = ConversationManager::new(mock_api_client, executor);

    manager.api_messages = vec![
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("u0".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("a0".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("a1".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("a2".to_string()),
        },
    ];

    manager.prune_message_history(2);
    assert!(manager.api_messages.is_empty());
}

#[test]
fn test_prune_message_history_reanchors_even_if_it_reduces_below_limit() {
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let executor = ToolOperator::new(std::path::PathBuf::from("."));
    let mut manager = ConversationManager::new(mock_api_client, executor);

    manager.api_messages = vec![
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("u0".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("a0".to_string()),
        },
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("u1".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("a1".to_string()),
        },
    ];

    manager.prune_message_history(3);

    assert_eq!(manager.api_messages.len(), 2);
    assert_eq!(manager.api_messages[0].role, "user");
    if let Content::Text(text) = &manager.api_messages[0].content {
        assert_eq!(text, "u1");
    } else {
        panic!("expected user text content");
    }
}

#[test]
fn test_prune_message_history_skips_leading_tool_result_user_message() {
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let executor = ToolOperator::new(std::path::PathBuf::from("."));
    let mut manager = ConversationManager::new(mock_api_client, executor);

    manager.api_messages = vec![
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("u0".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Blocks(vec![ContentBlock::ToolUse {
                id: "tool_1".to_string(),
                name: "read_file".to_string(),
                input: json!({"path":"src/lib.rs"}),
            }]),
        },
        ApiMessage {
            role: "user".to_string(),
            content: Content::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: "tool_1".to_string(),
                content: "ok".to_string(),
                is_error: false,
            }]),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("a1".to_string()),
        },
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("u1".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("a2".to_string()),
        },
    ];

    manager.prune_message_history(4);

    assert_eq!(manager.api_messages.len(), 2);
    assert_eq!(manager.api_messages[0].role, "user");
    match &manager.api_messages[0].content {
        Content::Text(text) => assert_eq!(text, "u1"),
        _ => panic!("expected first retained message to be user text, not tool_result"),
    }
}

#[test]
fn test_prune_message_history_clears_if_only_tool_result_user_messages_remain() {
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let executor = ToolOperator::new(std::path::PathBuf::from("."));
    let mut manager = ConversationManager::new(mock_api_client, executor);

    manager.api_messages = vec![
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("a0".to_string()),
        },
        ApiMessage {
            role: "user".to_string(),
            content: Content::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: "tool_1".to_string(),
                content: "ok".to_string(),
                is_error: false,
            }]),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("a1".to_string()),
        },
    ];

    manager.prune_message_history(2);

    assert!(manager.api_messages.is_empty());
}
