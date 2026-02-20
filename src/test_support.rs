use std::sync::Mutex;

/// Process-wide lock for tests that mutate environment variables.
pub static ENV_LOCK: Mutex<()> = Mutex::new(());
