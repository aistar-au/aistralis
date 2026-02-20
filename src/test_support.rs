use tokio::sync::Mutex as AsyncMutex;

/// Process-wide lock for tests that mutate environment variables.
/// Use `.blocking_lock()` in sync tests and `.lock().await` in async tests.
pub static ENV_LOCK: AsyncMutex<()> = AsyncMutex::const_new(());
