//! `RecoveryExecutor` and the circuit-breaker gate it consults.
//!
//! The executor only reaches its fallbacks when the breaker it was given
//! allows the attempt. These tests drive that gate from the outside — a closed
//! breaker, a breaker that has recovered past its timeout, and a closed
//! breaker with no fallback left to try — so the decision the executor makes
//! is observed through the operation itself.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use agentverify_core::VerificationResult;
use agentverify_recovery::{
    CircuitBreaker, CircuitBreakerConfig, CircuitState, RecoveryError, RecoveryExecutor,
    RecoveryOutcome, RecoveryStrategyEnum, RetryStrategy,
};
use chrono::Duration;

/// A breaker with the given thresholds, still closed.
fn breaker(config: CircuitBreakerConfig) -> CircuitBreaker {
    CircuitBreaker::new(config)
}

/// A breaker that opens after a single failure and stays open for `timeout`.
fn single_failure_breaker(timeout: Duration) -> Arc<CircuitBreaker> {
    Arc::new(breaker(CircuitBreakerConfig {
        failure_threshold: 1,
        success_threshold: 1,
        recovery_timeout: timeout,
    }))
}

/// The outcome produced by the `attempt`-th call, counting from zero: the
/// first attempt is ambiguous, every later one succeeds.
fn outcome_of_attempt(attempt: u32) -> VerificationResult {
    if attempt == 0 {
        VerificationResult::Unknown
    } else {
        VerificationResult::Verified
    }
}

/// The operation the executor drives: ambiguous on its first attempt, so only
/// a fallback can produce a success.
#[allow(clippy::type_complexity)]
fn counting_operation(
    counter: Arc<AtomicU32>,
) -> impl Fn() -> Pin<Box<dyn Future<Output = Result<VerificationResult, RecoveryError>> + Send>> {
    move || {
        let attempt = counter.fetch_add(1, Ordering::SeqCst);
        let result = outcome_of_attempt(attempt);
        Box::pin(async move { Ok::<VerificationResult, RecoveryError>(result) })
    }
}

#[tokio::test]
async fn a_closed_circuit_breaker_lets_the_fallback_run() {
    let cb = Arc::new(breaker(CircuitBreakerConfig {
        failure_threshold: 5,
        success_threshold: 1,
        recovery_timeout: Duration::seconds(30),
    }));
    let calls = Arc::new(AtomicU32::new(0));

    let executor = RecoveryExecutor::new(RecoveryStrategyEnum::Retry(
        RetryStrategy::with_default_backoff(1),
    ))
    .with_fallback(RecoveryStrategyEnum::Retry(
        RetryStrategy::with_default_backoff(2),
    ))
    .with_circuit_breaker(Arc::clone(&cb));

    let outcome = executor.execute(counting_operation(calls.clone())).await;

    assert!(
        matches!(
            outcome,
            RecoveryOutcome::Success(VerificationResult::Verified)
        ),
        "unexpected outcome: {outcome:?}"
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "the fallback ran after the primary"
    );
    // A closed breaker neither opened nor recorded a failure: the attempt was
    // simply allowed.
    assert_eq!(cb.state(), CircuitState::Closed);
}

#[tokio::test]
async fn a_recovered_breaker_admits_the_attempt_through_the_executor() {
    let cb = single_failure_breaker(Duration::milliseconds(30));
    cb.record_failure();
    assert_eq!(cb.state(), CircuitState::Open);
    assert!(!cb.is_allowed(), "the breaker is open before its timeout");

    let executor = RecoveryExecutor::new(RecoveryStrategyEnum::Retry(
        RetryStrategy::with_default_backoff(1),
    ))
    .with_fallback(RecoveryStrategyEnum::Retry(
        RetryStrategy::with_default_backoff(1),
    ))
    .with_circuit_breaker(Arc::clone(&cb));

    // The first attempt is refused outright.
    let blocked_calls = Arc::new(AtomicU32::new(0));
    let blocked = executor
        .execute(counting_operation(Arc::clone(&blocked_calls)))
        .await;
    assert!(
        matches!(
            blocked,
            RecoveryOutcome::Failure(RecoveryError::CircuitBreakerOpen)
        ),
        "unexpected outcome: {blocked:?}"
    );
    assert_eq!(
        blocked_calls.load(Ordering::SeqCst),
        1,
        "a blocked attempt never runs the operation"
    );

    tokio::time::sleep(tokio::time::Duration::from_millis(45)).await;

    // Once the timeout has elapsed the same executor is admitted, and its
    // fallback is what answers.
    let admitted_calls = Arc::new(AtomicU32::new(0));
    let admitted = executor
        .execute(counting_operation(Arc::clone(&admitted_calls)))
        .await;
    assert!(
        matches!(
            admitted,
            RecoveryOutcome::Success(VerificationResult::Verified)
        ),
        "unexpected outcome: {admitted:?}"
    );
    assert_eq!(
        cb.state(),
        CircuitState::HalfOpen,
        "admission moves the breaker to half-open"
    );
    assert_eq!(
        admitted_calls.load(Ordering::SeqCst),
        2,
        "a primary and then a fallback"
    );
}

#[tokio::test]
async fn a_closed_breaker_does_not_mask_the_primary_failure() {
    let cb = Arc::new(breaker(CircuitBreakerConfig {
        failure_threshold: 5,
        success_threshold: 1,
        recovery_timeout: Duration::seconds(30),
    }));

    let executor = RecoveryExecutor::new(RecoveryStrategyEnum::Retry(
        RetryStrategy::with_default_backoff(2),
    ))
    .with_circuit_breaker(cb);

    let outcome = executor
        .execute(|| {
            Box::pin(async { Ok::<VerificationResult, RecoveryError>(VerificationResult::Unknown) })
        })
        .await;

    // With no fallback to try, the primary's own failure is what surfaces.
    assert!(
        matches!(
            outcome,
            RecoveryOutcome::Failure(RecoveryError::MaxAttemptsExceeded { attempts: 2 })
        ),
        "unexpected outcome: {outcome:?}"
    );
}
