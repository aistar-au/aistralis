pub mod context;
pub mod event;
pub mod frontend;
pub mod mode;
pub mod r#loop;

pub fn parse_bool_flag(value: String) -> Option<bool> {
    parse_bool_str(value.as_str())
}

pub fn parse_bool_str(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

pub fn is_local_endpoint_url(url: &str) -> bool {
    let normalized = url.trim().to_ascii_lowercase();
    normalized.starts_with("http://localhost")
        || normalized.starts_with("https://localhost")
        || normalized.starts_with("http://127.0.0.1")
        || normalized.starts_with("https://127.0.0.1")
        || normalized.starts_with("http://0.0.0.0")
        || normalized.starts_with("https://0.0.0.0")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_bool_helpers() {
        assert_eq!(parse_bool_str("true"), Some(true));
        assert_eq!(parse_bool_str("0"), Some(false));
        assert_eq!(parse_bool_flag("YES".to_string()), Some(true));
        assert_eq!(parse_bool_flag("off".to_string()), Some(false));
        assert_eq!(parse_bool_str("maybe"), None);
    }


    #[test]
    fn test_ref_02_runtime_types_compile() {
        use crate::runtime::{
            context::RuntimeContext,
            event::RuntimeEvent,
            frontend::FrontendAdapter,
            mode::RuntimeMode,
        };
        // Zero-cost existence check — if the module tree compiles, this passes.
        let _ = std::mem::size_of::<RuntimeEvent>();
    }

    #[test]
    fn test_is_local_endpoint_url_normalizes_case_and_space() {
        assert!(is_local_endpoint_url(" HTTP://LOCALHOST:8000/v1/messages "));
        assert!(is_local_endpoint_url("https://127.0.0.1/v1/messages"));
        assert!(!is_local_endpoint_url(
            "https://api.anthropic.com/v1/messages"
        ));
    }
}
