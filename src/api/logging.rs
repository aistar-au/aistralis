use serde_json::Value;
use std::fs::OpenOptions;
use std::io::{IsTerminal, Write};

const DEFAULT_API_LOG_PATH: &str = "/tmp/vex-debug-payload.log";
const DEBUG_PAYLOAD_ENV: &str = "VEX_DEBUG_PAYLOAD";
const API_LOG_PATH_ENV: &str = "VEX_API_LOG_PATH";

pub fn debug_payload_enabled() -> bool {
    std::env::var(DEBUG_PAYLOAD_ENV)
        .ok()
        .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

pub fn emit_debug_payload(request_url: &str, payload: &Value) {
    let formatted_payload = serde_json::to_string_pretty(payload)
        .unwrap_or_else(|_| "<payload serialization error>".to_string());
    let message =
        format!("VEX_API DEBUG payload_request url={request_url}\npayload:\n{formatted_payload}\n");
    emit_log_message(&message);
}

pub fn emit_sse_parse_error(
    event_type: Option<&str>,
    json_data: &str,
    parse_error: &serde_json::Error,
) {
    let message = format!(
        "VEX_API ERROR sse_parse_failed error={parse_error}\nevent_type={}\ndata:\n{json_data}\n",
        event_type.unwrap_or("<none>")
    );
    emit_log_message(&message);
}

fn emit_log_message(message: &str) {
    if let Some(path) = resolve_log_path() {
        if append_log_file(&path, message).is_ok() {
            return;
        }
    }

    eprintln!("{message}");
}

fn resolve_log_path() -> Option<String> {
    std::env::var(API_LOG_PATH_ENV)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .or_else(|| {
            if std::io::stderr().is_terminal() {
                Some(DEFAULT_API_LOG_PATH.to_string())
            } else {
                None
            }
        })
}

fn append_log_file(path: &str, message: &str) -> std::io::Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(message.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_debug_payload_enabled_accepts_true_variants() {
        let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
        std::env::set_var(DEBUG_PAYLOAD_ENV, "1");
        assert!(debug_payload_enabled());
        std::env::set_var(DEBUG_PAYLOAD_ENV, "TRUE");
        assert!(debug_payload_enabled());
        std::env::remove_var(DEBUG_PAYLOAD_ENV);
    }

    #[test]
    fn test_resolve_log_path_uses_api_log_path() {
        let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
        std::env::set_var(API_LOG_PATH_ENV, "/tmp/test-api.log");
        assert_eq!(resolve_log_path().as_deref(), Some("/tmp/test-api.log"));
        std::env::remove_var(API_LOG_PATH_ENV);
    }
}
