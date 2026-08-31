//! `AgentVerify` Recovery
//!
//! Recovery strategies for handling verification failures and timeouts.
//!
//! This crate provides recovery mechanisms when verification cannot be completed
//! or when postconditions are not met:
//!
//! - **Retry strategies** - configurable backoff and retry limits
//! - **Fallback strategies** - alternative actions when primary fails
//! - **Circuit breaker strategies** - prevent cascading failures
//! - **Timeout strategies** - handle long-running operations
//!
//! # Core Principle
//!
//! UNKNOWN is a first-class state. A timeout does NOT equal failure.
//! Recovery should always prefer verification over assumption.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
use agentverify_core::VerificationResult;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, PoisonError};
use thiserror::Error;

/// Type alias for boxed future
type BoxFuture = Pin<Box<dyn Future<Output = Result<VerificationResult, RecoveryError>> + Send>>;

/// A closure that produces a future
pub type FutureFactory = Arc<dyn Fn() -> BoxFuture + Send + Sync>;

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during recovery
#[derive(Debug, Clone, Error)]
pub enum RecoveryError {
    /// Maximum retry attempts exceeded
    #[error("Maximum retry attempts ({attempts}) exceeded")]
    MaxAttemptsExceeded {
        /// Number of attempts that were made
        attempts: u32,
    },

    /// Circuit breaker is open
    #[error("Circuit breaker is open, retry not allowed")]
    CircuitBreakerOpen,

    /// Timeout exceeded
    #[error("Operation timed out after {duration}")]
    Timeout {
        /// How long the operation ran before giving up
        duration: Duration,
    },

    /// All fallbacks exhausted
    #[error("All fallback strategies exhausted")]
    AllFallbacksExhausted,

    /// Recovery not applicable for the result
    #[error("Recovery not applicable for result: {result}")]
    NotApplicable {
        /// The verification result recovery cannot handle
        result: VerificationResult,
    },

    /// Underlying error during recovery
    #[error("Recovery failed: {context}")]
    UnderlyingError {
        /// Description of the underlying failure
        context: String,
    },
}

// ============================================================================
// Backoff Configuration
// ============================================================================

/// Backoff type for retry strategies
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BackoffType {
    /// Fixed delay between retries
    Fixed,
    /// Linear increase in delay
    Linear,
    /// Exponential increase in delay
    #[default]
    Exponential,
}

/// Configuration for backoff between retries
#[derive(Debug, Clone)]
pub struct Backoff {
    /// Backoff type
    pub backoff_type: BackoffType,
    /// Initial delay
    pub initial: Duration,
    /// Maximum delay
    pub max: Duration,
    /// Multiplier for exponential/linear backoff
    pub multiplier: f64,
}

impl Backoff {
    /// Create a new backoff configuration
    #[must_use]
    pub fn new(backoff_type: BackoffType, initial: Duration, max: Duration) -> Self {
        Self {
            backoff_type,
            initial,
            max,
            multiplier: 2.0,
        }
    }

    /// Create with custom multiplier
    #[must_use]
    pub fn with_multiplier(mut self, multiplier: f64) -> Self {
        self.multiplier = multiplier;
        self
    }

    /// Calculate the delay for a given attempt number (0-indexed)
    ///
    /// Delay arithmetic uses floating point multipliers over small millisecond
    /// values and is clamped to `max`, so the lossy casts are intentional.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_possible_wrap
    )]
    #[must_use]
    pub fn calculate_delay(&self, attempt: u32) -> Duration {
        let delay = match self.backoff_type {
            BackoffType::Fixed => self.initial,
            BackoffType::Linear => {
                self.initial
                    + Duration::milliseconds(
                        (self.initial.num_milliseconds() as f64
                            * self.multiplier
                            * f64::from(attempt)) as i64,
                    )
            }
            BackoffType::Exponential => {
                let millis =
                    self.initial.num_milliseconds() as f64 * self.multiplier.powi(attempt as i32);
                Duration::milliseconds(millis as i64)
            }
        };

        if delay > self.max {
            self.max
        } else {
            delay
        }
    }
}

impl Default for Backoff {
    fn default() -> Self {
        Self {
            backoff_type: BackoffType::Exponential,
            initial: Duration::milliseconds(100),
            max: Duration::seconds(30),
            multiplier: 2.0,
        }
    }
}

// ============================================================================
// Strategy Types
// ============================================================================

/// Outcome of a recovery strategy execution
///
/// "Recovery not applicable" is reported as [`RecoveryOutcome::Failure`]
/// carrying [`RecoveryError::NotApplicable`], which names the verification
/// result recovery could not handle; a payload-free outcome variant would
/// have thrown that result away.
#[derive(Debug, Clone)]
pub enum RecoveryOutcome {
    /// Recovery succeeded with this result
    Success(VerificationResult),
    /// Recovery failed with error
    Failure(RecoveryError),
}

/// Strategy enum that wraps all concrete strategy types
#[derive(Debug, Clone)]
pub enum RecoveryStrategyEnum {
    /// Retry strategy
    Retry(RetryStrategy),
    /// Fallback strategy
    Fallback(FallbackStrategy),
    /// Circuit breaker strategy
    CircuitBreaker(CircuitBreakerStrategy),
    /// Timeout strategy
    Timeout(TimeoutStrategy),
    /// Composite strategy
    Composite(CompositeStrategy),
}

impl RecoveryStrategyEnum {
    /// Execute the strategy
    pub async fn execute(&self, factory: FutureFactory) -> RecoveryOutcome {
        match self {
            Self::Retry(s) => s.execute(factory).await,
            Self::Fallback(s) => s.execute(factory).await,
            Self::CircuitBreaker(s) => s.execute(factory).await,
            Self::Timeout(s) => s.execute(factory).await,
            Self::Composite(s) => s.execute(factory).await,
        }
    }

    /// Returns true if this strategy should be attempted for the given result
    #[must_use]
    pub fn is_applicable(&self, result: VerificationResult) -> bool {
        match self {
            Self::Retry(s) => s.is_applicable(result),
            Self::Fallback(s) => s.is_applicable(result),
            Self::CircuitBreaker(s) => s.is_applicable(result),
            Self::Timeout(s) => s.is_applicable(result),
            Self::Composite(s) => s.is_applicable(result),
        }
    }
}

/// Trait for recovery strategies
pub trait RecoveryStrategy: Send + Sync {
    /// Execute the recovery strategy
    fn execute(
        &self,
        factory: FutureFactory,
    ) -> Pin<Box<dyn Future<Output = RecoveryOutcome> + Send + '_>>;
    /// Returns true if this strategy should be attempted for the given result
    fn is_applicable(&self, result: VerificationResult) -> bool;
}

// ============================================================================
// Retry Strategy
// ============================================================================

/// Retry strategy with configurable backoff
#[derive(Debug, Clone)]
pub struct RetryStrategy {
    /// Maximum number of attempts
    max_attempts: u32,
    /// Backoff configuration
    backoff: Backoff,
}

impl RetryStrategy {
    /// Create a new retry strategy
    #[must_use]
    pub fn new(max_attempts: u32, backoff: Backoff) -> Self {
        Self {
            max_attempts,
            backoff,
        }
    }

    /// Create with default backoff
    #[must_use]
    pub fn with_default_backoff(max_attempts: u32) -> Self {
        Self {
            max_attempts,
            backoff: Backoff::default(),
        }
    }

    /// Get max attempts
    #[must_use]
    pub fn max_attempts(&self) -> u32 {
        self.max_attempts
    }

    /// Get backoff configuration
    #[must_use]
    pub fn backoff(&self) -> &Backoff {
        &self.backoff
    }
}

impl RecoveryStrategy for RetryStrategy {
    fn execute(
        &self,
        factory: FutureFactory,
    ) -> Pin<Box<dyn Future<Output = RecoveryOutcome> + Send + '_>> {
        let max_attempts = self.max_attempts;
        let backoff = self.backoff.clone();

        Box::pin(async move {
            let mut last_error = None;
            let mut has_failure = false;

            for attempt in 0..max_attempts {
                // Calculate and apply backoff delay (skip on first attempt)
                if attempt > 0 {
                    let delay = backoff.calculate_delay(attempt - 1);
                    // Durations here are always positive, so the cast is lossless.
                    #[allow(clippy::cast_sign_loss)]
                    let millis = delay.num_milliseconds() as u64;
                    tokio::time::sleep(tokio::time::Duration::from_millis(millis)).await;
                }

                // Execute the operation
                let op = factory();
                match op.await {
                    Ok(result) => {
                        if result.is_success() {
                            return RecoveryOutcome::Success(result);
                        }
                        // Track if we got a terminal failure
                        if result.is_failure() {
                            has_failure = true;
                        }
                        last_error = Some(RecoveryError::NotApplicable { result });
                    }
                    Err(ref e) => {
                        last_error = Some(e.clone());
                        if matches!(e, RecoveryError::CircuitBreakerOpen) {
                            break;
                        }
                    }
                }
            }

            // If we encountered a terminal failure (Failed/Partial), surface
            // its recorded error; a terminal failure always records
            // `last_error` alongside the flag, so exhaustion with a failure
            // but no error cannot occur. Otherwise report attempt
            // exhaustion on the non-terminal state.
            if has_failure {
                RecoveryOutcome::Failure(last_error.unwrap_or(RecoveryError::MaxAttemptsExceeded {
                    attempts: max_attempts,
                }))
            } else {
                RecoveryOutcome::Failure(RecoveryError::MaxAttemptsExceeded {
                    attempts: max_attempts,
                })
            }
        })
    }

    fn is_applicable(&self, result: VerificationResult) -> bool {
        matches!(
            result,
            VerificationResult::Unknown | VerificationResult::Failed | VerificationResult::Partial
        )
    }
}

// ============================================================================
// Fallback Strategy
// ============================================================================

/// A single fallback with its condition
#[derive(Debug)]
pub struct Fallback {
    /// Name for this fallback (for logging)
    name: String,
    /// Whether this fallback has been exhausted
    exhausted: AtomicU32,
    /// Max uses (0 = unlimited)
    max_uses: u32,
}

impl Clone for Fallback {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            exhausted: AtomicU32::new(self.exhausted.load(Ordering::SeqCst)),
            max_uses: self.max_uses,
        }
    }
}

impl Fallback {
    /// Create a new fallback with a name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            exhausted: AtomicU32::new(0),
            max_uses: 0,
        }
    }

    /// Set maximum number of uses
    #[must_use]
    pub fn with_max_uses(mut self, max: u32) -> Self {
        self.max_uses = max;
        self
    }

    /// Check if this fallback is exhausted
    #[allow(dead_code)]
    pub fn is_exhausted(&self) -> bool {
        if self.max_uses == 0 {
            return false;
        }
        self.exhausted.load(Ordering::SeqCst) >= self.max_uses
    }

    /// Get the name
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Fallback strategy that tries alternative operations
#[derive(Debug, Clone)]
pub struct FallbackStrategy {
    /// List of fallback operations
    fallbacks: Vec<Fallback>,
}

impl FallbackStrategy {
    /// Create a new fallback strategy
    #[must_use]
    pub fn new() -> Self {
        Self {
            fallbacks: Vec::new(),
        }
    }

    /// Add a fallback
    #[must_use]
    pub fn add_fallback(mut self, fallback: Fallback) -> Self {
        self.fallbacks.push(fallback);
        self
    }

    /// Add a fallback with a name
    #[must_use]
    pub fn with_fallback(mut self, name: impl Into<String>) -> Self {
        self.fallbacks.push(Fallback::new(name));
        self
    }
}

impl Default for FallbackStrategy {
    fn default() -> Self {
        Self::new()
    }
}

impl RecoveryStrategy for FallbackStrategy {
    fn execute(
        &self,
        _factory: FutureFactory,
    ) -> Pin<Box<dyn Future<Output = RecoveryOutcome> + Send + '_>> {
        Box::pin(async move { RecoveryOutcome::Failure(RecoveryError::AllFallbacksExhausted) })
    }

    fn is_applicable(&self, result: VerificationResult) -> bool {
        matches!(
            result,
            VerificationResult::Failed | VerificationResult::Partial | VerificationResult::Unknown
        )
    }
}

/// Execute fallback chain
///
/// # Errors
///
/// Returns [`RecoveryError::AllFallbacksExhausted`] when no fallback succeeds.
pub async fn execute_fallback_chain<Fut>(
    fallbacks: Vec<(String, impl Fn() -> Fut)>,
) -> Result<VerificationResult, RecoveryError>
where
    Fut: Future<Output = Result<VerificationResult, RecoveryError>>,
{
    for (name, op) in fallbacks {
        match op().await {
            Ok(result) => {
                if result.is_success() {
                    return Ok(result);
                }
                tracing::debug!(fallback = %name, result = %result, "Fallback did not succeed, trying next");
            }
            Err(e) => {
                tracing::warn!(fallback = %name, error = %e, "Fallback error");
            }
        }
    }
    Err(RecoveryError::AllFallbacksExhausted)
}

// ============================================================================
// Circuit Breaker Strategy
// ============================================================================

/// State of a circuit breaker
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Circuit is closed, operations proceed normally
    Closed,
    /// Circuit is open, operations fail fast
    Open,
    /// Circuit is half-open, allowing one test request
    HalfOpen,
}

/// Circuit breaker configuration
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of failures before opening circuit
    pub failure_threshold: u32,
    /// Number of successes to close circuit from half-open
    pub success_threshold: u32,
    /// Time to wait before trying again (in open state)
    pub recovery_timeout: Duration,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            success_threshold: 2,
            recovery_timeout: Duration::seconds(30),
        }
    }
}

/// Circuit breaker to prevent cascading failures
pub struct CircuitBreaker {
    /// Current state
    state: std::sync::RwLock<CircuitState>,
    /// Failure count in closed state
    failure_count: AtomicU32,
    /// Success count in half-open state
    success_count: AtomicU32,
    /// Last failure time
    last_failure: std::sync::RwLock<Option<DateTime<Utc>>>,
    /// Configuration
    config: CircuitBreakerConfig,
}

impl Debug for CircuitBreaker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CircuitBreaker")
            .field(
                "state",
                &self.state.read().unwrap_or_else(PoisonError::into_inner),
            )
            .field("failure_count", &self.failure_count.load(Ordering::SeqCst))
            .field("success_count", &self.success_count.load(Ordering::SeqCst))
            .field(
                "last_failure",
                &self
                    .last_failure
                    .read()
                    .unwrap_or_else(PoisonError::into_inner),
            )
            .field("config", &self.config)
            .finish()
    }
}

impl CircuitBreaker {
    /// Create a new circuit breaker
    #[must_use]
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            state: std::sync::RwLock::new(CircuitState::Closed),
            failure_count: AtomicU32::new(0),
            success_count: AtomicU32::new(0),
            last_failure: std::sync::RwLock::new(None),
            config,
        }
    }

    /// Create with default config
    #[must_use]
    pub fn with_default_config() -> Self {
        Self::new(CircuitBreakerConfig::default())
    }

    /// Get current state
    pub fn state(&self) -> CircuitState {
        *self.state.read().unwrap_or_else(PoisonError::into_inner)
    }

    /// Record a failure
    pub fn record_failure(&self) {
        let mut last = self
            .last_failure
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        *last = Some(Utc::now());

        let state = self.state();
        match state {
            CircuitState::Closed => {
                let failures = self.failure_count.fetch_add(1, Ordering::SeqCst) + 1;
                if failures >= self.config.failure_threshold {
                    *self.state.write().unwrap_or_else(PoisonError::into_inner) =
                        CircuitState::Open;
                    tracing::warn!(failures, "Circuit breaker opened");
                }
            }
            CircuitState::HalfOpen => {
                *self.state.write().unwrap_or_else(PoisonError::into_inner) = CircuitState::Open;
                self.success_count.store(0, Ordering::SeqCst);
                tracing::warn!("Circuit breaker re-opened from half-open");
            }
            CircuitState::Open => {}
        }
    }

    /// Record a success
    pub fn record_success(&self) {
        let state = self.state();
        match state {
            CircuitState::Closed => {
                self.failure_count.store(0, Ordering::SeqCst);
            }
            CircuitState::HalfOpen => {
                let successes = self.success_count.fetch_add(1, Ordering::SeqCst) + 1;
                if successes >= self.config.success_threshold {
                    *self.state.write().unwrap_or_else(PoisonError::into_inner) =
                        CircuitState::Closed;
                    self.failure_count.store(0, Ordering::SeqCst);
                    self.success_count.store(0, Ordering::SeqCst);
                    tracing::info!("Circuit breaker closed");
                }
            }
            CircuitState::Open => {}
        }
    }

    /// Check if operation is allowed
    pub fn is_allowed(&self) -> bool {
        let state = self.state();
        if state == CircuitState::Open {
            let last = self
                .last_failure
                .read()
                .unwrap_or_else(PoisonError::into_inner);
            if let Some(last_failure) = *last {
                if Utc::now() - last_failure >= self.config.recovery_timeout {
                    *self.state.write().unwrap_or_else(PoisonError::into_inner) =
                        CircuitState::HalfOpen;
                    tracing::info!("Circuit breaker entering half-open state");
                    return true;
                }
            }
            return false;
        }
        true
    }

    /// Execute an operation through the circuit breaker
    ///
    /// # Errors
    ///
    /// Returns [`RecoveryError::CircuitBreakerOpen`] when the breaker is open
    /// and the operation is not attempted, otherwise the error reported by the
    /// operation itself (mapped to [`RecoveryError::UnderlyingError`] by the
    /// caller-supplied future).
    pub async fn execute<F, Fut>(&self, op: F) -> Result<VerificationResult, RecoveryError>
    where
        F: Future<Output = Result<VerificationResult, RecoveryError>>,
    {
        if !self.is_allowed() {
            return Err(RecoveryError::CircuitBreakerOpen);
        }

        match op.await {
            Ok(result) => {
                if result.is_success() {
                    self.record_success();
                } else {
                    self.record_failure();
                }
                Ok(result)
            }
            Err(e) => {
                self.record_failure();
                Err(e)
            }
        }
    }
}

/// Circuit breaker strategy wrapper
#[derive(Debug, Clone)]
pub struct CircuitBreakerStrategy {
    circuit_breaker: Arc<CircuitBreaker>,
}

impl CircuitBreakerStrategy {
    /// Create a new circuit breaker strategy
    pub fn new(circuit_breaker: Arc<CircuitBreaker>) -> Self {
        Self { circuit_breaker }
    }

    /// Create with default config
    #[must_use]
    pub fn with_default_config() -> Self {
        Self {
            circuit_breaker: Arc::new(CircuitBreaker::with_default_config()),
        }
    }

    /// Get reference to circuit breaker for inspection
    #[must_use]
    pub fn circuit_breaker(&self) -> &Arc<CircuitBreaker> {
        &self.circuit_breaker
    }
}

impl RecoveryStrategy for CircuitBreakerStrategy {
    fn execute(
        &self,
        factory: FutureFactory,
    ) -> Pin<Box<dyn Future<Output = RecoveryOutcome> + Send + '_>> {
        let cb = self.circuit_breaker.clone();
        Box::pin(async move {
            if !cb.is_allowed() {
                return RecoveryOutcome::Failure(RecoveryError::CircuitBreakerOpen);
            }

            let result = cb.execute::<Pin<Box<dyn Future<Output = Result<VerificationResult, RecoveryError>> + Send>>, Pin<Box<dyn Future<Output = Result<VerificationResult, RecoveryError>> + Send>>>(factory()).await;
            match result {
                Ok(r) => RecoveryOutcome::Success(r),
                Err(e) => RecoveryOutcome::Failure(e),
            }
        })
    }

    fn is_applicable(&self, _result: VerificationResult) -> bool {
        true
    }
}

// ============================================================================
// Timeout Strategy
// ============================================================================

/// Timeout strategy configuration
#[derive(Debug, Clone)]
pub struct TimeoutStrategy {
    /// Timeout duration
    timeout: Duration,
}

impl TimeoutStrategy {
    /// Create a new timeout strategy
    #[must_use]
    pub fn new(timeout: Duration) -> Self {
        Self { timeout }
    }

    /// Get timeout duration
    #[must_use]
    pub fn timeout(&self) -> Duration {
        self.timeout
    }
}

impl RecoveryStrategy for TimeoutStrategy {
    fn execute(
        &self,
        factory: FutureFactory,
    ) -> Pin<Box<dyn Future<Output = RecoveryOutcome> + Send + '_>> {
        let timeout = self.timeout;
        Box::pin(async move {
            // Durations here are always positive, so the cast is lossless.
            #[allow(clippy::cast_sign_loss)]
            let millis = timeout.num_milliseconds() as u64;
            let result =
                tokio::time::timeout(tokio::time::Duration::from_millis(millis), factory()).await;

            match result {
                Ok(Ok(verification_result)) => RecoveryOutcome::Success(verification_result),
                Ok(Err(e)) => RecoveryOutcome::Failure(e),
                Err(_) => RecoveryOutcome::Failure(RecoveryError::Timeout { duration: timeout }),
            }
        })
    }

    fn is_applicable(&self, result: VerificationResult) -> bool {
        matches!(
            result,
            VerificationResult::Unknown | VerificationResult::Partial
        )
    }
}

// ============================================================================
// Recovery Executor
// ============================================================================

/// Recovery executor that coordinates multiple strategies
#[derive(Debug, Clone)]
pub struct RecoveryExecutor {
    /// Primary strategy
    primary: RecoveryStrategyEnum,
    /// Fallback strategies
    fallbacks: Vec<RecoveryStrategyEnum>,
    /// Circuit breaker (optional)
    circuit_breaker: Option<Arc<CircuitBreaker>>,
}

impl RecoveryExecutor {
    /// Create a new recovery executor with a primary strategy
    #[must_use]
    pub fn new(primary: RecoveryStrategyEnum) -> Self {
        Self {
            primary,
            fallbacks: Vec::new(),
            circuit_breaker: None,
        }
    }

    /// Add a fallback strategy
    #[must_use]
    pub fn with_fallback(mut self, fallback: RecoveryStrategyEnum) -> Self {
        self.fallbacks.push(fallback);
        self
    }

    /// Add a circuit breaker
    #[must_use]
    pub fn with_circuit_breaker(mut self, cb: Arc<CircuitBreaker>) -> Self {
        self.circuit_breaker = Some(cb);
        self
    }

    /// Execute recovery with the primary strategy, then fallbacks
    ///
    /// # Errors
    ///
    /// Returns the failure produced by the primary strategy, or
    /// [`RecoveryError::CircuitBreakerOpen`] when the circuit breaker blocks
    /// the attempt. Failures are reported through [`RecoveryOutcome::Failure`].
    pub async fn execute<F, Fut>(&self, op: F) -> RecoveryOutcome
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<VerificationResult, RecoveryError>> + Send + 'static,
    {
        let factory: FutureFactory = Arc::new(move || Box::pin(op()));

        // Try primary strategy
        let outcome = self.primary.execute(factory.clone()).await;
        if matches!(outcome, RecoveryOutcome::Success(_)) {
            return outcome;
        }

        // Try circuit breaker if present
        if let Some(ref cb) = self.circuit_breaker {
            if !cb.is_allowed() {
                return RecoveryOutcome::Failure(RecoveryError::CircuitBreakerOpen);
            }
        }

        // Try fallbacks in order
        for fallback in &self.fallbacks {
            let fb_outcome = fallback.execute(factory.clone()).await;
            if matches!(fb_outcome, RecoveryOutcome::Success(_)) {
                return fb_outcome;
            }
        }

        outcome
    }

    /// Execute recovery, returning the final result or error
    ///
    /// # Errors
    ///
    /// Forwards the failure from [`Self::execute`], including
    /// [`RecoveryError::NotApplicable`] when the operation reached a terminal
    /// result recovery cannot act on.
    pub async fn execute_and_return<F, Fut>(
        &self,
        op: F,
    ) -> Result<VerificationResult, RecoveryError>
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<VerificationResult, RecoveryError>> + Send + 'static,
    {
        match self.execute(op).await {
            RecoveryOutcome::Success(result) => Ok(result),
            RecoveryOutcome::Failure(e) => Err(e),
        }
    }
}

// ============================================================================
// Composite Strategy
// ============================================================================

/// A composite strategy that chains multiple strategies
#[derive(Debug, Clone)]
pub struct CompositeStrategy {
    /// Strategies to try in order
    strategies: Vec<RecoveryStrategyEnum>,
}

impl CompositeStrategy {
    /// Create a new composite strategy
    #[must_use]
    pub fn new() -> Self {
        Self {
            strategies: Vec::new(),
        }
    }

    /// Add a strategy to the chain
    #[must_use]
    pub fn add_strategy(mut self, strategy: RecoveryStrategyEnum) -> Self {
        self.strategies.push(strategy);
        self
    }
}

impl Default for CompositeStrategy {
    fn default() -> Self {
        Self::new()
    }
}

impl RecoveryStrategy for CompositeStrategy {
    fn execute(
        &self,
        factory: FutureFactory,
    ) -> Pin<Box<dyn Future<Output = RecoveryOutcome> + Send + '_>> {
        let strategies = self.strategies.clone();
        Box::pin(async move {
            for strategy in &strategies {
                let outcome = strategy.execute(factory.clone()).await;
                if matches!(outcome, RecoveryOutcome::Success(_)) {
                    return outcome;
                }
            }
            RecoveryOutcome::Failure(RecoveryError::MaxAttemptsExceeded { attempts: 1 })
        })
    }

    fn is_applicable(&self, result: VerificationResult) -> bool {
        self.strategies.iter().any(|s| s.is_applicable(result))
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Concrete operation type for [`CircuitBreaker::execute`], whose `Fut`
    /// parameter is unconstrained and therefore has to be named by callers.
    type ReadyOp = std::future::Ready<Result<VerificationResult, RecoveryError>>;

    fn async_ok(
        result: VerificationResult,
    ) -> impl Future<Output = Result<VerificationResult, RecoveryError>> {
        std::future::ready(Ok(result))
    }

    fn async_err(
        err: RecoveryError,
    ) -> impl Future<Output = Result<VerificationResult, RecoveryError>> {
        std::future::ready(Err(err))
    }

    /// Operation that becomes ready after `delay_ms`, used both by tests where
    /// the timeout wins (the future is dropped) and by tests where it completes.
    async fn completes_after(
        delay_ms: u64,
        result: VerificationResult,
    ) -> Result<VerificationResult, RecoveryError> {
        tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
        Ok(result)
    }

    #[tokio::test]
    async fn retry_strategy_success_on_first_attempt() {
        let strategy = RetryStrategy::with_default_backoff(3);
        let factory: FutureFactory = Arc::new(|| Box::pin(async_ok(VerificationResult::Verified)));
        let outcome = strategy.execute(factory).await;
        assert!(matches!(
            outcome,
            RecoveryOutcome::Success(VerificationResult::Verified)
        ));
    }

    #[tokio::test]
    async fn retry_strategy_retries_on_unknown() {
        let strategy = RetryStrategy::with_default_backoff(3);
        let call_count = Arc::new(AtomicU32::new(0));
        let call_count_clone = call_count.clone();

        let factory: FutureFactory = Arc::new(move || {
            let cc = call_count_clone.clone();
            Box::pin(async move {
                cc.fetch_add(1, Ordering::SeqCst);
                if cc.load(Ordering::SeqCst) < 3 {
                    Ok(VerificationResult::Unknown)
                } else {
                    Ok(VerificationResult::Verified)
                }
            })
        });

        let outcome = strategy.execute(factory).await;

        assert!(matches!(
            outcome,
            RecoveryOutcome::Success(VerificationResult::Verified)
        ));
        assert_eq!(call_count.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn retry_strategy_max_attempts_exceeded() {
        let backoff = Backoff::new(
            BackoffType::Fixed,
            Duration::milliseconds(1),
            Duration::seconds(1),
        );
        let strategy = RetryStrategy::new(2, backoff);
        let factory: FutureFactory = Arc::new(|| Box::pin(async_ok(VerificationResult::Unknown)));

        let outcome = strategy.execute(factory).await;

        assert!(matches!(
            outcome,
            RecoveryOutcome::Failure(RecoveryError::MaxAttemptsExceeded { attempts: 2 })
        ));
    }

    #[tokio::test]
    async fn circuit_breaker_opens_after_failures() {
        let config = CircuitBreakerConfig {
            failure_threshold: 3,
            success_threshold: 2,
            recovery_timeout: Duration::seconds(30),
        };
        let cb = Arc::new(CircuitBreaker::new(config));

        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Closed);

        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
    }

    #[tokio::test]
    async fn circuit_breaker_allows_request_in_half_open() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            success_threshold: 1,
            recovery_timeout: Duration::milliseconds(10),
        };
        let cb = Arc::new(CircuitBreaker::new(config));

        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);

        tokio::time::sleep(tokio::time::Duration::from_millis(15)).await;

        assert!(cb.is_allowed());
        assert_eq!(cb.state(), CircuitState::HalfOpen);
    }

    #[tokio::test]
    async fn circuit_breaker_closes_after_successes() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            success_threshold: 2,
            recovery_timeout: Duration::milliseconds(10),
        };
        let cb = Arc::new(CircuitBreaker::new(config));

        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);

        tokio::time::sleep(tokio::time::Duration::from_millis(15)).await;
        cb.is_allowed();

        cb.record_success();
        cb.record_success();
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[tokio::test]
    async fn timeout_strategy_times_out() {
        let strategy = TimeoutStrategy::new(Duration::milliseconds(10));
        let factory: FutureFactory =
            Arc::new(|| Box::pin(completes_after(100, VerificationResult::Unknown)));

        let outcome = strategy.execute(factory).await;

        assert!(matches!(
            outcome,
            RecoveryOutcome::Failure(RecoveryError::Timeout { .. })
        ));
    }

    #[tokio::test]
    async fn timeout_strategy_succeeds_within_timeout() {
        let strategy = TimeoutStrategy::new(Duration::milliseconds(100));
        let factory: FutureFactory =
            Arc::new(|| Box::pin(completes_after(10, VerificationResult::Verified)));

        let outcome = strategy.execute(factory).await;

        assert!(matches!(
            outcome,
            RecoveryOutcome::Success(VerificationResult::Verified)
        ));
    }

    #[tokio::test]
    async fn recovery_executor_tries_primary_then_fallbacks() {
        let primary = RecoveryStrategyEnum::Retry(RetryStrategy::with_default_backoff(1));
        let executor = RecoveryExecutor::new(primary).with_fallback(RecoveryStrategyEnum::Retry(
            RetryStrategy::with_default_backoff(1),
        ));

        let outcome = executor
            .execute(|| async_ok(VerificationResult::Unknown))
            .await;
        assert!(matches!(outcome, RecoveryOutcome::Failure(_)));
    }

    #[tokio::test]
    async fn backoff_exponential_calculates_correctly() {
        let backoff = Backoff::new(
            BackoffType::Exponential,
            Duration::milliseconds(100),
            Duration::seconds(10),
        );

        assert_eq!(backoff.calculate_delay(0), Duration::milliseconds(100));
        assert_eq!(backoff.calculate_delay(1), Duration::milliseconds(200));
        assert_eq!(backoff.calculate_delay(2), Duration::milliseconds(400));
        assert_eq!(backoff.calculate_delay(3), Duration::milliseconds(800));
    }

    #[tokio::test]
    async fn backoff_linear_calculates_correctly() {
        let backoff = Backoff::new(
            BackoffType::Linear,
            Duration::milliseconds(100),
            Duration::seconds(10),
        );

        assert_eq!(backoff.calculate_delay(0), Duration::milliseconds(100));
        assert_eq!(backoff.calculate_delay(1), Duration::milliseconds(300));
        assert_eq!(backoff.calculate_delay(2), Duration::milliseconds(500));
    }

    #[tokio::test]
    async fn backoff_fixed_always_returns_initial() {
        let backoff = Backoff::new(
            BackoffType::Fixed,
            Duration::milliseconds(100),
            Duration::seconds(10),
        );

        assert_eq!(backoff.calculate_delay(0), Duration::milliseconds(100));
        assert_eq!(backoff.calculate_delay(1), Duration::milliseconds(100));
        assert_eq!(backoff.calculate_delay(5), Duration::milliseconds(100));
    }

    #[tokio::test]
    async fn retry_strategy_is_applicable_for_unknown() {
        let strategy = RetryStrategy::with_default_backoff(3);
        assert!(strategy.is_applicable(VerificationResult::Unknown));
        assert!(strategy.is_applicable(VerificationResult::Failed));
        assert!(strategy.is_applicable(VerificationResult::Partial));
        assert!(!strategy.is_applicable(VerificationResult::Verified));
        assert!(!strategy.is_applicable(VerificationResult::Duplicate));
    }

    #[tokio::test]
    async fn composite_strategy_tries_all_strategies() {
        let composite = CompositeStrategy::new()
            .add_strategy(RecoveryStrategyEnum::Retry(
                RetryStrategy::with_default_backoff(1),
            ))
            .add_strategy(RecoveryStrategyEnum::Retry(
                RetryStrategy::with_default_backoff(1),
            ));

        let call_count = Arc::new(AtomicU32::new(0));
        let call_count_clone = call_count.clone();

        let factory: FutureFactory = Arc::new(move || {
            let cc = call_count_clone.clone();
            Box::pin(async move {
                cc.fetch_add(1, Ordering::SeqCst);
                Ok(VerificationResult::Unknown)
            })
        });

        let outcome = composite.execute(factory).await;

        assert!(matches!(outcome, RecoveryOutcome::Failure(_)));
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn execute_fallback_chain_succeeds_on_second() {
        let result = execute_fallback_chain(vec![
            (
                "first".to_string(),
                Box::new(|| async_ok(VerificationResult::Unknown)) as Box<dyn Fn() -> _>,
            ),
            (
                "second".to_string(),
                Box::new(|| async_ok(VerificationResult::Verified)) as Box<dyn Fn() -> _>,
            ),
        ])
        .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), VerificationResult::Verified);
    }

    // ------------------------------------------------------------------
    // Backoff
    // ------------------------------------------------------------------

    #[test]
    fn backoff_default_is_exponential_with_documented_bounds() {
        let backoff = Backoff::default();
        assert_eq!(backoff.backoff_type, BackoffType::Exponential);
        assert_eq!(backoff.initial, Duration::milliseconds(100));
        assert_eq!(backoff.max, Duration::seconds(30));
        assert!((backoff.multiplier - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn backoff_multiplier_is_configurable() {
        let backoff = Backoff::new(
            BackoffType::Linear,
            Duration::milliseconds(100),
            Duration::seconds(10),
        )
        .with_multiplier(0.5);

        assert!((backoff.multiplier - 0.5).abs() < f64::EPSILON);
        assert_eq!(backoff.calculate_delay(2), Duration::milliseconds(200));
    }

    #[test]
    fn backoff_delay_is_clamped_to_max() {
        let backoff = Backoff::new(
            BackoffType::Exponential,
            Duration::milliseconds(100),
            Duration::milliseconds(300),
        );

        assert_eq!(backoff.calculate_delay(0), Duration::milliseconds(100));
        assert_eq!(backoff.calculate_delay(1), Duration::milliseconds(200));
        assert_eq!(backoff.calculate_delay(2), Duration::milliseconds(300));
        assert_eq!(backoff.calculate_delay(9), Duration::milliseconds(300));
    }

    #[test]
    fn backoff_types_roundtrip_through_serde() {
        let pairs = [
            (BackoffType::Fixed, "fixed"),
            (BackoffType::Linear, "linear"),
            (BackoffType::Exponential, "exponential"),
        ];
        for (kind, name) in pairs {
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(json, format!(r#""{name}""#));
            let back: BackoffType = serde_json::from_str(&json).unwrap();
            assert_eq!(back, kind);
        }
        assert_eq!(BackoffType::default(), BackoffType::Exponential);
    }

    #[test]
    fn backoff_accessors_expose_the_configuration() {
        let strategy = RetryStrategy::new(
            5,
            Backoff::new(
                BackoffType::Fixed,
                Duration::milliseconds(5),
                Duration::seconds(1),
            ),
        );
        assert_eq!(strategy.max_attempts(), 5);
        assert_eq!(strategy.backoff().backoff_type, BackoffType::Fixed);
        assert_eq!(strategy.backoff().initial, Duration::milliseconds(5));
    }

    // ------------------------------------------------------------------
    // Strategy enum dispatch
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn strategy_enum_dispatches_execute_to_every_variant() {
        let circuit_breaker = Arc::new(CircuitBreaker::with_default_config());
        let strategies = vec![
            RecoveryStrategyEnum::Retry(RetryStrategy::with_default_backoff(1)),
            RecoveryStrategyEnum::CircuitBreaker(CircuitBreakerStrategy::new(circuit_breaker)),
            RecoveryStrategyEnum::Timeout(TimeoutStrategy::new(Duration::seconds(1))),
            RecoveryStrategyEnum::Composite(CompositeStrategy::new().add_strategy(
                RecoveryStrategyEnum::Retry(RetryStrategy::with_default_backoff(1)),
            )),
        ];

        for strategy in &strategies {
            let factory: FutureFactory =
                Arc::new(|| Box::pin(async_ok(VerificationResult::Verified)));
            let outcome = strategy.execute(factory).await;
            assert!(
                matches!(
                    outcome,
                    RecoveryOutcome::Success(VerificationResult::Verified)
                ),
                "every strategy must dispatch execute: got {outcome:?}"
            );
        }
    }

    /// The fallback variant has no way to run its (private) fallback list
    /// through a factory, so dispatching to it reports an exhausted chain.
    #[tokio::test]
    async fn strategy_enum_fallback_variant_reports_exhaustion() {
        let strategy = RecoveryStrategyEnum::Fallback(FallbackStrategy::new());
        let factory: FutureFactory = Arc::new(|| Box::pin(async_ok(VerificationResult::Verified)));
        let outcome = strategy.execute(factory).await;
        assert!(matches!(
            outcome,
            RecoveryOutcome::Failure(RecoveryError::AllFallbacksExhausted)
        ));
    }

    #[test]
    fn strategy_enum_dispatches_is_applicable_to_every_variant() {
        let circuit_breaker = Arc::new(CircuitBreaker::with_default_config());
        let strategies = vec![
            RecoveryStrategyEnum::Retry(RetryStrategy::with_default_backoff(1)),
            RecoveryStrategyEnum::Fallback(FallbackStrategy::new().with_fallback("cache")),
            RecoveryStrategyEnum::CircuitBreaker(CircuitBreakerStrategy::new(circuit_breaker)),
            RecoveryStrategyEnum::Timeout(TimeoutStrategy::new(Duration::seconds(1))),
            RecoveryStrategyEnum::Composite(CompositeStrategy::new().add_strategy(
                RecoveryStrategyEnum::Timeout(TimeoutStrategy::new(Duration::seconds(1))),
            )),
        ];

        for strategy in &strategies {
            // Only the circuit breaker accepts every result unconditionally.
            let expected = matches!(strategy, RecoveryStrategyEnum::CircuitBreaker(_));
            assert_eq!(
                strategy.is_applicable(VerificationResult::Verified),
                expected
            );
            assert!(strategy.is_applicable(VerificationResult::Unknown));
            assert!(strategy.is_applicable(VerificationResult::Partial));
        }
    }

    // ------------------------------------------------------------------
    // Retry
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn retry_reports_the_operation_error_once_attempts_run_out() {
        let backoff = Backoff::new(
            BackoffType::Fixed,
            Duration::milliseconds(1),
            Duration::seconds(1),
        );
        let strategy = RetryStrategy::new(2, backoff);
        let factory: FutureFactory = Arc::new(|| {
            Box::pin(async_err(RecoveryError::UnderlyingError {
                context: "provider unavailable".into(),
            }))
        });

        let outcome = strategy.execute(factory).await;
        // Repeated operation errors are collapsed: the strategy reports that it
        // ran out of attempts rather than the last underlying error.
        assert!(matches!(
            outcome,
            RecoveryOutcome::Failure(RecoveryError::MaxAttemptsExceeded { attempts: 2 })
        ));
    }

    /// The last underlying error only surfaces when a terminal failure was also
    /// seen, which sets `has_failure` and lets the recorded error through.
    #[tokio::test]
    async fn retry_surfaces_the_last_error_after_a_terminal_failure() {
        let backoff = Backoff::new(
            BackoffType::Fixed,
            Duration::milliseconds(1),
            Duration::seconds(1),
        );
        let strategy = RetryStrategy::new(2, backoff);
        let calls = Arc::new(AtomicU32::new(0));
        let counter = calls.clone();

        let factory: FutureFactory = Arc::new(move || {
            let seen = counter.fetch_add(1, Ordering::SeqCst);
            if seen == 0 {
                Box::pin(async_ok(VerificationResult::Failed))
            } else {
                Box::pin(async_err(RecoveryError::UnderlyingError {
                    context: "provider unavailable".into(),
                }))
            }
        });

        let outcome = strategy.execute(factory).await;
        assert!(matches!(
            outcome,
            RecoveryOutcome::Failure(RecoveryError::UnderlyingError { ref context })
                if context == "provider unavailable"
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn retry_stops_immediately_when_the_circuit_breaker_is_open() {
        let backoff = Backoff::new(
            BackoffType::Fixed,
            Duration::milliseconds(1),
            Duration::seconds(1),
        );
        let strategy = RetryStrategy::new(5, backoff);
        let calls = Arc::new(AtomicU32::new(0));
        let counter = calls.clone();

        let factory: FutureFactory = Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            Box::pin(async_err(RecoveryError::CircuitBreakerOpen))
        });

        let outcome = strategy.execute(factory).await;
        // The loop stops after the first open-breaker error, but the breaker
        // signal itself is not preserved: the caller sees the attempt limit.
        assert!(matches!(
            outcome,
            RecoveryOutcome::Failure(RecoveryError::MaxAttemptsExceeded { attempts: 5 })
        ));
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "an open breaker must abort the retry loop"
        );
    }

    #[tokio::test]
    async fn retry_surfaces_a_terminal_failure_as_not_applicable() {
        let backoff = Backoff::new(
            BackoffType::Fixed,
            Duration::milliseconds(1),
            Duration::seconds(1),
        );
        let strategy = RetryStrategy::new(2, backoff);
        let factory: FutureFactory = Arc::new(|| Box::pin(async_ok(VerificationResult::Failed)));

        let outcome = strategy.execute(factory).await;
        assert!(matches!(
            outcome,
            RecoveryOutcome::Failure(RecoveryError::NotApplicable {
                result: VerificationResult::Failed,
            })
        ));
    }

    #[tokio::test]
    async fn retry_treats_duplicate_as_a_successful_stop() {
        let strategy = RetryStrategy::with_default_backoff(2);
        let factory: FutureFactory = Arc::new(|| Box::pin(async_ok(VerificationResult::Duplicate)));
        let outcome = strategy.execute(factory).await;
        assert!(matches!(
            outcome,
            RecoveryOutcome::Success(VerificationResult::Duplicate)
        ));
    }

    // ------------------------------------------------------------------
    // Fallback
    // ------------------------------------------------------------------

    #[test]
    fn fallback_tracks_its_budget() {
        let fallback = Fallback::new("cache-read").with_max_uses(2);
        assert_eq!(fallback.name(), "cache-read");
        assert!(!fallback.is_exhausted());

        let once = fallback.clone();
        assert_eq!(once.name(), "cache-read", "clone keeps the name");
        assert!(!once.is_exhausted());

        // An unlimited fallback never reports exhaustion.
        let unlimited = Fallback::new("always");
        assert!(!unlimited.is_exhausted());
    }

    #[tokio::test]
    async fn fallback_strategy_reports_exhaustion() {
        let strategy = FallbackStrategy::new()
            .add_fallback(Fallback::new("cache"))
            .with_fallback("archive");

        assert_eq!(strategy.fallbacks.len(), 2);

        let factory: FutureFactory = Arc::new(|| Box::pin(async_ok(VerificationResult::Verified)));
        let outcome = strategy.execute(factory).await;
        assert!(matches!(
            outcome,
            RecoveryOutcome::Failure(RecoveryError::AllFallbacksExhausted)
        ));
    }

    #[tokio::test]
    async fn fallback_strategy_is_applicable_for_retryable_results() {
        let strategy = FallbackStrategy::default();
        assert!(strategy.is_applicable(VerificationResult::Failed));
        assert!(strategy.is_applicable(VerificationResult::Partial));
        assert!(strategy.is_applicable(VerificationResult::Unknown));
        assert!(!strategy.is_applicable(VerificationResult::Verified));
        assert!(!strategy.is_applicable(VerificationResult::Duplicate));
    }

    #[tokio::test]
    async fn fallback_chain_skips_a_fallback_that_does_not_succeed() {
        // The first fallback completes but does not satisfy the postcondition,
        // so the chain must keep looking rather than accept the result.
        let result = execute_fallback_chain(vec![
            (
                "stale".to_string(),
                Box::new(|| async_ok(VerificationResult::Partial)) as Box<dyn Fn() -> _>,
            ),
            (
                "fresh".to_string(),
                Box::new(|| async_ok(VerificationResult::Verified)) as Box<dyn Fn() -> _>,
            ),
        ])
        .await;

        assert_eq!(result.unwrap(), VerificationResult::Verified);
    }

    // ------------------------------------------------------------------
    // Circuit breaker
    // ------------------------------------------------------------------

    fn breaker(config: CircuitBreakerConfig) -> CircuitBreaker {
        CircuitBreaker::new(config)
    }

    #[test]
    fn circuit_breaker_config_defaults_are_documented() {
        let config = CircuitBreakerConfig::default();
        assert_eq!(config.failure_threshold, 5);
        assert_eq!(config.success_threshold, 2);
        assert_eq!(config.recovery_timeout, Duration::seconds(30));
    }

    #[test]
    fn circuit_breaker_is_debuggable() {
        let cb = breaker(CircuitBreakerConfig::default());
        let debugged = std::format!("{cb:?}");
        assert!(debugged.contains("CircuitBreaker"));
        assert!(debugged.contains("failure_count"));
        assert!(debugged.contains("success_count"));
        assert!(debugged.contains("last_failure"));
        assert!(debugged.contains("config"));
    }

    #[test]
    fn closed_circuit_allows_operations_and_resets_the_failure_count() {
        let cb = breaker(CircuitBreakerConfig {
            failure_threshold: 3,
            success_threshold: 1,
            recovery_timeout: Duration::seconds(30),
        });

        assert!(cb.is_allowed());
        assert_eq!(cb.state(), CircuitState::Closed);

        cb.record_failure();
        cb.record_success();
        assert_eq!(cb.state(), CircuitState::Closed);

        // The success reset the counter, so two more failures stay closed.
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn open_circuit_rejects_until_the_recovery_timeout_elapses() {
        let cb = breaker(CircuitBreakerConfig {
            failure_threshold: 1,
            success_threshold: 1,
            recovery_timeout: Duration::seconds(3600),
        });

        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
        assert!(
            !cb.is_allowed(),
            "no time has passed, so the breaker stays open"
        );
        assert_eq!(cb.state(), CircuitState::Open);

        // Further failures while open are ignored rather than re-arming timers.
        cb.record_failure();
        cb.record_success();
        assert_eq!(cb.state(), CircuitState::Open);
    }

    #[test]
    fn half_open_failure_reopens_the_breaker() {
        let cb = breaker(CircuitBreakerConfig {
            failure_threshold: 1,
            success_threshold: 2,
            recovery_timeout: Duration::milliseconds(5),
        });

        cb.record_failure();
        std::thread::sleep(std::time::Duration::from_millis(10));
        assert!(cb.is_allowed());
        assert_eq!(cb.state(), CircuitState::HalfOpen);

        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open, "a failed probe reopens");

        // The probe's success count was cleared, so half-open must be re-earned.
        std::thread::sleep(std::time::Duration::from_millis(10));
        assert!(cb.is_allowed());
        cb.record_success();
        assert_eq!(cb.state(), CircuitState::HalfOpen);
    }

    #[tokio::test]
    async fn circuit_breaker_execute_records_successes_and_failures() {
        let cb = breaker(CircuitBreakerConfig {
            failure_threshold: 2,
            success_threshold: 1,
            recovery_timeout: Duration::seconds(30),
        });

        // `Fut` is an unconstrained parameter on `CircuitBreaker::execute`, so
        // each operation names its concrete future type via `ReadyOp`.
        let verified: ReadyOp = std::future::ready(Ok(VerificationResult::Verified));
        let ok = cb.execute::<ReadyOp, ReadyOp>(verified).await;
        assert_eq!(ok.unwrap(), VerificationResult::Verified);

        // A completed operation that does not satisfy the postconditions still
        // counts as a breaker failure.
        let failed: ReadyOp = std::future::ready(Ok(VerificationResult::Failed));
        let unsatisfied = cb.execute::<ReadyOp, ReadyOp>(failed).await;
        assert_eq!(unsatisfied.unwrap(), VerificationResult::Failed);
        assert_eq!(cb.state(), CircuitState::Closed);

        let rejected: ReadyOp = std::future::ready(Err(RecoveryError::UnderlyingError {
            context: "boom".into(),
        }));
        let err = cb.execute::<ReadyOp, ReadyOp>(rejected).await;
        assert!(matches!(
            err,
            Err(RecoveryError::UnderlyingError { ref context }) if context == "boom"
        ));
        assert_eq!(
            cb.state(),
            CircuitState::Open,
            "two failures opened the breaker"
        );

        // While open, execute refuses without running the operation.
        let op: ReadyOp = std::future::ready(Ok(VerificationResult::Verified));
        let refused = cb.execute::<ReadyOp, ReadyOp>(op).await;
        assert!(matches!(refused, Err(RecoveryError::CircuitBreakerOpen)));
    }

    #[tokio::test]
    async fn circuit_breaker_strategy_wraps_the_breaker() {
        let strategy = CircuitBreakerStrategy::with_default_config();
        let breaker = strategy.circuit_breaker();
        assert_eq!(breaker.state(), CircuitState::Closed);

        let factory: FutureFactory = Arc::new(|| Box::pin(async_ok(VerificationResult::Verified)));
        let outcome = strategy.execute(factory).await;
        assert!(matches!(
            outcome,
            RecoveryOutcome::Success(VerificationResult::Verified)
        ));
        assert!(strategy.is_applicable(VerificationResult::Failed));
    }

    #[tokio::test]
    async fn circuit_breaker_strategy_reports_an_open_breaker() {
        let cb = Arc::new(breaker(CircuitBreakerConfig {
            failure_threshold: 1,
            success_threshold: 1,
            recovery_timeout: Duration::seconds(3600),
        }));
        cb.record_failure();

        let strategy = CircuitBreakerStrategy::new(cb);
        let factory: FutureFactory = Arc::new(|| Box::pin(async_ok(VerificationResult::Verified)));
        let outcome = strategy.execute(factory).await;
        assert!(matches!(
            outcome,
            RecoveryOutcome::Failure(RecoveryError::CircuitBreakerOpen)
        ));
    }

    #[tokio::test]
    async fn circuit_breaker_strategy_propagates_operation_errors() {
        let strategy = CircuitBreakerStrategy::new(Arc::new(breaker(CircuitBreakerConfig {
            failure_threshold: 5,
            success_threshold: 1,
            recovery_timeout: Duration::seconds(30),
        })));
        let factory: FutureFactory =
            Arc::new(|| Box::pin(async_err(RecoveryError::CircuitBreakerOpen)));
        let outcome = strategy.execute(factory).await;
        assert!(matches!(
            outcome,
            RecoveryOutcome::Failure(RecoveryError::CircuitBreakerOpen)
        ));
    }

    // ------------------------------------------------------------------
    // Timeout
    // ------------------------------------------------------------------

    #[test]
    fn timeout_strategy_exposes_its_duration() {
        let strategy = TimeoutStrategy::new(Duration::milliseconds(250));
        assert_eq!(strategy.timeout(), Duration::milliseconds(250));
    }

    #[tokio::test]
    async fn timeout_strategy_propagates_an_operation_error() {
        let strategy = TimeoutStrategy::new(Duration::seconds(1));
        let factory: FutureFactory = Arc::new(|| {
            Box::pin(async_err(RecoveryError::UnderlyingError {
                context: "rejected".into(),
            }))
        });

        let outcome = strategy.execute(factory).await;
        assert!(matches!(
            outcome,
            RecoveryOutcome::Failure(RecoveryError::UnderlyingError { ref context })
                if context == "rejected"
        ));
    }

    #[test]
    fn timeout_strategy_is_applicable_for_ambiguous_results() {
        let strategy = TimeoutStrategy::new(Duration::seconds(1));
        assert!(strategy.is_applicable(VerificationResult::Unknown));
        assert!(strategy.is_applicable(VerificationResult::Partial));
        assert!(!strategy.is_applicable(VerificationResult::Failed));
        assert!(!strategy.is_applicable(VerificationResult::Verified));
        assert!(!strategy.is_applicable(VerificationResult::Duplicate));
    }

    // ------------------------------------------------------------------
    // Executor
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn executor_returns_the_primary_success_without_trying_fallbacks() {
        let calls = Arc::new(AtomicU32::new(0));
        let counter = calls.clone();
        let primary = RecoveryStrategyEnum::Retry(RetryStrategy::with_default_backoff(2));
        let executor = RecoveryExecutor::new(primary)
            .with_fallback(RecoveryStrategyEnum::Fallback(FallbackStrategy::new()));

        let outcome = executor
            .execute(move || {
                counter.fetch_add(1, Ordering::SeqCst);
                async_ok(VerificationResult::Verified)
            })
            .await;

        assert!(matches!(
            outcome,
            RecoveryOutcome::Success(VerificationResult::Verified)
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn executor_falls_back_to_a_strategy_that_succeeds() {
        let calls = Arc::new(AtomicU32::new(0));
        let counter = calls.clone();

        let executor = RecoveryExecutor::new(RecoveryStrategyEnum::Retry(
            RetryStrategy::with_default_backoff(1),
        ))
        .with_fallback(RecoveryStrategyEnum::Retry(
            RetryStrategy::with_default_backoff(2),
        ));

        let outcome = executor
            .execute(move || {
                let seen = counter.fetch_add(1, Ordering::SeqCst);
                if seen == 0 {
                    async_ok(VerificationResult::Unknown)
                } else {
                    async_ok(VerificationResult::Verified)
                }
            })
            .await;

        assert!(
            matches!(
                outcome,
                RecoveryOutcome::Success(VerificationResult::Verified)
            ),
            "the fallback must be allowed to recover"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn executor_is_blocked_by_an_open_circuit_breaker() {
        let cb = Arc::new(breaker(CircuitBreakerConfig {
            failure_threshold: 1,
            success_threshold: 1,
            recovery_timeout: Duration::seconds(3600),
        }));
        cb.record_failure();

        let executor = RecoveryExecutor::new(RecoveryStrategyEnum::Retry(
            RetryStrategy::with_default_backoff(1),
        ))
        .with_fallback(RecoveryStrategyEnum::Retry(
            RetryStrategy::with_default_backoff(1),
        ))
        .with_circuit_breaker(cb);

        let outcome = executor
            .execute(|| async_ok(VerificationResult::Unknown))
            .await;
        assert!(matches!(
            outcome,
            RecoveryOutcome::Failure(RecoveryError::CircuitBreakerOpen)
        ));
    }

    #[tokio::test]
    async fn execute_and_return_maps_outcomes_to_results() {
        let success = RecoveryExecutor::new(RecoveryStrategyEnum::Retry(
            RetryStrategy::with_default_backoff(1),
        ));
        let result = success
            .execute_and_return(|| async_ok(VerificationResult::Verified))
            .await;
        assert_eq!(result.unwrap(), VerificationResult::Verified);

        let failing = RecoveryExecutor::new(RecoveryStrategyEnum::Timeout(TimeoutStrategy::new(
            Duration::milliseconds(1),
        )));
        let result = failing
            .execute_and_return(|| completes_after(50, VerificationResult::Unknown))
            .await;
        assert!(matches!(result, Err(RecoveryError::Timeout { .. })));

        // A terminal verification failure surfaces as "recovery not
        // applicable" carrying the result that caused it — not as an
        // attempt-limit failure and not with the result replaced by `Unknown`.
        let terminal = RecoveryExecutor::new(RecoveryStrategyEnum::Retry(
            RetryStrategy::with_default_backoff(1),
        ));
        let result = terminal
            .execute_and_return(|| async_ok(VerificationResult::Failed))
            .await;
        assert!(matches!(
            result,
            Err(RecoveryError::NotApplicable {
                result: VerificationResult::Failed,
            })
        ));
    }

    // ------------------------------------------------------------------
    // Composite
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn composite_stops_at_the_first_successful_strategy() {
        let calls = Arc::new(AtomicU32::new(0));
        let counter = calls.clone();

        let composite = CompositeStrategy::new()
            .add_strategy(RecoveryStrategyEnum::Retry(
                RetryStrategy::with_default_backoff(1),
            ))
            .add_strategy(RecoveryStrategyEnum::Timeout(TimeoutStrategy::new(
                Duration::seconds(1),
            )));

        let factory: FutureFactory = Arc::new(move || {
            let seen = counter.fetch_add(1, Ordering::SeqCst);
            if seen == 0 {
                Box::pin(async_ok(VerificationResult::Unknown))
            } else {
                Box::pin(async_ok(VerificationResult::Verified))
            }
        });

        let outcome = composite.execute(factory).await;
        assert!(matches!(
            outcome,
            RecoveryOutcome::Success(VerificationResult::Verified)
        ));
    }

    #[tokio::test]
    async fn composite_with_no_strategies_reports_exhaustion() {
        let composite = CompositeStrategy::default();
        let factory: FutureFactory = Arc::new(|| Box::pin(async_ok(VerificationResult::Unknown)));
        let outcome = composite.execute(factory).await;
        assert!(matches!(
            outcome,
            RecoveryOutcome::Failure(RecoveryError::MaxAttemptsExceeded { attempts: 1 })
        ));
    }

    #[test]
    fn composite_is_applicable_when_any_child_strategy_applies() {
        let composite = CompositeStrategy::new().add_strategy(RecoveryStrategyEnum::Timeout(
            TimeoutStrategy::new(Duration::seconds(1)),
        ));
        assert!(composite.is_applicable(VerificationResult::Unknown));
        assert!(!composite.is_applicable(VerificationResult::Failed));

        let empty = CompositeStrategy::default();
        assert!(!empty.is_applicable(VerificationResult::Unknown));
    }

    #[test]
    fn composite_strategy_is_debug_and_clone() {
        let composite = CompositeStrategy::new().add_strategy(RecoveryStrategyEnum::Retry(
            RetryStrategy::with_default_backoff(1),
        ));
        let cloned = composite.clone();
        assert!(cloned.is_applicable(VerificationResult::Unknown));
        assert!(std::format!("{composite:?}").contains("CompositeStrategy"));
    }

    #[test]
    fn recovery_errors_render_for_every_variant() {
        let errors = [
            RecoveryError::MaxAttemptsExceeded { attempts: 3 },
            RecoveryError::CircuitBreakerOpen,
            RecoveryError::Timeout {
                duration: Duration::seconds(2),
            },
            RecoveryError::AllFallbacksExhausted,
            RecoveryError::NotApplicable {
                result: VerificationResult::Unknown,
            },
            RecoveryError::UnderlyingError {
                context: "provider 500".into(),
            },
        ];
        let rendered: Vec<String> = errors.iter().map(ToString::to_string).collect();
        assert_eq!(rendered[0], "Maximum retry attempts (3) exceeded");
        assert_eq!(rendered[1], "Circuit breaker is open, retry not allowed");
        assert_eq!(rendered[2], "Operation timed out after PT2S");
        assert_eq!(rendered[3], "All fallback strategies exhausted");
        assert_eq!(rendered[4], "Recovery not applicable for result: unknown");
        assert_eq!(rendered[5], "Recovery failed: provider 500");

        let cloned = errors[0].clone();
        assert!(std::format!("{cloned:?}").contains("MaxAttemptsExceeded"));
    }

    #[test]
    fn recovery_outcomes_are_debuggable() {
        let outcomes = vec![
            RecoveryOutcome::Success(VerificationResult::Verified),
            RecoveryOutcome::Failure(RecoveryError::CircuitBreakerOpen),
            RecoveryOutcome::Failure(RecoveryError::NotApplicable {
                result: VerificationResult::Unknown,
            }),
        ];
        let debugged = std::format!("{outcomes:?}");
        assert!(debugged.contains("Success"));
        assert!(debugged.contains("Failure"));
        assert!(debugged.contains("NotApplicable"));
    }

    #[tokio::test]
    async fn execute_fallback_chain_returns_error_when_all_fail() {
        let result = execute_fallback_chain(vec![
            (
                "first".to_string(),
                Box::new(|| async_err(RecoveryError::MaxAttemptsExceeded { attempts: 1 }))
                    as Box<dyn Fn() -> _>,
            ),
            (
                "second".to_string(),
                Box::new(|| async_err(RecoveryError::CircuitBreakerOpen)) as Box<dyn Fn() -> _>,
            ),
        ])
        .await;

        assert!(result.is_err());
    }
}
