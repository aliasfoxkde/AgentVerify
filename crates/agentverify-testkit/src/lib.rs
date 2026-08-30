//! `AgentVerify` testkit - testing utilities for `AgentVerify` crates
//!
//! This crate provides mock implementations, test helpers, and testing utilities
//! for writing tests against `AgentVerify` components.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
use agentverify_core::VerificationResult;
use agentverify_runtime::{ClaimResult, IdempotencyStore};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

/// Result type for `claim_or_check` operations
type ClaimCheckResult = (ClaimResult, Option<VerificationResult>);

/// A mock implementation of [`IdempotencyStore`] for testing.
///
/// This mock allows you to:
/// - Record all calls to `claim_or_check`, `complete`, and `release`
/// - Configure predefined results for specific keys
/// - Inspect call history after test execution
///
/// # Example
///
/// ```
/// use agentverify_testkit::MockIdempotencyStore;
/// use agentverify_runtime::ClaimResult;
///
/// let mut store = MockIdempotencyStore::new();
/// store.set_result("key", (ClaimResult::Claimed, None));
///
/// // Use in tests (requires async context, e.g., #[tokio::test])
/// // let (result, opt) = store.claim_or_check("key").await;
/// // assert_eq!(result, ClaimResult::Claimed);
///
/// // Check call history
/// assert_eq!(store.claim_or_check_calls().len(), 0);
/// ```
#[derive(Debug, Clone)]
pub struct MockIdempotencyStore {
    /// Results to return for specific keys
    results: Arc<Mutex<HashMap<String, ClaimCheckResult>>>,
    /// Call history for `claim_or_check`
    claim_or_check_calls: Arc<Mutex<Vec<String>>>,
    /// Call history for complete
    complete_calls: Arc<Mutex<Vec<(String, VerificationResult)>>>,
    /// Call history for release
    release_calls: Arc<Mutex<Vec<String>>>,
    /// Default result when no specific key is configured
    default_result: Arc<Mutex<Option<ClaimCheckResult>>>,
    /// Whether to return `AlreadyClaimed` on second claim of same key
    return_already_claimed: Arc<Mutex<bool>>,
}

impl MockIdempotencyStore {
    /// Create a new empty mock store
    #[must_use]
    pub fn new() -> Self {
        Self {
            results: Arc::new(Mutex::new(HashMap::new())),
            claim_or_check_calls: Arc::new(Mutex::new(Vec::new())),
            complete_calls: Arc::new(Mutex::new(Vec::new())),
            release_calls: Arc::new(Mutex::new(Vec::new())),
            default_result: Arc::new(Mutex::new(None)),
            return_already_claimed: Arc::new(Mutex::new(true)),
        }
    }

    /// Set the result for a specific key
    ///
    /// When `claim_or_check` is called with this key, it will return
    /// the configured `(ClaimResult, Option<VerificationResult>)`.
    pub fn set_result(
        &mut self,
        key: impl Into<String>,
        result: (ClaimResult, Option<VerificationResult>),
    ) {
        self.results
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(key.into(), result);
    }

    /// Set the default result for any unconfigured key
    ///
    /// If no specific key result is configured, this default is returned.
    pub fn set_default_result(&mut self, result: (ClaimResult, Option<VerificationResult>)) {
        *self
            .default_result
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(result);
    }

    /// Configure whether to return `AlreadyClaimed` when the same key
    /// is claimed twice (simulating in-flight state).
    ///
    /// Default is `true`.
    pub fn set_return_already_claimed(&mut self, value: bool) {
        *self
            .return_already_claimed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = value;
    }

    /// Returns the number of times `claim_or_check` was called
    #[must_use]
    pub fn claim_or_check_call_count(&self) -> usize {
        self.claim_or_check_calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    /// Returns the list of keys passed to `claim_or_check`
    #[must_use]
    pub fn claim_or_check_calls(&self) -> Vec<String> {
        self.claim_or_check_calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Returns the number of times `complete` was called
    #[must_use]
    pub fn complete_call_count(&self) -> usize {
        self.complete_calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    /// Returns the list of `(key, result)` pairs passed to `complete`
    #[must_use]
    pub fn complete_calls(&self) -> Vec<(String, VerificationResult)> {
        self.complete_calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Returns the number of times `release` was called
    #[must_use]
    pub fn release_call_count(&self) -> usize {
        self.release_calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    /// Returns the list of keys passed to `release`
    #[must_use]
    pub fn release_calls(&self) -> Vec<String> {
        self.release_calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Reset all call history
    pub fn reset_calls(&self) {
        self.claim_or_check_calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        self.complete_calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        self.release_calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }

    /// Reset all state including configured results
    pub fn reset_all(&self) {
        self.results
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        *self
            .default_result
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        *self
            .return_already_claimed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        self.reset_calls();
    }
}

impl Default for MockIdempotencyStore {
    fn default() -> Self {
        Self::new()
    }
}

impl IdempotencyStore for MockIdempotencyStore {
    fn claim_or_check<'a>(
        &'a self,
        key: &'a str,
    ) -> Pin<Box<dyn Future<Output = (ClaimResult, Option<VerificationResult>)> + Send + 'a>> {
        let results = Arc::clone(&self.results);
        let default_result = Arc::clone(&self.default_result);
        let return_already_claimed = Arc::clone(&self.return_already_claimed);
        let claim_or_check_calls = Arc::clone(&self.claim_or_check_calls);

        Box::pin(async move {
            // Record the call
            claim_or_check_calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(key.to_string());

            // Check for configured result
            let result = {
                let results_guard = results
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let return_already = *return_already_claimed
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);

                if let Some(result) = results_guard.get(key) {
                    // If this key was already claimed and we should return AlreadyClaimed,
                    // check if it's a second call
                    if return_already && key.contains("_claimed") {
                        // Simulate: first call returns Claimed, second returns AlreadyClaimed
                        let already_result = results_guard.get(&format!("{key}_already"));
                        if let Some(r) = already_result {
                            return r.clone();
                        }
                        // Default: return AlreadyClaimed with None (in-flight)
                        return (ClaimResult::AlreadyClaimed, None);
                    }
                    result.clone()
                } else if let Some(default) = &*default_result
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                {
                    default.clone()
                } else {
                    // No configured result: return Claimed by default
                    (ClaimResult::Claimed, None)
                }
            };

            result
        })
    }

    fn complete(
        &self,
        key: String,
        result: VerificationResult,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let complete_calls = Arc::clone(&self.complete_calls);

        Box::pin(async move {
            complete_calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((key, result));
        })
    }

    fn release(&self, key: &str) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let release_calls = Arc::clone(&self.release_calls);
        let key_str = key.to_string();

        Box::pin(async move {
            release_calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(key_str);
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_idempotency_store_records_claim_calls() {
        let mut store = MockIdempotencyStore::new();
        store.set_result("key1", (ClaimResult::Claimed, None));

        store.claim_or_check("key1").await;
        store.claim_or_check("key2").await;

        assert_eq!(store.claim_or_check_call_count(), 2);
        assert_eq!(store.claim_or_check_calls(), vec!["key1", "key2"]);
    }

    #[tokio::test]
    async fn mock_idempotency_store_records_complete_calls() {
        let store = MockIdempotencyStore::new();

        store
            .complete("key1".to_string(), VerificationResult::Verified)
            .await;
        store
            .complete("key2".to_string(), VerificationResult::Failed)
            .await;

        assert_eq!(store.complete_call_count(), 2);
        assert_eq!(
            store.complete_calls(),
            vec![
                ("key1".to_string(), VerificationResult::Verified),
                ("key2".to_string(), VerificationResult::Failed),
            ]
        );
    }

    #[tokio::test]
    async fn mock_idempotency_store_records_release_calls() {
        let store = MockIdempotencyStore::new();

        store.release("key1").await;
        store.release("key2").await;

        assert_eq!(store.release_call_count(), 2);
        assert_eq!(store.release_calls(), vec!["key1", "key2"]);
    }

    #[tokio::test]
    async fn mock_idempotency_store_returns_configured_result() {
        let mut store = MockIdempotencyStore::new();
        store.set_result(
            "test-key",
            (
                ClaimResult::AlreadyClaimed,
                Some(VerificationResult::Verified),
            ),
        );

        let (result, verification) = store.claim_or_check("test-key").await;

        assert_eq!(result, ClaimResult::AlreadyClaimed);
        assert_eq!(verification, Some(VerificationResult::Verified));
    }

    #[tokio::test]
    async fn mock_idempotency_store_returns_default_when_no_key_configured() {
        let mut store = MockIdempotencyStore::new();
        store.set_default_result((ClaimResult::Claimed, None));

        let (result, verification) = store.claim_or_check("unknown-key").await;

        assert_eq!(result, ClaimResult::Claimed);
        assert_eq!(verification, None);
    }

    #[tokio::test]
    async fn mock_idempotency_store_reset_clears_calls() {
        let store = MockIdempotencyStore::new();

        store.claim_or_check("key1").await;
        store
            .complete("key1".to_string(), VerificationResult::Verified)
            .await;
        store.release("key1").await;

        assert_eq!(store.claim_or_check_call_count(), 1);
        assert_eq!(store.complete_call_count(), 1);
        assert_eq!(store.release_call_count(), 1);

        store.reset_calls();

        assert_eq!(store.claim_or_check_call_count(), 0);
        assert_eq!(store.complete_call_count(), 0);
        assert_eq!(store.release_call_count(), 0);
    }

    #[tokio::test]
    async fn mock_idempotency_store_reset_all_clears_everything() {
        let mut store = MockIdempotencyStore::new();
        store.set_result(
            "key1",
            (
                ClaimResult::AlreadyClaimed,
                Some(VerificationResult::Verified),
            ),
        );
        store.set_default_result((ClaimResult::Claimed, None));

        store.claim_or_check("key1").await;

        store.reset_all();

        // After reset_all, no results are configured, so it returns default Claimed
        let (result, _) = store.claim_or_check("key1").await;
        assert_eq!(result, ClaimResult::Claimed);
    }
}
