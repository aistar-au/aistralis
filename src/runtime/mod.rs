pub mod context;
pub mod event;
pub mod frontend;
pub mod r#loop;
pub mod mode;

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
            context::RuntimeContext, event::RuntimeEvent, frontend::FrontendAdapter,
            mode::RuntimeMode,
        };

        fn _uses_runtime_mode_trait<T: RuntimeMode>() {}
        fn _uses_frontend_adapter_trait<T: FrontendAdapter>() {}

        struct DummyMode;
        impl RuntimeMode for DummyMode {
            fn on_user_input(&mut self, _input: String, _ctx: &mut RuntimeContext) {}
            fn on_model_update(
                &mut self,
                _update: crate::app::UiUpdate,
                _ctx: &mut RuntimeContext,
            ) {
            }
            fn is_turn_in_progress(&self) -> bool {
                false
            }
        }

        struct DummyFrontend;
        impl FrontendAdapter for DummyFrontend {
            fn poll_user_input(&mut self) -> Option<String> {
                None
            }
            fn render<M: RuntimeMode>(&mut self, _mode: &M) {}
            fn should_quit(&self) -> bool {
                true
            }
        }

        let _ = std::mem::size_of::<RuntimeEvent>();
        let _ = std::mem::size_of::<Option<RuntimeContext<'static>>>();
        let _ = _uses_runtime_mode_trait::<DummyMode>;
        let _ = _uses_frontend_adapter_trait::<DummyFrontend>;
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
