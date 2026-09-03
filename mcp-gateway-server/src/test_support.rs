//! Shared helpers for the unit test suite.

/// Serialises every test that reads or writes `JWT_SECRET`.
///
/// The secret is process-global and it keys the API-key cipher
/// ([`crate::api::api_keys`]). `cargo test` runs tests concurrently, so without
/// a shared lock one test's `set_var` lands between another's encrypt and
/// decrypt and fails it. The lock is crate-wide rather than per-module because
/// anything else that comes to depend on the secret has to serialise with it
/// too.
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

/// Serialises every test that touches `TEST_DATABASE_URL`.
///
/// They all point at one scratch database and all start by emptying it, so
/// concurrently they delete each other's fixtures mid-test. The failure is a
/// missing row in an unrelated assertion, which reads as a broken fix rather
/// than as a broken harness.
pub static DB_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Acquire the database lock from a `#[tokio::test]`.
pub async fn lock_db() -> tokio::sync::MutexGuard<'static, ()> {
    DB_LOCK.lock().await
}
