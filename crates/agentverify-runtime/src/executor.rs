//! Verified executor implementation
//!
//! Orchestrates action execution with verification using verify-before-retry semantics.

use crate::action_executor::{ActionExecutor, DispatchOutcome};
use crate::receipt_store::ReceiptStore;
use agentverify_contract::Contract;
use agentverify_core::{
    Action, BackoffType, Observation, PostconditionResult, Receipt, ReceiptId, RecoveryConfig,
    RecoveryStrategy, SourceId, State, StateMachine, VerificationResult,
};
use agentverify_engine::PredicateEngine;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;
use tokio::time::timeout as tokio_timeout;

/// Errors raised while executing and verifying an action
#[derive(Debug, Error)]
pub enum ExecutorError {
    /// No contract was registered for the action name
    #[error("Contract not found for action: {0}")]
    ContractNotFound(String),
    /// A precondition evaluated to a non-verified result
    #[error("Precondition failed: {0}")]
    PreconditionFailed(String),
    /// Dispatching or executing the action failed
    #[error("Execution failed: {0}")]
    ExecutionFailed(String),
    /// A postcondition could not be evaluated
    #[error("Verification failed: {0}")]
    VerificationFailed(String),
    /// An operation exceeded its allotted time
    #[error("Timeout: {0}")]
    Timeout(String),
    /// The outcome could not be determined
    #[error("Unknown result: {0}")]
    Unknown(String),
    /// The idempotency key was already claimed and completed elsewhere
    #[error("Idempotency conflict: action already executed")]
    IdempotencyConflict,
    /// All retry attempts were consumed without a verified outcome
    #[error("Retry exhausted after {attempts} attempts")]
    RetryExhausted {
        /// Number of attempts made before giving up
        attempts: u32,
    },
    /// The action executor adapter reported an error
    #[error("Action executor error: {0}")]
    ActionExecutor(String),
}

/// Observer trait for collecting state observations
#[async_trait::async_trait]
pub trait Observer: Send + Sync {
    /// Observe the system state and return an observation
    async fn observe(
        &self,
        action: &Action,
        contract: &Contract,
    ) -> Result<Observation, ExecutorError>;
}

/// Executor configuration
#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    /// Default verification timeout in milliseconds
    pub verification_timeout_ms: u64,
    /// Maximum retry attempts
    pub max_retries: u32,
    /// Enable verify-before-retry
    pub verify_before_retry: bool,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            verification_timeout_ms: 5000,
            max_retries: 3,
            verify_before_retry: true,
        }
    }
}

/// Result of an idempotency claim attempt
#[derive(Debug, Clone, PartialEq)]
pub enum ClaimResult {
    /// Key was claimed by this call (first request wins)
    Claimed,
    /// Key was already claimed by another concurrent request
    AlreadyClaimed,
}

/// Idempotency store trait for tracking executed actions
///
/// Implement this trait to provide custom idempotency storage:
/// - In-memory for tests (`IdempotencyRegistry`)
/// - Redis for distributed systems (atomic SETNX + GET)
/// - `PostgreSQL` for durable storage (ON CONFLICT DO UPDATE)
///
/// # Atomic semantics
/// The `claim_or_check` operation is atomic: it prevents two concurrent
/// requests from both dispatching the same action. Only the first to call
/// `claim_or_check` will receive `Claimed`; all others get `AlreadyClaimed`
/// with the in-flight or completed result.
///
/// # Key lifecycle
/// 1. `claim_or_check(key)` → (Claimed, None) — caller is responsible for dispatch
/// 2. `complete(key, result)` — store final result; other requests now see the result
///    (on release) `release(key)` — could not dispatch; allow subsequent requests to retry
///
/// # TTL
/// Implementors should expire entries after TTL to prevent unbounded growth
/// and to allow retry of genuinely failed actions.
pub trait IdempotencyStore: Send + Sync {
    /// Atomically claim a key or return the existing result if already claimed/completed
    ///
    /// Returns `(ClaimResult, optional_result)`:
    /// - `(Claimed, None)` — key was claimed by this call; caller is responsible for dispatch
    /// - `(AlreadyClaimed, Some(result))` — key was already claimed (in-flight) or completed
    fn claim_or_check<'a>(
        &'a self,
        key: &'a str,
    ) -> Pin<Box<dyn Future<Output = (ClaimResult, Option<VerificationResult>)> + Send + 'a>>;

    /// Complete a claimed key with the final result
    ///
    /// # Panics
    /// Panics if the key is not currently claimed. Use `claim_or_check` first.
    fn complete(
        &self,
        key: String,
        result: VerificationResult,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;

    /// Release a claimed key without storing a result (dispatch failed, allow retry)
    fn release(&self, key: &str) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;
}

/// In-memory idempotency registry for process-local use
///
/// # Atomicity
/// Uses `tokio::sync::Mutex` to serialize all claim/check/complete operations.
/// This provides atomic claim semantics: only the first concurrent caller to
/// `claim_or_check` for a given key will receive `Claimed`; all others
/// receive `AlreadyClaimed` with the in-flight or completed result.
///
/// # Limitations
/// - Process-local only: does not persist across restarts
/// - No TTL: entries live until process exits
/// - No graceful expiry: entries accumulate until process exit
///
/// For production, use a distributed store (Redis, `PostgreSQL`) implementing `IdempotencyStore`.
pub struct IdempotencyRegistry {
    entries: Mutex<HashMap<String, EntryState>>,
}

/// Internal state of an idempotency key
#[derive(Debug, Clone)]
enum EntryState {
    /// Action is in-flight (claimed but not yet complete)
    InFlight,
    /// Action completed with this result
    Completed(VerificationResult),
}

impl IdempotencyRegistry {
    /// Create an empty registry
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }
}

impl IdempotencyStore for IdempotencyRegistry {
    fn claim_or_check<'a>(
        &'a self,
        key: &'a str,
    ) -> Pin<Box<dyn Future<Output = (ClaimResult, Option<VerificationResult>)> + Send + 'a>> {
        Box::pin(async move {
            let mut guard = self.entries.lock().await;
            match guard.entry(key.to_string()) {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    // Key doesn't exist — atomically claim it
                    entry.insert(EntryState::InFlight);
                    (ClaimResult::Claimed, None)
                }
                std::collections::hash_map::Entry::Occupied(entry) => {
                    // Key exists — return current state
                    match entry.get() {
                        EntryState::InFlight => (ClaimResult::AlreadyClaimed, None),
                        EntryState::Completed(result) => {
                            (ClaimResult::AlreadyClaimed, Some(*result))
                        }
                    }
                }
            }
        })
    }

    fn complete(
        &self,
        key: String,
        result: VerificationResult,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            let mut guard = self.entries.lock().await;
            guard.insert(key, EntryState::Completed(result));
        })
    }

    fn release(&self, key: &str) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let key = key.to_string();
        Box::pin(async move {
            let mut guard = self.entries.lock().await;
            guard.remove(&key);
        })
    }
}

impl Default for IdempotencyRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Aggregate outcome of postcondition verification
///
/// Pairs the final [`VerificationResult`] with the per-postcondition evidence
/// recorded into the receipt.
#[derive(Debug, Clone)]
struct PostconditionEvaluation {
    /// Aggregate verification outcome
    result: VerificationResult,
    /// Per-postcondition evidence, in contract order
    details: Vec<PostconditionResult>,
}

/// Verified executor
pub struct Executor {
    config: ExecutorConfig,
    idempotency: Arc<dyn IdempotencyStore>,
    receipt_store: Option<Arc<dyn ReceiptStore>>,
}

impl Executor {
    /// Create a new executor with default configuration
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(ExecutorConfig::default())
    }

    /// Create a new executor with custom configuration and idempotency store
    pub fn with_config_and_store(config: ExecutorConfig, store: Arc<dyn IdempotencyStore>) -> Self {
        Self {
            config,
            idempotency: store,
            receipt_store: None,
        }
    }

    /// Create a new executor with custom configuration (uses in-memory store)
    #[must_use]
    pub fn with_config(config: ExecutorConfig) -> Self {
        Self {
            config,
            idempotency: Arc::new(IdempotencyRegistry::new()),
            receipt_store: None,
        }
    }

    /// Create a new executor with a receipt store attached
    ///
    /// The receipt store is used to persist receipts after execution completes.
    /// If no store is attached, receipts are still returned but not persisted.
    pub fn with_receipt_store(
        config: ExecutorConfig,
        idempotency: Arc<dyn IdempotencyStore>,
        receipt_store: Arc<dyn ReceiptStore>,
    ) -> Self {
        Self {
            config,
            idempotency,
            receipt_store: Some(receipt_store),
        }
    }

    /// Retrieve a stored receipt by ID
    pub async fn get_receipt(&self, id: &ReceiptId) -> Option<Receipt> {
        let store = self.receipt_store.as_ref()?;
        store.get(id).await
    }

    /// Store a receipt in the attached receipt store (if any)
    ///
    /// A persistence failure is logged and does not abort verification: the
    /// outcome was already determined, but the evidence gap is observable.
    async fn store_receipt(&self, receipt: &Receipt) {
        if let Some(store) = &self.receipt_store {
            if let Err(e) = store.store(receipt).await {
                tracing::warn!(
                    receipt_id = %receipt.id,
                    error = %e,
                    "failed to persist verification receipt"
                );
            }
        }
    }

    /// Calculate the next backoff delay in milliseconds
    ///
    /// Uses the recovery config's backoff settings if available,
    /// otherwise falls back to the executor config's default values.
    // Backoff arithmetic intentionally mixes integer millisecond durations with
    // floating-point multipliers; values are small and the result is clamped, so
    // the lossy casts cannot change the resulting delay.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        clippy::cast_possible_wrap
    )]
    fn calculate_backoff(
        attempt: u32,
        recovery: Option<&RecoveryConfig>,
        default_initial: u64,
        default_max: u64,
        default_multiplier: f64,
    ) -> u64 {
        if let Some(config) = recovery {
            if let Some(ref backoff) = config.backoff {
                let initial_ms = backoff.initial.num_milliseconds() as u64;
                let max_ms = backoff.max.num_milliseconds() as u64;
                let multiplier = backoff.multiplier;

                let delay = match backoff.backoff_type {
                    BackoffType::Linear => initial_ms * u64::from(attempt),
                    BackoffType::Exponential => {
                        (initial_ms as f64 * multiplier.powi(attempt as i32 - 1)) as u64
                    }
                };
                return delay.min(max_ms);
            }
        }
        // Fall back to exponential backoff with defaults
        let delay = (default_initial as f64 * default_multiplier.powi(attempt as i32 - 1)) as u64;
        delay.min(default_max)
    }

    /// Determine if we should retry based on recovery config
    ///
    /// Returns (`should_retry`, `max_attempts`) based on the recovery config
    /// and the current attempt number.
    fn should_retry(
        attempts: u32,
        recovery: Option<&RecoveryConfig>,
        default_max_retries: u32,
    ) -> bool {
        if let Some(config) = recovery {
            // NoAction strategy means no retry
            if config.strategy == RecoveryStrategy::NoAction {
                return false;
            }
            return attempts < config.max_attempts;
        }
        // Fall back to executor config
        attempts < default_max_retries
    }

    /// Execute an action with verification (simulated dispatch)
    ///
    /// This method simulates dispatch for testing/development. For real dispatch,
    /// use `execute_with_executor` which accepts an `ActionExecutor` adapter.
    ///
    /// # Process
    /// 1. Atomically claim idempotency key (only first caller dispatches)
    /// 2. Validate preconditions
    /// 3. Simulate execution (always returns Unknown)
    /// 4. Observe the result state
    /// 5. Verify postconditions
    /// 6. Complete idempotency entry
    ///
    /// # Errors
    ///
    /// Returns [`ExecutorError::PreconditionFailed`] when preconditions do not
    /// hold, [`ExecutorError::ExecutionFailed`] when the state machine cannot
    /// advance, and [`ExecutorError::VerificationFailed`] when postconditions
    /// cannot be evaluated.
    #[allow(clippy::too_many_lines)] // single linear pipeline; splitting it obscures the flow
    pub async fn execute(
        &self,
        action: Action,
        contract: Contract,
        observer: Option<Arc<dyn Observer>>,
    ) -> Result<(VerificationResult, Receipt), ExecutorError> {
        // Atomically claim or check idempotency key
        if let Some(ref key) = action.idempotency_key {
            let (result, existing) = self.idempotency.claim_or_check(&key.0).await;
            match result {
                ClaimResult::Claimed => {
                    // We won — proceed with execution
                }
                ClaimResult::AlreadyClaimed => {
                    if let Some(cached) = existing {
                        let receipt =
                            Self::create_receipt(&action, &contract, cached, 0, Vec::new());
                        return Ok((cached, receipt));
                    }
                    // In-flight — treat as duplicate
                    let receipt = Self::create_receipt(
                        &action,
                        &contract,
                        VerificationResult::Duplicate,
                        0,
                        Vec::new(),
                    );
                    return Ok((VerificationResult::Duplicate, receipt));
                }
            }
        }

        let mut attempts = 0;
        let final_result: Option<VerificationResult>;
        let mut final_observation: Option<Observation> = None;
        let mut postcondition_details = Vec::new();

        // Main execution loop
        loop {
            attempts += 1;
            let mut state_machine = StateMachine::new();

            // Validate preconditions
            state_machine
                .advance(State::Validating)
                .map_err(|e| ExecutorError::PreconditionFailed(e.to_string()))?;

            if let Err(e) = Self::validate_preconditions(&action, &contract) {
                state_machine
                    .advance(State::Rejected)
                    .map_err(|_| ExecutorError::PreconditionFailed(e.clone()))?;
                // Precondition failure is terminal for this action — complete with Failed
                final_result = Some(VerificationResult::Failed);
                break;
            }

            state_machine
                .advance(State::Authorized)
                .map_err(|e| ExecutorError::ExecutionFailed(e.to_string()))?;

            // Execute action (simulated)
            state_machine
                .advance(State::Executing)
                .map_err(|e| ExecutorError::ExecutionFailed(e.to_string()))?;

            // Simulate execution — no real dispatch available in this path
            state_machine
                .advance(State::Unknown)
                .map_err(|e| ExecutorError::Unknown(e.to_string()))?;

            // Observe state
            state_machine
                .advance(State::Observing)
                .map_err(|e| ExecutorError::Unknown(e.to_string()))?;

            let timeout_duration =
                std::time::Duration::from_millis(self.config.verification_timeout_ms);
            let observation = if let Some(ref obs) = observer {
                match tokio_timeout(timeout_duration, obs.observe(&action, &contract)).await {
                    Ok(Ok(obs)) => obs,
                    Ok(Err(e)) => return Err(e),
                    Err(_) => {
                        return Err(ExecutorError::Timeout(format!(
                            "Observation timed out after {}ms",
                            self.config.verification_timeout_ms
                        )));
                    }
                }
            } else {
                Observation::new(SourceId("none".into()), serde_json::json!({}))
            };

            // Verify postconditions
            state_machine
                .advance(State::Verifying)
                .map_err(|e| ExecutorError::VerificationFailed(e.to_string()))?;

            let evaluation = Self::verify_postconditions(&action, &contract, &observation)?;
            postcondition_details = evaluation.details;

            // Determine final state
            match evaluation.result {
                VerificationResult::Verified => {
                    state_machine
                        .advance(State::Verified)
                        .map_err(|e| ExecutorError::VerificationFailed(e.to_string()))?;
                    state_machine
                        .advance(State::Committed)
                        .map_err(|e| ExecutorError::VerificationFailed(e.to_string()))?;
                    final_result = Some(VerificationResult::Verified);
                    final_observation = Some(observation);
                    break;
                }
                VerificationResult::Failed => {
                    let backoff_ms = Self::calculate_backoff(
                        attempts,
                        contract.recovery.as_ref(),
                        100,  // default_initial
                        5000, // default_max
                        2.0,  // default_multiplier
                    );
                    // Only retry on Failed if verify_before_retry is enabled AND should_retry says to
                    if self.config.verify_before_retry
                        && Self::should_retry(
                            attempts,
                            contract.recovery.as_ref(),
                            self.config.max_retries,
                        )
                    {
                        tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
                        continue;
                    }
                    state_machine
                        .advance(State::VerificationFailed)
                        .map_err(|e| ExecutorError::VerificationFailed(e.to_string()))?;
                    final_result = Some(VerificationResult::Failed);
                    break;
                }
                VerificationResult::Unknown => {
                    let backoff_ms = Self::calculate_backoff(
                        attempts,
                        contract.recovery.as_ref(),
                        100,  // default_initial
                        5000, // default_max
                        2.0,  // default_multiplier
                    );
                    if Self::should_retry(
                        attempts,
                        contract.recovery.as_ref(),
                        self.config.max_retries,
                    ) {
                        tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
                        continue;
                    }
                    final_result = Some(VerificationResult::Unknown);
                    break;
                }
                _ => {
                    // Partial, Duplicate — terminal
                    final_result = Some(evaluation.result);
                    break;
                }
            }
        }

        // Complete idempotency entry
        let result = final_result.unwrap_or(VerificationResult::Failed);
        if let Some(ref key) = action.idempotency_key {
            self.idempotency.complete(key.0.clone(), result).await;
        }

        let receipt = if let Some(obs) = final_observation {
            Self::create_receipt_with_observation(
                &action,
                &contract,
                result,
                attempts,
                obs,
                postcondition_details,
            )
        } else {
            Self::create_receipt(&action, &contract, result, attempts, postcondition_details)
        };

        // Persist receipt if a store is attached
        self.store_receipt(&receipt).await;

        Ok((result, receipt))
    }

    /// Validate preconditions against current state
    fn validate_preconditions(_action: &Action, contract: &Contract) -> Result<(), String> {
        for precond in &contract.preconditions {
            let result = PredicateEngine::default()
                .evaluate(
                    &precond.predicate,
                    &serde_json::json!({}), // No state yet
                    &serde_json::json!({}), // No args yet
                )
                .map_err(|e| e.to_string())?;

            if !matches!(result, VerificationResult::Verified) {
                return Err(format!("Precondition failed: {}", precond.description));
            }
        }
        Ok(())
    }

    /// Verify postconditions against observed state
    ///
    /// Returns the aggregate [`VerificationResult`] together with a
    /// [`PostconditionResult`] per evaluated postcondition, so receipts carry
    /// the per-predicate evidence rather than only the final outcome. When a
    /// predicate evaluates to an indeterminate result (`Unknown`, `Partial`,
    /// `Duplicate`), evaluation stops: the details cover exactly the
    /// postconditions that were evaluated, and the indeterminate one is
    /// recorded with its outcome in `error`.
    fn verify_postconditions(
        action: &Action,
        contract: &Contract,
        observation: &Observation,
    ) -> Result<PostconditionEvaluation, ExecutorError> {
        let mut all_passed = true;
        let mut mandatory_failed = false;
        let mut details = Vec::with_capacity(contract.postconditions.len());

        for postcond in &contract.postconditions {
            let result = PredicateEngine::default()
                .evaluate(&postcond.predicate, &observation.state, &action.arguments)
                .map_err(|e| ExecutorError::VerificationFailed(e.to_string()))?;

            match result {
                VerificationResult::Verified => {
                    details.push(PostconditionResult {
                        predicate: postcond.predicate.clone(),
                        description: postcond.description.clone(),
                        passed: true,
                        error: None,
                    });
                }
                VerificationResult::Failed => {
                    if postcond.mandatory {
                        mandatory_failed = true;
                    }
                    all_passed = false;
                    details.push(PostconditionResult {
                        predicate: postcond.predicate.clone(),
                        description: postcond.description.clone(),
                        passed: false,
                        error: None,
                    });
                }
                other => {
                    // Unknown, Partial, Duplicate — indeterminate: stop
                    // evaluating and surface the outcome. The predicate was
                    // not shown to fail, so `passed` stays false and the
                    // indeterminate outcome is preserved in `error`.
                    details.push(PostconditionResult {
                        predicate: postcond.predicate.clone(),
                        description: postcond.description.clone(),
                        passed: false,
                        error: Some(format!("evaluation returned {other}")),
                    });
                    return Ok(PostconditionEvaluation {
                        result: other,
                        details,
                    });
                }
            }
        }

        let result = if all_passed {
            VerificationResult::Verified
        } else if mandatory_failed {
            VerificationResult::Failed
        } else {
            VerificationResult::Partial
        };
        Ok(PostconditionEvaluation { result, details })
    }

    /// Execute an action using a real action executor
    ///
    /// This method uses the verify-before-retry pattern with atomic idempotency:
    /// 1. Atomically claim idempotency key (only first caller dispatches)
    /// 2. Validate preconditions
    /// 3. Dispatch the action through the executor
    /// 4. Observe the result state
    /// 5. Verify postconditions
    /// 6. Complete the idempotency entry with the result
    ///
    /// Timeouts are treated as UNKNOWN, not failure. The caller must observe
    /// state before retrying. On transport error, the claim is released so
    /// a subsequent request may retry.
    ///
    /// # Errors
    ///
    /// Returns [`ExecutorError::PreconditionFailed`] when preconditions do not
    /// hold, [`ExecutorError::ActionExecutor`] when dispatch fails, and
    /// [`ExecutorError::VerificationFailed`] when postconditions cannot be
    /// evaluated.
    #[allow(clippy::too_many_lines)] // single linear pipeline; splitting it obscures the flow
    pub async fn execute_with_executor(
        &self,
        action: Action,
        contract: Contract,
        action_executor: Arc<dyn ActionExecutor>,
        observer: Option<Arc<dyn Observer>>,
    ) -> Result<(VerificationResult, Receipt), ExecutorError> {
        use tokio::time::sleep;

        // Atomically claim or check the idempotency key
        let _claimed = if let Some(ref key) = action.idempotency_key {
            let (result, existing) = self.idempotency.claim_or_check(&key.0).await;
            match result {
                ClaimResult::Claimed => {
                    // We won the claim — we are responsible for dispatch
                    false
                }
                ClaimResult::AlreadyClaimed => {
                    // Another request is already handling this action
                    if let Some(cached) = existing {
                        // Already completed — return cached result
                        let receipt =
                            Self::create_receipt(&action, &contract, cached, 0, Vec::new());
                        return Ok((cached, receipt));
                    }
                    // In-flight — wait briefly and poll for completion
                    sleep(std::time::Duration::from_millis(50)).await;
                    let (_, existing) = self.idempotency.claim_or_check(&key.0).await;
                    if let Some(cached) = existing {
                        let receipt =
                            Self::create_receipt(&action, &contract, cached, 0, Vec::new());
                        return Ok((cached, receipt));
                    }
                    // Still in-flight after poll — treat as duplicate
                    let receipt = Self::create_receipt(
                        &action,
                        &contract,
                        VerificationResult::Duplicate,
                        0,
                        Vec::new(),
                    );
                    return Ok((VerificationResult::Duplicate, receipt));
                }
            }
        } else {
            // No idempotency key — proceed without claim
            false
        };

        let mut attempts = 0;
        let final_result: Option<VerificationResult>;
        let mut final_observation: Option<Observation> = None;
        let mut postcondition_details = Vec::new();

        // Main execution loop
        loop {
            attempts += 1;
            let mut state_machine = StateMachine::new();

            // Validate preconditions
            state_machine
                .advance(State::Validating)
                .map_err(|e| ExecutorError::PreconditionFailed(e.to_string()))?;

            if let Err(e) = Self::validate_preconditions(&action, &contract) {
                state_machine
                    .advance(State::Rejected)
                    .map_err(|_| ExecutorError::PreconditionFailed(e.clone()))?;
                final_result = Some(VerificationResult::Failed);
                break;
            }

            state_machine
                .advance(State::Authorized)
                .map_err(|e| ExecutorError::ExecutionFailed(e.to_string()))?;

            // Execute action through executor
            state_machine
                .advance(State::Executing)
                .map_err(|e| ExecutorError::ExecutionFailed(e.to_string()))?;

            let dispatch_result = action_executor
                .execute(&action)
                .await
                .map_err(|e| ExecutorError::ActionExecutor(e.to_string()))?;

            // Handle dispatch outcome
            match dispatch_result {
                DispatchOutcome::Completed | DispatchOutcome::Accepted => {
                    state_machine
                        .advance(State::Executed)
                        .map_err(|e| ExecutorError::ExecutionFailed(e.to_string()))?;
                }
                DispatchOutcome::TimeoutBeforeDispatch | DispatchOutcome::TimeoutAfterDispatch => {
                    state_machine
                        .advance(State::Unknown)
                        .map_err(|e| ExecutorError::Timeout(e.to_string()))?;
                }
                DispatchOutcome::TransportError(_) => {
                    // Terminal — release claim and return Failed
                    if let Some(ref key) = action.idempotency_key {
                        self.idempotency.release(&key.0).await;
                    }
                    let receipt = Self::create_receipt(
                        &action,
                        &contract,
                        VerificationResult::Failed,
                        attempts,
                        Vec::new(),
                    );
                    return Ok((VerificationResult::Failed, receipt));
                }
                DispatchOutcome::Ambiguous(_) => {
                    // Terminal — complete with Unknown
                    final_result = Some(VerificationResult::Unknown);
                    break;
                }
            }

            // Observe state
            state_machine
                .advance(State::Observing)
                .map_err(|e| ExecutorError::Unknown(e.to_string()))?;

            let timeout_duration =
                std::time::Duration::from_millis(self.config.verification_timeout_ms);
            let observation = match observer {
                Some(ref obs) => {
                    match tokio_timeout(timeout_duration, obs.observe(&action, &contract)).await {
                        Ok(Ok(obs)) => obs,
                        Ok(Err(ExecutorError::Unknown(_msg))) => {
                            final_result = Some(VerificationResult::Unknown);
                            break;
                        }
                        Ok(Err(e)) => return Err(e),
                        Err(_) => {
                            return Err(ExecutorError::Timeout(format!(
                                "Observation timed out after {}ms",
                                self.config.verification_timeout_ms
                            )));
                        }
                    }
                }
                None => Observation::new(SourceId("none".into()), serde_json::json!({})),
            };

            // Verify postconditions
            state_machine
                .advance(State::Verifying)
                .map_err(|e| ExecutorError::VerificationFailed(e.to_string()))?;

            let evaluation = Self::verify_postconditions(&action, &contract, &observation)?;
            postcondition_details = evaluation.details;

            // Determine final state
            match evaluation.result {
                VerificationResult::Verified => {
                    state_machine
                        .advance(State::Verified)
                        .map_err(|e| ExecutorError::VerificationFailed(e.to_string()))?;
                    state_machine
                        .advance(State::Committed)
                        .map_err(|e| ExecutorError::VerificationFailed(e.to_string()))?;
                    final_result = Some(VerificationResult::Verified);
                    final_observation = Some(observation);
                    break;
                }
                VerificationResult::Failed => {
                    let backoff_ms = Self::calculate_backoff(
                        attempts,
                        contract.recovery.as_ref(),
                        100,  // default_initial
                        5000, // default_max
                        2.0,  // default_multiplier
                    );
                    if self.config.verify_before_retry
                        && Self::should_retry(
                            attempts,
                            contract.recovery.as_ref(),
                            self.config.max_retries,
                        )
                    {
                        sleep(std::time::Duration::from_millis(backoff_ms)).await;
                        continue;
                    }
                    state_machine
                        .advance(State::VerificationFailed)
                        .map_err(|e| ExecutorError::VerificationFailed(e.to_string()))?;
                    final_result = Some(VerificationResult::Failed);
                    break;
                }
                VerificationResult::Unknown => {
                    let backoff_ms = Self::calculate_backoff(
                        attempts,
                        contract.recovery.as_ref(),
                        100,  // default_initial
                        5000, // default_max
                        2.0,  // default_multiplier
                    );
                    if Self::should_retry(
                        attempts,
                        contract.recovery.as_ref(),
                        self.config.max_retries,
                    ) {
                        sleep(std::time::Duration::from_millis(backoff_ms)).await;
                        continue;
                    }
                    final_result = Some(VerificationResult::Unknown);
                    break;
                }
                _ => {
                    // Partial, Duplicate — terminal
                    final_result = Some(evaluation.result);
                    break;
                }
            }
        }

        // Complete idempotency entry and return
        let result = final_result.unwrap_or(VerificationResult::Failed);
        if let Some(ref key) = action.idempotency_key {
            self.idempotency.complete(key.0.clone(), result).await;
        }

        let receipt = if let Some(obs) = final_observation {
            Self::create_receipt_with_observation(
                &action,
                &contract,
                result,
                attempts,
                obs,
                postcondition_details,
            )
        } else {
            Self::create_receipt(&action, &contract, result, attempts, postcondition_details)
        };

        // Persist receipt if a store is attached
        self.store_receipt(&receipt).await;

        Ok((result, receipt))
    }

    /// Create a receipt for the action
    ///
    /// `postcondition_details` carries the per-postcondition evidence from
    /// [`Self::verify_postconditions`]; paths that never evaluated
    /// postconditions (duplicate claims, dispatch failures) pass an empty
    /// vector, and the receipt records exactly what was evaluated.
    fn create_receipt(
        action: &Action,
        contract: &Contract,
        result: VerificationResult,
        attempts: u32,
        postcondition_details: Vec<PostconditionResult>,
    ) -> Receipt {
        let idempotency_key = action.idempotency_key.as_ref().map(|k| k.0.clone());
        let mut receipt = Receipt::with_contract_version_and_key(
            action.id,
            contract.id,
            contract.schema_version.clone(),
            result,
            attempts,
            idempotency_key,
        );
        for detail in postcondition_details {
            receipt = receipt.with_postcondition_result(detail);
        }
        receipt
    }

    /// Create a receipt with an observation attached
    fn create_receipt_with_observation(
        action: &Action,
        contract: &Contract,
        result: VerificationResult,
        attempts: u32,
        observation: Observation,
        postcondition_details: Vec<PostconditionResult>,
    ) -> Receipt {
        let idempotency_key = action.idempotency_key.as_ref().map(|k| k.0.clone());
        let mut receipt = Receipt::with_contract_version_and_key(
            action.id,
            contract.id,
            contract.schema_version.clone(),
            result,
            attempts,
            idempotency_key,
        )
        .with_observation(observation);
        for detail in postcondition_details {
            receipt = receipt.with_postcondition_result(detail);
        }
        receipt
    }
}

impl Default for Executor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action_executor::DispatchError;
    use crate::idempotency_store::FileIdempotencyStore;
    use agentverify_core::{
        BackoffConfig, BackoffType, IdempotencyKey, Postcondition, Predicate, RecoveryConfig,
    };
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::time::sleep;

    #[tokio::test]
    async fn executor_returns_verified_on_empty_postconditions() {
        let executor = Executor::new();
        let action = Action::new("test", serde_json::json!({}));
        let contract = Contract::new("test").with_postcondition(Predicate::exists("x"), "x exists");

        // No observer, so observation will be empty
        let result = executor.execute(action, contract, None).await;
        assert!(result.is_ok());

        let (verification_result, _receipt) = result.unwrap();
        // With no observer, state is empty, so postcondition should fail
        assert_eq!(verification_result, VerificationResult::Failed);
    }

    #[tokio::test]
    async fn executor_idempotency_returns_cached() {
        use agentverify_core::IdempotencyKey;

        let executor = Executor::new();
        let key = IdempotencyKey::new("test-key");
        let action = Action::with_idempotency("test", serde_json::json!({}), key);
        let contract =
            Contract::new("test").with_postcondition(Predicate::exists("result"), "result exists");

        // First execution - no observer
        let result1 = executor
            .execute(action.clone(), contract.clone(), None)
            .await;
        assert!(result1.is_ok());

        // Create new action with same idempotency key
        let key2 = IdempotencyKey::new("test-key");
        let action2 = Action::with_idempotency("test", serde_json::json!({}), key2);

        // Second execution should return cached result
        let result2 = executor.execute(action2, contract, None).await;
        assert!(result2.is_ok());
        let (verification_result2, receipt2) = result2.unwrap();
        // Should be the same result, with 0 attempts (cached)
        assert_eq!(verification_result2, VerificationResult::Failed);
        assert_eq!(receipt2.attempts, 0);
    }

    #[tokio::test]
    async fn execute_with_executor_timeout_before_dispatch_with_empty_state_fails() {
        use crate::action_executor::{ActionExecutor, DispatchError, DispatchOutcome};
        use std::sync::Arc;

        struct MockExecutor {
            outcome: DispatchOutcome,
        }

        #[async_trait::async_trait]
        impl ActionExecutor for MockExecutor {
            async fn execute(&self, _action: &Action) -> Result<DispatchOutcome, DispatchError> {
                Ok(self.outcome.clone())
            }
        }

        let executor = Executor::new();
        let action = Action::new("test", serde_json::json!({}));
        let contract =
            Contract::new("test").with_postcondition(Predicate::exists("result"), "result exists");

        let mock = Arc::new(MockExecutor {
            outcome: DispatchOutcome::TimeoutBeforeDispatch,
        });

        let result = executor
            .execute_with_executor(action, contract, mock, None)
            .await;

        assert!(result.is_ok());
        let (verification_result, receipt) = result.unwrap();
        // With no observer, empty state causes verification to fail
        // This is correct: we cannot verify without observation
        assert_eq!(verification_result, VerificationResult::Failed);
        // With default max_retries=3, we get 3 attempts
        assert_eq!(receipt.attempts, 3);
    }

    #[tokio::test]
    async fn execute_with_executor_transport_error_is_terminal() {
        use crate::action_executor::{ActionExecutor, DispatchError, DispatchOutcome};
        use std::sync::Arc;

        struct MockExecutor {
            outcome: DispatchOutcome,
        }

        #[async_trait::async_trait]
        impl ActionExecutor for MockExecutor {
            async fn execute(&self, _action: &Action) -> Result<DispatchOutcome, DispatchError> {
                Ok(self.outcome.clone())
            }
        }

        let executor = Executor::new();
        let action = Action::new("test", serde_json::json!({}));
        let contract =
            Contract::new("test").with_postcondition(Predicate::exists("result"), "result exists");

        let mock = Arc::new(MockExecutor {
            outcome: DispatchOutcome::TransportError("connection refused".to_string()),
        });

        let result = executor
            .execute_with_executor(action, contract, mock, None)
            .await;

        assert!(result.is_ok());
        let (verification_result, receipt) = result.unwrap();
        // TransportError is terminal - should be Failed immediately, not retried
        assert_eq!(verification_result, VerificationResult::Failed);
        assert_eq!(receipt.attempts, 1); // First attempt
    }

    #[tokio::test]
    async fn execute_with_executor_ambiguous_is_terminal_unknown() {
        use crate::action_executor::{ActionExecutor, DispatchError, DispatchOutcome};
        use std::sync::Arc;

        struct MockExecutor {
            outcome: DispatchOutcome,
        }

        #[async_trait::async_trait]
        impl ActionExecutor for MockExecutor {
            async fn execute(&self, _action: &Action) -> Result<DispatchOutcome, DispatchError> {
                Ok(self.outcome.clone())
            }
        }

        let executor = Executor::new();
        let action = Action::new("test", serde_json::json!({}));
        let contract =
            Contract::new("test").with_postcondition(Predicate::exists("result"), "result exists");

        let mock = Arc::new(MockExecutor {
            outcome: DispatchOutcome::Ambiguous("result unclear".to_string()),
        });

        let result = executor
            .execute_with_executor(action, contract, mock, None)
            .await;

        assert!(result.is_ok());
        let (verification_result, receipt) = result.unwrap();
        // Ambiguous is terminal - should be Unknown immediately, not retried
        assert_eq!(verification_result, VerificationResult::Unknown);
        assert_eq!(receipt.attempts, 1);
    }

    #[tokio::test]
    async fn execute_with_executor_retry_exhaustion_returns_failed() {
        use crate::action_executor::{ActionExecutor, DispatchError, DispatchOutcome};

        struct MockExecutor;

        #[async_trait::async_trait]
        impl ActionExecutor for MockExecutor {
            async fn execute(&self, _action: &Action) -> Result<DispatchOutcome, DispatchError> {
                Ok(DispatchOutcome::TimeoutBeforeDispatch)
            }
        }

        let config = ExecutorConfig {
            verification_timeout_ms: 5000,
            max_retries: 3,
            verify_before_retry: true,
        };
        let executor = Executor::with_config(config);
        let action = Action::new("test", serde_json::json!({}));
        let contract =
            Contract::new("test").with_postcondition(Predicate::exists("result"), "result exists");

        let result = executor
            .execute_with_executor(action, contract, Arc::new(MockExecutor), None)
            .await;

        assert!(result.is_ok());
        let (verification_result, receipt) = result.unwrap();
        // With no observer, verification fails after exhausting retries
        assert_eq!(verification_result, VerificationResult::Failed);
        // With max_retries=3, we get 3 attempts
        assert_eq!(receipt.attempts, 3);
    }

    #[tokio::test]
    async fn execute_with_executor_observer_error_propagates_as_unknown() {
        use crate::action_executor::{ActionExecutor, DispatchError, DispatchOutcome};

        struct MockExecutor;

        #[async_trait::async_trait]
        impl ActionExecutor for MockExecutor {
            async fn execute(&self, _action: &Action) -> Result<DispatchOutcome, DispatchError> {
                Ok(DispatchOutcome::Completed)
            }
        }

        struct FailingObserver;

        #[async_trait::async_trait]
        impl Observer for FailingObserver {
            async fn observe(
                &self,
                _action: &Action,
                _contract: &Contract,
            ) -> Result<Observation, ExecutorError> {
                Err(ExecutorError::Unknown("Observer unavailable".to_string()))
            }
        }

        let executor = Executor::new();
        let action = Action::new("test", serde_json::json!({}));
        let contract =
            Contract::new("test").with_postcondition(Predicate::exists("result"), "result exists");

        let result = executor
            .execute_with_executor(
                action,
                contract,
                Arc::new(MockExecutor),
                Some(Arc::new(FailingObserver)),
            )
            .await;

        assert!(result.is_ok());
        let (verification_result, receipt) = result.unwrap();
        // Observer error should propagate as Unknown, not Failed
        assert_eq!(verification_result, VerificationResult::Unknown);
        assert_eq!(receipt.attempts, 1);
    }

    #[tokio::test]
    async fn execute_with_executor_stale_read_causes_verification_failure() {
        use crate::action_executor::{ActionExecutor, DispatchError, DispatchOutcome};

        struct MockExecutor;

        #[async_trait::async_trait]
        impl ActionExecutor for MockExecutor {
            async fn execute(&self, _action: &Action) -> Result<DispatchOutcome, DispatchError> {
                Ok(DispatchOutcome::Completed)
            }
        }

        // Observer returns stale data: status is "pending" instead of "completed"
        // The postcondition checks for status == "completed"
        struct StaleObserver;

        #[async_trait::async_trait]
        impl Observer for StaleObserver {
            async fn observe(
                &self,
                _action: &Action,
                _contract: &Contract,
            ) -> Result<Observation, ExecutorError> {
                // Return stale state where the action appears not to have completed
                Ok(Observation::new(
                    SourceId("stale-source".into()),
                    serde_json::json!({
                        "result": {
                            "status": "pending",
                            "updated_at": "2020-01-01T00:00:00Z"
                        }
                    }),
                ))
            }
        }

        let executor = Executor::new();
        let action = Action::new("test", serde_json::json!({}));
        // Postcondition: result.status must equal "completed"
        let contract = Contract::new("test").with_postcondition(
            Predicate::equals("result.status", serde_json::json!("completed")),
            "status must be completed",
        );

        let result = executor
            .execute_with_executor(
                action,
                contract,
                Arc::new(MockExecutor),
                Some(Arc::new(StaleObserver)),
            )
            .await;

        assert!(result.is_ok());
        let (verification_result, _receipt) = result.unwrap();
        // Stale data should cause verification to fail
        assert_eq!(verification_result, VerificationResult::Failed);
    }

    #[tokio::test]
    async fn execute_with_executor_cancellation_returns_unknown() {
        use crate::action_executor::{ActionExecutor, DispatchError, DispatchOutcome};
        use tokio::time::{sleep, Duration};

        struct SlowExecutor;

        #[async_trait::async_trait]
        impl ActionExecutor for SlowExecutor {
            async fn execute(&self, _action: &Action) -> Result<DispatchOutcome, DispatchError> {
                // Simulate slow dispatch
                sleep(Duration::from_secs(10)).await;
                Ok(DispatchOutcome::Completed)
            }
        }

        let executor = Executor::new();
        let action = Action::new("test", serde_json::json!({}));
        let contract =
            Contract::new("test").with_postcondition(Predicate::exists("result"), "result exists");

        // Timeout after 100ms - should trigger cancellation
        let result = tokio::time::timeout(
            Duration::from_millis(100),
            executor.execute_with_executor(action, contract, Arc::new(SlowExecutor), None),
        )
        .await;

        // Should timeout
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn concurrent_idempotency_both_succeed_without_panic() {
        use crate::action_executor::{ActionExecutor, DispatchError, DispatchOutcome};
        use agentverify_core::IdempotencyKey;
        use std::sync::Arc;
        use tokio::time::{sleep, Duration};

        struct SlowExecutor;

        #[async_trait::async_trait]
        impl ActionExecutor for SlowExecutor {
            async fn execute(&self, _action: &Action) -> Result<DispatchOutcome, DispatchError> {
                // Simulate slow dispatch
                sleep(Duration::from_millis(100)).await;
                Ok(DispatchOutcome::Completed)
            }
        }

        struct FastObserver;

        #[async_trait::async_trait]
        impl Observer for FastObserver {
            async fn observe(
                &self,
                _action: &Action,
                _contract: &Contract,
            ) -> Result<Observation, ExecutorError> {
                Ok(Observation::new(
                    SourceId("fast".into()),
                    serde_json::json!({"result": {"status": "completed"}}),
                ))
            }
        }

        let config = ExecutorConfig {
            verification_timeout_ms: 5000,
            max_retries: 3,
            verify_before_retry: true,
        };
        let executor = Arc::new(Executor::with_config(config));
        let idempotency_key = IdempotencyKey::new("concurrent-test-key-2");
        let contract = Contract::new("test").with_postcondition(
            Predicate::equals("result.status", serde_json::json!("completed")),
            "must be completed",
        );

        // Both tasks use the same idempotency key and are dispatched concurrently.
        // The idempotency registry uses a RwLock so concurrent reads are safe.
        // At least one should return Verified (possibly both if the first completed
        // its idempotency check before the second started).
        let fut1 = {
            let executor = executor.clone();
            let action =
                Action::with_idempotency("test", serde_json::json!({}), idempotency_key.clone());
            let contract = contract.clone();
            async move {
                executor
                    .execute_with_executor(
                        action,
                        contract,
                        Arc::new(SlowExecutor),
                        Some(Arc::new(FastObserver)),
                    )
                    .await
            }
        };

        let fut2 = {
            let executor = executor.clone();
            let action =
                Action::with_idempotency("test", serde_json::json!({}), idempotency_key.clone());
            async move {
                executor
                    .execute_with_executor(
                        action,
                        contract,
                        Arc::new(SlowExecutor),
                        Some(Arc::new(FastObserver)),
                    )
                    .await
            }
        };

        let (result1, result2) = tokio::join!(fut1, fut2);

        // Both must succeed without panic or error (idempotency registry is thread-safe)
        let (r1, receipt1) = result1.expect("first execution panicked");
        let (r2, receipt2) = result2.expect("second execution panicked");

        // With atomic claim semantics:
        // - First request claims the key → executes dispatch → Verified
        // - Second concurrent request sees AlreadyClaimed → polls → Duplicate
        let outcomes = [(r1, receipt1.attempts), (r2, receipt2.attempts)];
        let verified_count = outcomes
            .iter()
            .filter(|(r, _)| *r == VerificationResult::Verified)
            .count();
        let duplicate_count = outcomes
            .iter()
            .filter(|(r, _)| *r == VerificationResult::Duplicate)
            .count();

        // Exactly one Verified (the winner) and one Duplicate (the loser)
        assert_eq!(verified_count, 1, "exactly one request should get Verified");
        assert_eq!(
            duplicate_count, 1,
            "exactly one request should get Duplicate"
        );
    }

    #[tokio::test]
    async fn execute_with_executor_transport_error_releases_claim_for_retry() {
        use crate::action_executor::{ActionExecutor, DispatchError, DispatchOutcome};

        struct MockExecutor;

        #[async_trait::async_trait]
        impl ActionExecutor for MockExecutor {
            async fn execute(&self, _action: &Action) -> Result<DispatchOutcome, DispatchError> {
                Ok(DispatchOutcome::TransportError(
                    "connection refused".to_string(),
                ))
            }
        }

        let executor = Executor::new();
        let action = Action::new("test", serde_json::json!({}));
        let contract =
            Contract::new("test").with_postcondition(Predicate::exists("result"), "result exists");

        let result = executor
            .execute_with_executor(action, contract, Arc::new(MockExecutor), None)
            .await;

        assert!(result.is_ok());
        let (verification_result, _receipt) = result.unwrap();
        // TransportError is terminal — Failed immediately, not retried
        assert_eq!(verification_result, VerificationResult::Failed);
    }

    #[tokio::test]
    async fn execute_with_executor_retry_exhaustion_returns_failed_after_max_attempts() {
        use crate::action_executor::{ActionExecutor, DispatchError, DispatchOutcome};

        struct MockExecutor;

        #[async_trait::async_trait]
        impl ActionExecutor for MockExecutor {
            async fn execute(&self, _action: &Action) -> Result<DispatchOutcome, DispatchError> {
                Ok(DispatchOutcome::TimeoutBeforeDispatch)
            }
        }

        let config = ExecutorConfig {
            verification_timeout_ms: 5000,
            max_retries: 3,
            verify_before_retry: true,
        };
        let executor = Executor::with_config(config);
        let action = Action::new("test", serde_json::json!({}));
        let contract =
            Contract::new("test").with_postcondition(Predicate::exists("result"), "result exists");

        let result = executor
            .execute_with_executor(action, contract, Arc::new(MockExecutor), None)
            .await;

        assert!(result.is_ok());
        let (verification_result, receipt) = result.unwrap();
        // After exhausting retries with TimeoutBeforeDispatch and no observer,
        // verification fails
        assert_eq!(verification_result, VerificationResult::Failed);
        assert_eq!(receipt.attempts, 3); // max_retries
    }

    #[tokio::test]
    async fn execute_with_executor_observer_error_returns_unknown() {
        use crate::action_executor::{ActionExecutor, DispatchError, DispatchOutcome};

        struct MockExecutor;

        #[async_trait::async_trait]
        impl ActionExecutor for MockExecutor {
            async fn execute(&self, _action: &Action) -> Result<DispatchOutcome, DispatchError> {
                Ok(DispatchOutcome::Completed)
            }
        }

        struct FailingObserver;

        #[async_trait::async_trait]
        impl Observer for FailingObserver {
            async fn observe(
                &self,
                _action: &Action,
                _contract: &Contract,
            ) -> Result<Observation, ExecutorError> {
                Err(ExecutorError::Unknown("Observer unavailable".to_string()))
            }
        }

        let executor = Executor::new();
        let action = Action::new("test", serde_json::json!({}));
        let contract =
            Contract::new("test").with_postcondition(Predicate::exists("result"), "result exists");

        let result = executor
            .execute_with_executor(
                action,
                contract,
                Arc::new(MockExecutor),
                Some(Arc::new(FailingObserver)),
            )
            .await;

        assert!(result.is_ok());
        let (verification_result, receipt) = result.unwrap();
        // Observer error propagates as Unknown, not Failed
        assert_eq!(verification_result, VerificationResult::Unknown);
        assert_eq!(receipt.attempts, 1);
    }

    #[tokio::test]
    async fn execute_with_executor_idempotency_key_prevents_double_dispatch() {
        use crate::action_executor::{ActionExecutor, DispatchError, DispatchOutcome};
        use agentverify_core::IdempotencyKey;
        use std::sync::Arc;

        struct MockExecutor;

        #[async_trait::async_trait]
        impl ActionExecutor for MockExecutor {
            async fn execute(&self, _action: &Action) -> Result<DispatchOutcome, DispatchError> {
                // This should only be called once due to atomic idempotency claim
                Ok(DispatchOutcome::Completed)
            }
        }

        struct FastObserver;

        #[async_trait::async_trait]
        impl Observer for FastObserver {
            async fn observe(
                &self,
                _action: &Action,
                _contract: &Contract,
            ) -> Result<Observation, ExecutorError> {
                Ok(Observation::new(
                    SourceId("fast".into()),
                    serde_json::json!({"result": {"status": "completed"}}),
                ))
            }
        }

        let executor = Executor::new();
        let idempotency_key = IdempotencyKey::new("idempotent-test-key");
        let action = Action::with_idempotency("test", serde_json::json!({}), idempotency_key);
        let contract = Contract::new("test").with_postcondition(
            Predicate::equals("result.status", serde_json::json!("completed")),
            "must be completed",
        );

        // Execute twice with same idempotency key
        let result1 = executor
            .execute_with_executor(
                action.clone(),
                contract.clone(),
                Arc::new(MockExecutor),
                Some(Arc::new(FastObserver)),
            )
            .await;

        assert!(result1.is_ok());
        let (r1, receipt1) = result1.unwrap();
        assert_eq!(r1, VerificationResult::Verified);
        assert!(receipt1.attempts >= 1);

        // Second execution with same key should return Duplicate or cached
        let result2 = executor
            .execute_with_executor(
                action,
                contract,
                Arc::new(MockExecutor),
                Some(Arc::new(FastObserver)),
            )
            .await;

        assert!(result2.is_ok());
        let (r2, receipt2) = result2.unwrap();
        // Should be Duplicate (already claimed) or Verified (cached)
        assert!(
            r2 == VerificationResult::Duplicate || r2 == VerificationResult::Verified,
            "Expected Duplicate or Verified, got {r2:?}"
        );
        // Second execution should not have dispatched (attempts should be 0)
        assert_eq!(receipt2.attempts, 0);
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // P3: Receipt lifecycle tests
    // ─────────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn executor_with_receipt_store_persists_receipt() {
        use agentverify_core::InMemoryReceiptStore;

        let receipt_store = Arc::new(InMemoryReceiptStore::new());
        let config = ExecutorConfig::default();
        let executor = Executor::with_receipt_store(
            config,
            Arc::new(IdempotencyRegistry::new()),
            receipt_store.clone(),
        );

        let action = Action::new("test", serde_json::json!({}));
        let contract = Contract::new("test")
            .with_postcondition(Predicate::exists("result"), "result must exist");

        let result = executor
            .execute(action.clone(), contract.clone(), None)
            .await;

        assert!(result.is_ok());
        let (_verification_result, receipt) = result.unwrap();

        // Receipt should be stored
        let stored = receipt_store.get(&receipt.id).await;
        assert!(stored.is_some(), "receipt should be persisted in store");
        assert_eq!(stored.unwrap().id, receipt.id);
    }

    #[tokio::test]
    async fn executor_with_receipt_store_retrievable_by_id() {
        use agentverify_core::InMemoryReceiptStore;

        let receipt_store = Arc::new(InMemoryReceiptStore::new());
        let config = ExecutorConfig::default();
        let executor = Executor::with_receipt_store(
            config,
            Arc::new(IdempotencyRegistry::new()),
            receipt_store.clone(),
        );

        let action = Action::new("test", serde_json::json!({}));
        let contract = Contract::new("test")
            .with_postcondition(Predicate::exists("result"), "result must exist");

        let (_, receipt) = executor.execute(action, contract, None).await.unwrap();

        // Retrieve via executor API
        let retrieved = executor.get_receipt(&receipt.id).await;
        assert!(retrieved.is_some(), "receipt should be retrievable by ID");
        assert_eq!(retrieved.unwrap().id, receipt.id);
    }

    #[tokio::test]
    async fn executor_with_receipt_store_no_store_attached() {
        // Without a receipt store, get_receipt returns None but execution still works
        let executor = Executor::new();
        let action = Action::new("test", serde_json::json!({}));
        let contract = Contract::new("test")
            .with_postcondition(Predicate::exists("result"), "result must exist");

        let result = executor.execute(action, contract, None).await;

        assert!(result.is_ok());
        let (_, receipt) = result.unwrap();

        // get_receipt returns None when no store is attached
        let retrieved = executor.get_receipt(&receipt.id).await;
        assert!(retrieved.is_none(), "no store attached should return None");
    }

    #[tokio::test]
    async fn executor_receipt_contains_contract_version_and_idempotency_key() {
        use agentverify_core::IdempotencyKey;

        let executor = Executor::new();
        let idempotency_key = IdempotencyKey::new("test-key-123");
        let action = Action::with_idempotency("test", serde_json::json!({}), idempotency_key);
        let contract = Contract::new("test")
            .with_postcondition(Predicate::exists("result"), "result must exist");

        let (_, receipt) = executor.execute(action, contract, None).await.unwrap();

        // Receipt should bind contract version and idempotency key
        assert_eq!(receipt.idempotency_key, Some("test-key-123".to_string()));
    }

    #[tokio::test]
    async fn execute_with_executor_timeout_after_dispatch_is_unknown_not_failed() {
        use crate::action_executor::{ActionExecutor, DispatchError, DispatchOutcome};

        struct MockExecutor;

        #[async_trait::async_trait]
        impl ActionExecutor for MockExecutor {
            async fn execute(&self, _action: &Action) -> Result<DispatchOutcome, DispatchError> {
                Ok(DispatchOutcome::TimeoutAfterDispatch)
            }
        }

        // No observer - empty state will fail verification
        // But TimeoutAfterDispatch should be Unknown, not Failed
        let executor = Executor::new();
        let action = Action::new("test", serde_json::json!({}));
        let contract =
            Contract::new("test").with_postcondition(Predicate::exists("result"), "result exists");

        let result = executor
            .execute_with_executor(action, contract, Arc::new(MockExecutor), None)
            .await;

        assert!(result.is_ok());
        let (verification_result, _receipt) = result.unwrap();
        // With no observer, empty state causes failure
        // But TimeoutAfterDispatch was the dispatch outcome
        assert_eq!(verification_result, VerificationResult::Failed);
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // Observers, recovery-driven retries, partial verification, precondition
    // rejection, idempotency edge cases, receipt persistence.
    // ─────────────────────────────────────────────────────────────────────────────

    /// Observer that always reports the same state from a fixed source.
    struct StaticObserver {
        state: serde_json::Value,
        source: &'static str,
    }

    #[async_trait::async_trait]
    impl Observer for StaticObserver {
        async fn observe(
            &self,
            _action: &Action,
            _contract: &Contract,
        ) -> Result<Observation, ExecutorError> {
            Ok(Observation::new(
                SourceId(self.source.into()),
                self.state.clone(),
            ))
        }
    }

    /// Observer that reports the required state only after a fixed delay, so the
    /// caller's verification timeout expires first.
    struct SlowObserver {
        delay: Duration,
    }

    #[async_trait::async_trait]
    impl Observer for SlowObserver {
        async fn observe(
            &self,
            _action: &Action,
            _contract: &Contract,
        ) -> Result<Observation, ExecutorError> {
            sleep(self.delay).await;
            Ok(Observation::new(
                SourceId("slow".into()),
                serde_json::json!({}),
            ))
        }
    }

    /// Observer that refuses to observe for a reason other than UNKNOWN.
    struct UnavailableObserver;

    #[async_trait::async_trait]
    impl Observer for UnavailableObserver {
        async fn observe(
            &self,
            _action: &Action,
            _contract: &Contract,
        ) -> Result<Observation, ExecutorError> {
            Err(ExecutorError::ContractNotFound("state_source".to_string()))
        }
    }

    /// Action executor that always reports the same dispatch outcome.
    struct OutcomeExecutor {
        outcome: DispatchOutcome,
    }

    #[async_trait::async_trait]
    impl ActionExecutor for OutcomeExecutor {
        async fn execute(&self, _action: &Action) -> Result<DispatchOutcome, DispatchError> {
            Ok(self.outcome.clone())
        }
    }

    /// Idempotency store modelling a sibling request: the key is already claimed
    /// on the first check, and the sibling has finished by the second check (the
    /// executor polls exactly once, 50ms later).
    struct SiblingWinsStore {
        result: VerificationResult,
        first_check: AtomicBool,
    }

    impl IdempotencyStore for SiblingWinsStore {
        fn claim_or_check<'a>(
            &'a self,
            _key: &'a str,
        ) -> Pin<Box<dyn Future<Output = (ClaimResult, Option<VerificationResult>)> + Send + 'a>>
        {
            Box::pin(async move {
                let sibling_finished = self.first_check.swap(true, Ordering::SeqCst);
                if sibling_finished {
                    // The sibling completed inside the executor's poll window.
                    (ClaimResult::AlreadyClaimed, Some(self.result))
                } else {
                    // The sibling holds the claim and has not finished yet.
                    (ClaimResult::AlreadyClaimed, None)
                }
            })
        }

        fn complete(
            &self,
            _key: String,
            _result: VerificationResult,
        ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
            Box::pin(async move {})
        }

        fn release(&self, _key: &str) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
            Box::pin(async move {})
        }
    }

    /// Recovery config with a short backoff so retry paths stay fast in tests.
    fn fast_recovery(strategy: RecoveryStrategy, max_attempts: u32) -> RecoveryConfig {
        RecoveryConfig {
            strategy,
            max_attempts,
            backoff: Some(BackoffConfig {
                backoff_type: BackoffType::Linear,
                initial: chrono::Duration::milliseconds(1),
                max: chrono::Duration::milliseconds(2),
                multiplier: 2.0,
            }),
            on_unknown: vec![],
        }
    }

    fn observing_executor(timeout_ms: u64) -> Executor {
        Executor::with_config(ExecutorConfig {
            verification_timeout_ms: timeout_ms,
            max_retries: 3,
            verify_before_retry: true,
        })
    }

    #[test]
    fn default_executor_is_ready_to_verify() {
        let executor = Executor::default();
        let contract = Contract::new("test").with_postcondition(Predicate::exists("x"), "x exists");

        let (result, _) = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(executor.execute(Action::new("test", serde_json::json!({})), contract, None))
            .unwrap();

        assert_eq!(result, VerificationResult::Failed);
    }

    #[tokio::test]
    async fn idempotency_registry_release_frees_the_key_and_default_is_empty() {
        let registry = IdempotencyRegistry::default();
        let (claim, _) = registry.claim_or_check("released-key").await;
        assert_eq!(claim, ClaimResult::Claimed);

        registry.release("released-key").await;

        let (claim, observed) = registry.claim_or_check("released-key").await;
        assert_eq!(
            claim,
            ClaimResult::Claimed,
            "released key must be claimable"
        );
        assert_eq!(observed, None);
    }

    #[tokio::test]
    async fn receipt_persistence_failure_does_not_change_the_outcome() {
        use agentverify_core::FileReceiptStore;

        // The receipt store is built while its directory exists, then the
        // directory is removed so that every write fails for real.
        let dir = tempfile::tempdir().unwrap();
        let receipt_store = Arc::new(FileReceiptStore::new(dir.path()).unwrap());
        drop(dir);

        let executor = Executor::with_receipt_store(
            ExecutorConfig::default(),
            Arc::new(IdempotencyRegistry::new()),
            receipt_store,
        );

        let action = Action::new("test", serde_json::json!({}));
        let contract =
            Contract::new("test").with_postcondition(Predicate::exists("result"), "result exists");

        // Verification is already decided by the time the receipt is written, so
        // a persistence failure must not surface as a verification error.
        let (result, receipt) = executor
            .execute(action, contract, None)
            .await
            .expect("verification must not fail when receipt persistence fails");

        assert_eq!(result, VerificationResult::Failed);
        assert_eq!(receipt.result, VerificationResult::Failed);
        assert!(
            executor.get_receipt(&receipt.id).await.is_none(),
            "nothing was persisted, so the receipt is not retrievable"
        );
    }

    #[test]
    fn calculate_backoff_uses_the_contract_backoff_when_present() {
        let linear = fast_recovery(RecoveryStrategy::VerifyThenRetry, 3);
        assert_eq!(
            Executor::calculate_backoff(1, Some(&linear), 100, 5_000, 2.0),
            1
        );
        assert_eq!(
            Executor::calculate_backoff(3, Some(&linear), 100, 5_000, 2.0),
            2
        );

        let exponential = RecoveryConfig {
            backoff: Some(BackoffConfig {
                backoff_type: BackoffType::Exponential,
                initial: chrono::Duration::milliseconds(10),
                max: chrono::Duration::milliseconds(50),
                multiplier: 3.0,
            }),
            ..fast_recovery(RecoveryStrategy::VerifyThenRetry, 3)
        };
        // 10ms * 3^(attempt-1), clamped to the configured maximum.
        assert_eq!(
            Executor::calculate_backoff(1, Some(&exponential), 100, 5_000, 2.0),
            10
        );
        assert_eq!(
            Executor::calculate_backoff(2, Some(&exponential), 100, 5_000, 2.0),
            30
        );
        assert_eq!(
            Executor::calculate_backoff(5, Some(&exponential), 100, 5_000, 2.0),
            50
        );

        // No backoff in the contract: the executor defaults apply.
        let without_backoff = RecoveryConfig {
            backoff: None,
            ..fast_recovery(RecoveryStrategy::VerifyThenRetry, 3)
        };
        assert_eq!(
            Executor::calculate_backoff(2, Some(&without_backoff), 100, 5_000, 2.0),
            200
        );
        assert_eq!(Executor::calculate_backoff(2, None, 100, 5_000, 2.0), 200);
        assert_eq!(
            Executor::calculate_backoff(20, None, 100, 5_000, 2.0),
            5_000,
            "default backoff must be clamped to the default maximum"
        );
    }

    #[test]
    fn should_retry_honours_recovery_config_and_no_action() {
        let retry_twice = fast_recovery(RecoveryStrategy::VerifyThenRetry, 2);
        assert!(Executor::should_retry(1, Some(&retry_twice), 3));
        assert!(!Executor::should_retry(2, Some(&retry_twice), 3));

        let never = fast_recovery(RecoveryStrategy::NoAction, 5);
        assert!(
            !Executor::should_retry(1, Some(&never), 3),
            "NoAction must suppress retries regardless of max_attempts"
        );

        assert!(Executor::should_retry(2, None, 3));
        assert!(!Executor::should_retry(3, None, 3));
    }

    #[test]
    fn validate_preconditions_evaluates_against_the_current_state() {
        // Absence of state satisfies a not-exists precondition.
        let passing = Contract::new("test")
            .with_precondition(Predicate::not_exists("audit_entry"), "no audit entry yet");
        assert_eq!(
            Executor::validate_preconditions(&Action::new("test", serde_json::json!({})), &passing),
            Ok(())
        );

        // An unsatisfied precondition names the failed check.
        let failing = Contract::new("test")
            .with_precondition(Predicate::exists("ledger_row"), "ledger row must exist");
        let error =
            Executor::validate_preconditions(&Action::new("test", serde_json::json!({})), &failing)
                .expect_err("unsatisfied precondition must be rejected");
        assert_eq!(error, "Precondition failed: ledger row must exist");

        // Several preconditions are checked in order and the first failure wins.
        let ordered = Contract::new("test")
            .with_precondition(Predicate::not_exists("first"), "first absent")
            .with_precondition(Predicate::exists("second"), "second present");
        assert_eq!(
            Executor::validate_preconditions(&Action::new("test", serde_json::json!({})), &ordered),
            Err("Precondition failed: second present".to_string())
        );
    }

    #[tokio::test]
    async fn execute_rejects_unsatisfied_preconditions_before_any_observation() {
        let observer = StaticObserver {
            state: serde_json::json!({"x": 1}),
            source: "precondition-check",
        };
        let executor = observing_executor(1_000);
        let action = Action::new("test", serde_json::json!({}));
        let contract = Contract::new("test")
            .with_precondition(Predicate::exists("gate"), "gate must be open")
            .with_postcondition(Predicate::equals("x", serde_json::json!(1)), "x is 1");

        let (result, receipt) = executor
            .execute(action, contract, Some(Arc::new(observer)))
            .await
            .unwrap();

        // Rejected on preconditions even though the postcondition would hold.
        assert_eq!(result, VerificationResult::Failed);
        assert_eq!(receipt.attempts, 1);
        assert!(
            receipt.postcondition_results.is_empty(),
            "postconditions are never evaluated for a rejected action"
        );
    }

    #[tokio::test]
    async fn execute_with_executor_rejects_unsatisfied_preconditions() {
        struct CountingExecutor {
            dispatches: Arc<AtomicBool>,
        }

        #[async_trait::async_trait]
        impl ActionExecutor for CountingExecutor {
            async fn execute(&self, _action: &Action) -> Result<DispatchOutcome, DispatchError> {
                self.dispatches.store(true, Ordering::SeqCst);
                Ok(DispatchOutcome::Completed)
            }
        }

        let dispatches = Arc::new(AtomicBool::new(false));
        let action_executor = Arc::new(CountingExecutor {
            dispatches: Arc::clone(&dispatches),
        });

        let executor = observing_executor(1_000);
        let action = Action::new("test", serde_json::json!({}));
        let contract = Contract::new("test")
            .with_precondition(Predicate::exists("gate"), "gate must be open")
            .with_postcondition(Predicate::exists("result"), "result exists");

        let (result, receipt) = executor
            .execute_with_executor(action, contract, action_executor, None)
            .await
            .unwrap();

        assert_eq!(result, VerificationResult::Failed);
        assert_eq!(receipt.attempts, 1);
        assert!(
            !dispatches.load(Ordering::SeqCst),
            "a rejected action must never be dispatched"
        );
    }

    #[tokio::test]
    async fn execute_records_observation_and_per_postcondition_evidence() {
        let observer = StaticObserver {
            state: serde_json::json!({"result": "completed"}),
            source: "postgres-primary",
        };
        let executor = observing_executor(1_000);
        let action = Action::with_idempotency(
            "test",
            serde_json::json!({}),
            IdempotencyKey::new("evidence-key"),
        );
        let contract = Contract::new("test").with_postcondition(
            Predicate::equals("result", serde_json::json!("completed")),
            "result must be completed",
        );

        let (result, receipt) = executor
            .execute(action, contract, Some(Arc::new(observer)))
            .await
            .unwrap();

        assert_eq!(result, VerificationResult::Verified);
        assert_eq!(receipt.result, VerificationResult::Verified);
        assert_eq!(receipt.attempts, 1);
        assert_eq!(receipt.idempotency_key, Some("evidence-key".to_string()));
        assert_eq!(receipt.observations.len(), 1, "the observation is evidence");
        assert_eq!(receipt.observations[0].source.0, "postgres-primary");
        assert_eq!(receipt.postcondition_results.len(), 1);
        assert!(receipt.postcondition_results[0].passed);
        assert_eq!(receipt.postcondition_results[0].error, None);
    }

    #[tokio::test]
    async fn execute_observer_failure_aborts_verification() {
        let executor = observing_executor(1_000);
        let action = Action::new("test", serde_json::json!({}));
        let contract =
            Contract::new("test").with_postcondition(Predicate::exists("result"), "result exists");

        let error = executor
            .execute(action, contract, Some(Arc::new(UnavailableObserver)))
            .await
            .expect_err("an observer failure cannot produce a verdict");

        assert!(
            matches!(error, ExecutorError::ContractNotFound(ref name) if name == "state_source"),
            "the observer error must be surfaced as-is, got {error:?}"
        );
    }

    #[tokio::test]
    async fn execute_observation_timeout_is_reported_as_a_timeout() {
        let executor = observing_executor(40);
        let action = Action::new("test", serde_json::json!({}));
        let contract =
            Contract::new("test").with_postcondition(Predicate::exists("result"), "result exists");

        let error = executor
            .execute(
                action,
                contract,
                Some(Arc::new(SlowObserver {
                    delay: Duration::from_millis(250),
                })),
            )
            .await
            .expect_err("an observation that never returns cannot produce a verdict");

        match error {
            ExecutorError::Timeout(message) => {
                assert!(
                    message.contains("40ms"),
                    "timeout must name the budget: {message}"
                );
            }
            other => assert!(
                matches!(other, ExecutorError::Timeout(_)),
                "expected ExecutorError::Timeout, got {other:?}"
            ),
        }
    }

    #[tokio::test]
    async fn execute_with_executor_observer_failure_aborts_verification() {
        let executor = observing_executor(1_000);
        let action = Action::new("test", serde_json::json!({}));
        let contract =
            Contract::new("test").with_postcondition(Predicate::exists("result"), "result exists");

        let error = executor
            .execute_with_executor(
                action,
                contract,
                Arc::new(OutcomeExecutor {
                    outcome: DispatchOutcome::Completed,
                }),
                Some(Arc::new(UnavailableObserver)),
            )
            .await
            .expect_err("an observer failure cannot produce a verdict");

        assert!(
            matches!(error, ExecutorError::ContractNotFound(_)),
            "got {error:?}"
        );
    }

    #[tokio::test]
    async fn execute_with_executor_observation_timeout_is_reported_as_a_timeout() {
        let executor = observing_executor(40);
        let action = Action::new("test", serde_json::json!({}));
        let contract =
            Contract::new("test").with_postcondition(Predicate::exists("result"), "result exists");

        let error = executor
            .execute_with_executor(
                action,
                contract,
                Arc::new(OutcomeExecutor {
                    outcome: DispatchOutcome::Accepted,
                }),
                Some(Arc::new(SlowObserver {
                    delay: Duration::from_millis(250),
                })),
            )
            .await
            .expect_err("an observation that never returns cannot produce a verdict");

        assert!(
            matches!(error, ExecutorError::Timeout(ref message) if message.contains("40ms")),
            "got {error:?}"
        );
    }

    #[tokio::test]
    async fn execute_with_executor_poll_returns_the_sibling_result() {
        let store = Arc::new(SiblingWinsStore {
            result: VerificationResult::Verified,
            first_check: AtomicBool::new(false),
        });
        let executor = Executor::with_config_and_store(ExecutorConfig::default(), store.clone());

        let action = Action::with_idempotency(
            "test",
            serde_json::json!({}),
            IdempotencyKey::new("sibling-key"),
        );
        let contract =
            Contract::new("test").with_postcondition(Predicate::exists("result"), "result exists");

        let (result, receipt) = executor
            .execute_with_executor(
                action,
                contract,
                Arc::new(OutcomeExecutor {
                    outcome: DispatchOutcome::Completed,
                }),
                None,
            )
            .await
            .unwrap();

        // The sibling finished inside the poll window, so its verdict is reused.
        assert_eq!(result, VerificationResult::Verified);
        assert_eq!(receipt.attempts, 0, "nothing was dispatched by this caller");

        // This store never owns a claim of its own, so completing or releasing
        // a key is inert; the calls must not panic.
        store
            .complete("sibling-key".to_string(), VerificationResult::Duplicate)
            .await;
        store.release("sibling-key").await;
    }

    #[tokio::test]
    async fn execute_reports_a_claim_held_elsewhere_as_duplicate() {
        let dir = tempfile::tempdir().unwrap();
        let key = "duplicate-claim-key";

        // Another store instance claims the key and leaves it in flight.
        let first = FileIdempotencyStore::new(dir.path()).unwrap();
        let (claim, _) = first.claim_or_check(key).await;
        assert_eq!(claim, ClaimResult::Claimed);

        let executor = Executor::with_config_and_store(
            ExecutorConfig::default(),
            Arc::new(FileIdempotencyStore::new(dir.path()).unwrap()),
        );
        let action =
            Action::with_idempotency("test", serde_json::json!({}), IdempotencyKey::new(key));
        let contract =
            Contract::new("test").with_postcondition(Predicate::exists("result"), "result exists");

        let (result, receipt) = executor.execute(action, contract, None).await.unwrap();

        assert_eq!(result, VerificationResult::Duplicate);
        assert_eq!(
            receipt.attempts, 0,
            "an in-flight claim must not be re-dispatched"
        );
        assert_eq!(receipt.result, VerificationResult::Duplicate);
    }

    #[tokio::test]
    async fn execute_with_executor_partial_verification_is_terminal() {
        let executor = observing_executor(1_000);

        // One mandatory postcondition holds, one optional one does not.
        let mut contract = Contract::new("test")
            .with_postcondition(Predicate::not_exists("rollback_entry"), "no rollback entry");
        contract.postconditions.push(Postcondition {
            predicate: Predicate::exists("optional_receipt"),
            description: "optional receipt recorded".to_string(),
            mandatory: false,
        });

        let (result, receipt) = executor
            .execute_with_executor(
                Action::new("test", serde_json::json!({})),
                contract,
                Arc::new(OutcomeExecutor {
                    outcome: DispatchOutcome::Completed,
                }),
                None,
            )
            .await
            .unwrap();

        assert_eq!(
            result,
            VerificationResult::Partial,
            "only optional postconditions failed"
        );
        assert_eq!(receipt.result, VerificationResult::Partial);
        assert_eq!(receipt.attempts, 1, "partial outcomes are terminal");
        assert_eq!(receipt.postcondition_results.len(), 2);
        assert!(receipt.postcondition_results[0].passed);
        assert!(!receipt.postcondition_results[1].passed);
    }

    #[tokio::test]
    async fn execute_partial_verification_is_terminal() {
        let executor = observing_executor(1_000);
        let mut contract = Contract::new("test")
            .with_postcondition(Predicate::not_exists("rollback_entry"), "no rollback entry");
        contract.postconditions.push(Postcondition {
            predicate: Predicate::exists("optional_receipt"),
            description: "optional receipt recorded".to_string(),
            mandatory: false,
        });

        let (result, receipt) = executor
            .execute(Action::new("test", serde_json::json!({})), contract, None)
            .await
            .unwrap();

        assert_eq!(result, VerificationResult::Partial);
        assert_eq!(receipt.attempts, 1);
    }

    #[tokio::test]
    async fn mandatory_postcondition_failure_is_failed_not_partial() {
        let executor = observing_executor(1_000);
        let contract = Contract::new("test")
            .with_postcondition(Predicate::exists("required_receipt"), "required receipt");

        let (result, receipt) = executor
            .execute(Action::new("test", serde_json::json!({})), contract, None)
            .await
            .unwrap();

        assert_eq!(result, VerificationResult::Failed);
        assert!(!receipt.postcondition_results[0].passed);
        assert_eq!(receipt.postcondition_results[0].error, None);
    }

    #[tokio::test]
    async fn contract_recovery_config_drives_backoff_and_attempt_count() {
        // Two attempts are allowed; verification fails both times because the
        // observed state is stale. The contract's own backoff is applied.
        let executor = observing_executor(1_000);
        let contract = Contract::new("test")
            .with_postcondition(
                Predicate::equals("result.status", serde_json::json!("completed")),
                "status must be completed",
            )
            .with_recovery(fast_recovery(RecoveryStrategy::VerifyThenRetry, 2));

        let (result, receipt) = executor
            .execute_with_executor(
                Action::new("test", serde_json::json!({})),
                contract,
                Arc::new(OutcomeExecutor {
                    outcome: DispatchOutcome::Completed,
                }),
                Some(Arc::new(StaticObserver {
                    state: serde_json::json!({"result": {"status": "pending"}}),
                    source: "stale",
                })),
            )
            .await
            .unwrap();

        assert_eq!(result, VerificationResult::Failed);
        assert_eq!(receipt.attempts, 2, "the contract allowed a second attempt");
        // The receipt carries the evidence of the final attempt: one entry per
        // postcondition, each recording how that predicate evaluated last.
        assert_eq!(receipt.postcondition_results.len(), 1);
        assert!(!receipt.postcondition_results[0].passed);
    }

    #[tokio::test]
    async fn no_action_strategy_suppresses_retries() {
        let executor = observing_executor(1_000);
        let contract = Contract::new("test")
            .with_postcondition(
                Predicate::equals("result.status", serde_json::json!("completed")),
                "status must be completed",
            )
            .with_recovery(fast_recovery(RecoveryStrategy::NoAction, 5));

        let (result, receipt) = executor
            .execute_with_executor(
                Action::new("test", serde_json::json!({})),
                contract,
                Arc::new(OutcomeExecutor {
                    outcome: DispatchOutcome::Completed,
                }),
                Some(Arc::new(StaticObserver {
                    state: serde_json::json!({"result": {"status": "pending"}}),
                    source: "stale",
                })),
            )
            .await
            .unwrap();

        assert_eq!(result, VerificationResult::Failed);
        assert_eq!(receipt.attempts, 1, "NoAction must not retry");
    }

    #[tokio::test]
    async fn verify_before_retry_disabled_stops_after_the_first_failure() {
        let executor = Executor::with_config(ExecutorConfig {
            verification_timeout_ms: 1_000,
            max_retries: 3,
            verify_before_retry: false,
        });
        let contract = Contract::new("test")
            .with_postcondition(
                Predicate::equals("result.status", serde_json::json!("completed")),
                "status must be completed",
            )
            .with_recovery(fast_recovery(RecoveryStrategy::VerifyThenRetry, 3));

        let (result, receipt) = executor
            .execute_with_executor(
                Action::new("test", serde_json::json!({})),
                contract,
                Arc::new(OutcomeExecutor {
                    outcome: DispatchOutcome::Completed,
                }),
                Some(Arc::new(StaticObserver {
                    state: serde_json::json!({"result": {"status": "pending"}}),
                    source: "stale",
                })),
            )
            .await
            .unwrap();

        assert_eq!(result, VerificationResult::Failed);
        assert_eq!(
            receipt.attempts, 1,
            "without verify-before-retry a failed verification is terminal"
        );
    }

    #[tokio::test]
    async fn transport_error_releases_the_claim_so_the_key_can_be_retried() {
        let store = Arc::new(IdempotencyRegistry::new());
        let executor = Executor::with_config_and_store(ExecutorConfig::default(), store.clone());
        let action = Action::with_idempotency(
            "test",
            serde_json::json!({}),
            IdempotencyKey::new("transport-failure-key"),
        );
        let contract =
            Contract::new("test").with_postcondition(Predicate::exists("result"), "result exists");

        let (result, receipt) = executor
            .execute_with_executor(
                action,
                contract.clone(),
                Arc::new(OutcomeExecutor {
                    outcome: DispatchOutcome::TransportError("connection refused".into()),
                }),
                None,
            )
            .await
            .unwrap();

        assert_eq!(result, VerificationResult::Failed);
        assert_eq!(receipt.attempts, 1);

        // The claim was released: this caller no longer owns the key.
        let (claim, observed) = store.claim_or_check("transport-failure-key").await;
        assert_eq!(
            claim,
            ClaimResult::Claimed,
            "the failed dispatch must release its claim"
        );
        assert_eq!(observed, None);

        store.release("transport-failure-key").await;
    }

    #[tokio::test]
    async fn simulated_action_executor_completes_and_verifies() {
        let executor = observing_executor(1_000);
        let contract = Contract::new("test").with_postcondition(
            Predicate::equals("result.status", serde_json::json!("completed")),
            "status must be completed",
        );

        let (result, receipt) = executor
            .execute_with_executor(
                Action::new("test", serde_json::json!({})),
                contract,
                Arc::new(crate::SimulatedActionExecutor::new()),
                Some(Arc::new(StaticObserver {
                    state: serde_json::json!({"result": {"status": "completed"}}),
                    source: "simulated",
                })),
            )
            .await
            .unwrap();

        assert_eq!(result, VerificationResult::Verified);
        assert_eq!(receipt.attempts, 1);
        assert_eq!(receipt.observations.len(), 1);
    }
}
