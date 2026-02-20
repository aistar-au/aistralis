use std::sync::Mutex;
use tokio::sync::Mutex as AsyncMutex;

/// Process-wide lock for tests that mutate environment variables.
pub static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Async-aware process-wide lock for async tests that mutate environment variables.
pub static ASYNC_ENV_LOCK: AsyncMutex<()> = AsyncMutex::const_new(());
