use vexcoder::config::Config;

#[test]
fn test_config_validation_rejects_invalid_models_for_remote_api() {
    let config = Config {
        api_key: Some("test-key".to_string()),
        model: "local/mock-model".to_string(),
        api_url: "https://api.anthropic.com/v1/messages".to_string(),
        anthropic_version: "2023-06-01".to_string(),
        working_dir: std::env::current_dir().expect("cwd"),
    };

    assert!(config.validate().is_err());
}

#[test]
fn test_config_validation_allows_local_endpoint_without_api_key() {
    let config = Config {
        api_key: None,
        model: "local/llama3.3".to_string(),
        api_url: "http://localhost:8000/v1/messages".to_string(),
        anthropic_version: "2023-06-01".to_string(),
        working_dir: std::env::current_dir().expect("cwd"),
    };

    assert!(config.validate().is_ok());
}
