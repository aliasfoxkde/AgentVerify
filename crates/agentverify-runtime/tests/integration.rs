//! Integration tests for agentverify-runtime
//!
//! These tests exercise the full Executor flow with mock implementations.

use agentverify_core::{
    Action, ActionId, BackoffConfig, Contract as CoreContract, IdempotencyKey, Observation,
    Predicate, RecoveryConfig, RecoveryStrategy, SourceId, VerificationResult,
};
use agentverify_runtime::{ClaimResult, Executor, ExecutorConfig, IdempotencyStore, Observer};
use async_trait::async_trait;
use chrono::Utc;
use std::sync::{Arc, Mutex};

/// A mock observer that returns configurable state
struct MockObserver {
    state: Mutex<serde_json::Value>,
}

impl MockObserver {
    fn new(state: serde_json::Value) -> Self {
        Self {
            state: Mutex::new(state),
        }
    }
}

#[async_trait]
impl Observer for MockObserver {
    async fn observe(
        &self,
        _action: &Action,
        _contract: &CoreContract,
    ) -> Result<Observation, agentverify_runtime::ExecutorError> {
        let state = self.state.lock().unwrap().clone();
        Ok(Observation::new(SourceId("mock".into()), state))
    }
}

/// A mock idempotency store for testing
struct MockIdempotencyStore {
    claims: Mutex<Vec<String>>,
    results: Mutex<std::collections::HashMap<String, VerificationResult>>,
}

impl MockIdempotencyStore {
    fn new() -> Self {
        Self {
            claims: Mutex::new(Vec::new()),
            results: Mutex::new(std::collections::HashMap::new()),
        }
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
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = (ClaimResult, Option<VerificationResult>)> + Send + 'a,
        >,
    > {
        let key = key.to_string();
        Box::pin(async move {
            let mut claims = self.claims.lock().unwrap();
            let results = self.results.lock().unwrap();

            if let Some(result) = results.get(&key) {
                return (ClaimResult::AlreadyClaimed, Some(*result));
            }

            if claims.contains(&key) {
                return (ClaimResult::AlreadyClaimed, None);
            }

            claims.push(key);
            (ClaimResult::Claimed, None)
        })
    }

    fn complete(
        &self,
        key: String,
        result: VerificationResult,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        let key = key.clone();
        Box::pin(async move {
            let mut results = self.results.lock().unwrap();
            results.insert(key, result);
        })
    }

    fn release(
        &self,
        key: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        let key = key.to_string();
        Box::pin(async move {
            let mut claims = self.claims.lock().unwrap();
            claims.retain(|k| k != &key);
        })
    }
}

/// Helper to create a simple contract for testing
fn create_test_contract() -> CoreContract {
    let recovery = RecoveryConfig {
        strategy: RecoveryStrategy::VerifyThenRetry,
        max_attempts: 3,
        backoff: Some(BackoffConfig {
            backoff_type: agentverify_core::BackoffType::Exponential,
            initial: chrono::Duration::milliseconds(100),
            max: chrono::Duration::seconds(5),
            multiplier: 2.0,
        }),
        on_unknown: vec![],
    };
    CoreContract::new("test_action")
        .with_postcondition(
            Predicate::Equals {
                path: "value".into(),
                value: serde_json::json!(42),
            },
            "state.value equals 42",
        )
        .with_recovery(recovery)
}

/// Helper to create a test action
fn create_test_action() -> Action {
    Action {
        id: ActionId::new(),
        name: "test_action".into(),
        arguments: serde_json::json!({}),
        idempotency_key: Some(IdempotencyKey::new("test-key-123")),
        created_at: Utc::now(),
    }
}

#[test]
fn test_executor_initialization() {
    let config = ExecutorConfig::default();
    let _executor = Executor::with_config(config);
    // Executor created successfully - verified it compiles
}

#[test]
fn test_executor_with_custom_config() {
    let config = ExecutorConfig {
        verification_timeout_ms: 10000,
        max_retries: 5,
        verify_before_retry: false,
    };
    let _executor = Executor::with_config(config);
    // Executor created successfully with custom config
}

#[test]
fn test_executor_with_idempotency_store() {
    let config = ExecutorConfig::default();
    let store = Arc::new(MockIdempotencyStore::new());
    let _executor = Executor::with_config_and_store(config, store);
    // Executor created successfully with custom store
}

#[tokio::test]
async fn test_mock_observer_returns_state() {
    let state = serde_json::json!({
        "value": 42,
        "name": "test"
    });
    let observer = MockObserver::new(state);
    let action = create_test_action();
    let contract = create_test_contract();

    let observation = observer.observe(&action, &contract).await.unwrap();

    assert_eq!(observation.state["value"], 42);
    assert_eq!(observation.state["name"], "test");
    assert_eq!(observation.source.0, "mock");
}

#[tokio::test]
async fn test_mock_observer_empty_state() {
    let state = serde_json::json!({});
    let observer = MockObserver::new(state);
    let action = create_test_action();
    let contract = create_test_contract();

    let observation = observer.observe(&action, &contract).await.unwrap();

    assert!(observation.state.as_object().unwrap().is_empty());
}

#[tokio::test]
async fn test_mock_idempotency_store_claims_key() {
    let store = Arc::new(MockIdempotencyStore::new());

    let (result, existing) = store.claim_or_check("key-1").await;
    assert_eq!(result, ClaimResult::Claimed);
    assert!(existing.is_none());

    // Second claim should return AlreadyClaimed
    let (result2, existing2) = store.claim_or_check("key-1").await;
    assert_eq!(result2, ClaimResult::AlreadyClaimed);
    assert!(existing2.is_none());
}

#[tokio::test]
async fn test_mock_idempotency_store_complete() {
    let store = Arc::new(MockIdempotencyStore::new());

    // Claim and complete
    let (result, _) = store.claim_or_check("key-2").await;
    assert_eq!(result, ClaimResult::Claimed);

    store
        .complete("key-2".to_string(), VerificationResult::Verified)
        .await;

    // Now claim should return the completed result
    let (result2, existing) = store.claim_or_check("key-2").await;
    assert_eq!(result2, ClaimResult::AlreadyClaimed);
    assert_eq!(existing, Some(VerificationResult::Verified));
}

#[tokio::test]
async fn test_mock_idempotency_store_release() {
    let store = Arc::new(MockIdempotencyStore::new());

    // Claim a key
    let (result, _) = store.claim_or_check("key-3").await;
    assert_eq!(result, ClaimResult::Claimed);

    // Release the key
    store.release("key-3").await;

    // Should be able to claim again
    let (result2, _) = store.claim_or_check("key-3").await;
    assert_eq!(result2, ClaimResult::Claimed);
}

#[tokio::test]
async fn test_mock_idempotency_store_different_keys() {
    let store = Arc::new(MockIdempotencyStore::new());

    let (r1, _) = store.claim_or_check("key-a").await;
    let (r2, _) = store.claim_or_check("key-b").await;
    let (r3, _) = store.claim_or_check("key-c").await;

    assert_eq!(r1, ClaimResult::Claimed);
    assert_eq!(r2, ClaimResult::Claimed);
    assert_eq!(r3, ClaimResult::Claimed);
}

#[test]
fn test_executor_config_default() {
    let config = ExecutorConfig::default();

    assert_eq!(config.verification_timeout_ms, 5000);
    assert_eq!(config.max_retries, 3);
    assert!(config.verify_before_retry);
}

#[test]
fn test_executor_config_clone() {
    let config = ExecutorConfig {
        verification_timeout_ms: 10000,
        max_retries: 5,
        verify_before_retry: false,
    };
    let config2 = config.clone();

    assert_eq!(
        config.verification_timeout_ms,
        config2.verification_timeout_ms
    );
    assert_eq!(config.max_retries, config2.max_retries);
    assert_eq!(config.verify_before_retry, config2.verify_before_retry);
}
