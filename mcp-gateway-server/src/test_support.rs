//! Shared helpers for the unit test suite.

/// Serialises every test that reads or writes `JWT_SECRET`.
///
/// The secret is process-global, and it keys the API-key cipher
/// ([`crate::api::api_keys`]) as well as the configuration-transfer round trip
/// ([`crate::api::config_transfer`]). Tests in *both* modules set it, and
/// `cargo test` runs them concurrently, so without a single shared lock one
/// test's `set_var` lands between another's encrypt and decrypt and fails it.
/// A per-module lock is not enough — the lock has to be crate-wide, which is
/// why this lives here rather than next to either test module.
///
/// It is a tokio mutex because the database tests hold it across `.await`
/// points; a blocking guard there would park a runtime worker thread.
pub static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Acquire the lock from a synchronous test (one running outside a runtime).
pub fn lock_env() -> tokio::sync::MutexGuard<'static, ()> {
    ENV_LOCK.blocking_lock()
}

/// Acquire the lock from a `#[tokio::test]`, where `blocking_lock` would panic.
pub async fn lock_env_async() -> tokio::sync::MutexGuard<'static, ()> {
    ENV_LOCK.lock().await
}
