pub mod context;
pub mod event;
pub mod frontend;
pub mod r#loop;
pub mod mode;
pub mod update;

pub use update::UiUpdate;

#[cfg(test)]
mod tests {

    #[test]
    fn test_parse_bool_helpers() {
        assert_eq!(crate::util::parse_bool_str("true"), Some(true));
        assert_eq!(crate::util::parse_bool_str("0"), Some(false));
        assert_eq!(crate::util::parse_bool_flag("YES".to_string()), Some(true));
        assert_eq!(crate::util::parse_bool_flag("off".to_string()), Some(false));
        assert_eq!(crate::util::parse_bool_str("maybe"), None);
    }

    #[test]
    fn test_ref_02_runtime_types_compile() {
        use crate::runtime::{
            context::RuntimeContext,
            event::RuntimeEvent,
            frontend::{FrontendAdapter, UserInputEvent},
            mode::RuntimeMode,
        };

        fn _uses_runtime_mode_trait<T: RuntimeMode>() {}
        fn _uses_frontend_adapter_trait<T: FrontendAdapter<DummyMode>>() {}

        struct DummyMode;
        impl RuntimeMode for DummyMode {
            fn on_user_input(&mut self, _input: String, _ctx: &mut RuntimeContext) {}
            fn on_model_update(
                &mut self,
                _update: crate::runtime::UiUpdate,
                _ctx: &mut RuntimeContext,
            ) {
            }
            fn is_turn_in_progress(&self) -> bool {
                false
            }
        }

        struct DummyFrontend;
        impl FrontendAdapter<DummyMode> for DummyFrontend {
            fn poll_user_input(&mut self, _mode: &DummyMode) -> Option<UserInputEvent> {
                None
            }
            fn render(&mut self, _mode: &DummyMode) {}
            fn should_quit(&self) -> bool {
                true
            }
        }

        let _ = std::mem::size_of::<RuntimeEvent>();
        let _ = std::mem::size_of::<Option<RuntimeContext>>();
        let _ = _uses_runtime_mode_trait::<DummyMode>;
        let _ = _uses_frontend_adapter_trait::<DummyFrontend>;
    }

    #[test]
    fn test_is_local_endpoint_url_normalizes_case_and_space() {
        assert!(crate::util::is_local_endpoint_url(
            " HTTP://LOCALHOST:8000/v1/messages "
        ));
        assert!(crate::util::is_local_endpoint_url(
            "https://127.0.0.1/v1/messages"
        ));
        assert!(!crate::util::is_local_endpoint_url(
            "https://api.anthropic.com/v1/messages"
        ));
    }
}
