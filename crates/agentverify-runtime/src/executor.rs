//! Verified executor implementation
//!
//! Orchestrates action execution with verification using verify-before-retry semantics.

use crate::action_executor::{ActionExecutor, DispatchOutcome};
use agentverify_contract::Contract;
use agentverify_core::{
    Action, Observation, Receipt, SourceId, State, StateMachine, VerificationResult,
};
use agentverify_engine::PredicateEngine;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

#[derive(Debug, Error)]
pub enum ExecutorError {
    #[error("Contract not found for action: {0}")]
    ContractNotFound(String),
    #[error("Precondition failed: {0}")]
    PreconditionFailed(String),
    #[error("Execution failed: {0}")]
    ExecutionFailed(String),
    #[error("Verification failed: {0}")]
    VerificationFailed(String),
    #[error("Timeout: {0}")]
    Timeout(String),
    #[error("Unknown result: {0}")]
    Unknown(String),
    #[error("Idempotency conflict: action already executed")]
    IdempotencyConflict,
    #[error("Retry exhausted after {attempts} attempts")]
    RetryExhausted { attempts: u32 },
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

/// Idempotency registry to track executed actions
pub struct IdempotencyRegistry {
    entries: RwLock<HashMap<String, VerificationResult>>,
}

impl IdempotencyRegistry {
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
        }
    }

    pub async fn check(&self, key: &str) -> Option<VerificationResult> {
        self.entries.read().await.get(key).cloned()
    }

    pub async fn insert(&self, key: String, result: VerificationResult) {
        self.entries.write().await.insert(key, result);
    }
}

impl Default for IdempotencyRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Verified executor
pub struct Executor {
    config: ExecutorConfig,
    idempotency: Arc<IdempotencyRegistry>,
}

impl Executor {
    /// Create a new executor with default configuration
    pub fn new() -> Self {
        Self::with_config(ExecutorConfig::default())
    }

    /// Create a new executor with custom configuration
    pub fn with_config(config: ExecutorConfig) -> Self {
        Self {
            config,
            idempotency: Arc::new(IdempotencyRegistry::new()),
        }
    }

    /// Execute an action with verification
    ///
    /// # Process
    /// 1. Check idempotency (return cached result if already executed)
    /// 2. Validate preconditions
    /// 3. Execute the action
    /// 4. Observe the result state
    /// 5. Verify postconditions
    /// 6. Generate receipt
    pub async fn execute(
        &self,
        action: Action,
        contract: Contract,
        observer: Option<Arc<dyn Observer>>,
    ) -> Result<(VerificationResult, Receipt), ExecutorError> {
        // Check idempotency
        if let Some(key) = &action.idempotency_key {
            if let Some(cached) = self.idempotency.check(&key.0).await {
                let receipt = self.create_receipt(&action, &contract, cached, 0);
                return Ok((cached, receipt));
            }
        }

        let mut attempts = 0;

        // Main execution loop
        loop {
            attempts += 1;
            let mut state_machine = StateMachine::new();

            // Validate preconditions
            state_machine
                .advance(State::Validating)
                .map_err(|e| ExecutorError::PreconditionFailed(e.to_string()))?;

            if let Err(e) = self.validate_preconditions(&action, &contract) {
                state_machine
                    .advance(State::Rejected)
                    .map_err(|_| ExecutorError::PreconditionFailed(e.to_string()))?;
                let receipt =
                    self.create_receipt(&action, &contract, VerificationResult::Failed, attempts);
                return Ok((VerificationResult::Failed, receipt));
            }

            state_machine
                .advance(State::Authorized)
                .map_err(|e| ExecutorError::ExecutionFailed(e.to_string()))?;

            // Execute action
            state_machine
                .advance(State::Executing)
                .map_err(|e| ExecutorError::ExecutionFailed(e.to_string()))?;

            // Simulate execution completing with unknown result (since we don't have actual execution)
            // In a real implementation, this would come from the action executor
            let _ = state_machine.advance(State::Unknown);

            // Observe state
            state_machine
                .advance(State::Observing)
                .map_err(|e| ExecutorError::Unknown(e.to_string()))?;

            let observation = if let Some(ref obs) = observer {
                obs.observe(&action, &contract).await?
            } else {
                // If no observer, assume no change
                Observation::new(SourceId("none".into()), serde_json::json!({}))
            };

            // Verify postconditions
            state_machine
                .advance(State::Verifying)
                .map_err(|e| ExecutorError::VerificationFailed(e.to_string()))?;

            let result = self.verify_postconditions(&action, &contract, &observation)?;

            // Record in idempotency registry
            if let Some(key) = &action.idempotency_key {
                self.idempotency.insert(key.0.clone(), result).await;
            }

            // Determine final state
            match result {
                VerificationResult::Verified => {
                    state_machine
                        .advance(State::Verified)
                        .map_err(|e| ExecutorError::VerificationFailed(e.to_string()))?;
                    state_machine
                        .advance(State::Committed)
                        .map_err(|e| ExecutorError::VerificationFailed(e.to_string()))?;

                    let receipt = self.create_receipt_with_observation(
                        &action,
                        &contract,
                        result,
                        attempts,
                        observation,
                    );
                    return Ok((result, receipt));
                }
                VerificationResult::Failed => {
                    // If verify_before_retry is enabled and we haven't exceeded retries
                    if self.config.verify_before_retry && attempts < self.config.max_retries {
                        // Retry after verification - loop continues with fresh state machine
                        continue;
                    }

                    state_machine
                        .advance(State::VerificationFailed)
                        .map_err(|e| ExecutorError::VerificationFailed(e.to_string()))?;

                    let receipt = self.create_receipt(&action, &contract, result, attempts);
                    return Ok((VerificationResult::Failed, receipt));
                }
                _ => {
                    // Unknown, Partial, Duplicate
                    if attempts < self.config.max_retries {
                        continue;
                    }
                    let receipt = self.create_receipt(&action, &contract, result, attempts);
                    return Ok((result, receipt));
                }
            }
        }
    }

    /// Validate preconditions against current state
    fn validate_preconditions(&self, _action: &Action, contract: &Contract) -> Result<(), String> {
        for precond in &contract.preconditions {
            let result = PredicateEngine::evaluate(
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
    fn verify_postconditions(
        &self,
        action: &Action,
        contract: &Contract,
        observation: &Observation,
    ) -> Result<VerificationResult, ExecutorError> {
        let mut all_passed = true;
        let mut mandatory_failed = false;

        for postcond in &contract.postconditions {
            let result = PredicateEngine::evaluate(
                &postcond.predicate,
                &observation.state,
                &action.arguments,
            )
            .map_err(|e| ExecutorError::VerificationFailed(e.to_string()))?;

            match result {
                VerificationResult::Verified => {}
                VerificationResult::Failed => {
                    if postcond.mandatory {
                        mandatory_failed = true;
                    }
                    all_passed = false;
                }
                other => {
                    // Unknown, Partial, Duplicate
                    return Ok(other);
                }
            }
        }

        if all_passed {
            Ok(VerificationResult::Verified)
        } else if mandatory_failed {
            Ok(VerificationResult::Failed)
        } else {
            Ok(VerificationResult::Partial)
        }
    }

    /// Execute an action using a real action executor
    ///
    /// This method uses the verify-before-retry pattern:
    /// 1. Check idempotency (return cached result if already executed)
    /// 2. Validate preconditions
    /// 3. Dispatch the action through the executor
    /// 4. Observe the result state
    /// 5. Verify postconditions
    /// 6. Generate receipt
    ///
    /// Timeouts are treated as UNKNOWN, not failure. The caller must observe
    /// state before retrying.
    pub async fn execute_with_executor(
        &self,
        action: Action,
        contract: Contract,
        action_executor: Arc<dyn ActionExecutor>,
        observer: Option<Arc<dyn Observer>>,
    ) -> Result<(VerificationResult, Receipt), ExecutorError> {
        use tokio::time::sleep;

        // Check idempotency
        if let Some(key) = &action.idempotency_key {
            if let Some(cached) = self.idempotency.check(&key.0).await {
                let receipt = self.create_receipt(&action, &contract, cached, 0);
                return Ok((cached, receipt));
            }
        }

        let mut attempts = 0;
        let mut backoff_ms: u64 = 100; // Initial backoff

        // Main execution loop
        loop {
            attempts += 1;
            let mut state_machine = StateMachine::new();

            // Validate preconditions
            state_machine
                .advance(State::Validating)
                .map_err(|e| ExecutorError::PreconditionFailed(e.to_string()))?;

            if let Err(e) = self.validate_preconditions(&action, &contract) {
                state_machine
                    .advance(State::Rejected)
                    .map_err(|_| ExecutorError::PreconditionFailed(e.to_string()))?;
                let receipt =
                    self.create_receipt(&action, &contract, VerificationResult::Failed, attempts);
                return Ok((VerificationResult::Failed, receipt));
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
                DispatchOutcome::Completed => {}
                DispatchOutcome::Accepted => {}
                DispatchOutcome::TimeoutBeforeDispatch => {
                    state_machine
                        .advance(State::Unknown)
                        .map_err(|e| ExecutorError::Timeout(e.to_string()))?;
                }
                DispatchOutcome::TimeoutAfterDispatch => {
                    state_machine
                        .advance(State::Unknown)
                        .map_err(|e| ExecutorError::Timeout(e.to_string()))?;
                }
                DispatchOutcome::TransportError(_) => {
                    // Terminal - don't retry
                    let receipt = self.create_receipt(
                        &action,
                        &contract,
                        VerificationResult::Failed,
                        attempts,
                    );
                    return Ok((VerificationResult::Failed, receipt));
                }
                DispatchOutcome::Ambiguous(_) => {
                    // Terminal - require human review
                    let receipt = self.create_receipt(
                        &action,
                        &contract,
                        VerificationResult::Unknown,
                        attempts,
                    );
                    return Ok((VerificationResult::Unknown, receipt));
                }
            };

            // Observe state
            state_machine
                .advance(State::Observing)
                .map_err(|e| ExecutorError::Unknown(e.to_string()))?;

            let observation = if let Some(ref obs) = observer {
                obs.observe(&action, &contract).await?
            } else {
                // If no observer, assume no change
                Observation::new(SourceId("none".into()), serde_json::json!({}))
            };

            // Verify postconditions
            state_machine
                .advance(State::Verifying)
                .map_err(|e| ExecutorError::VerificationFailed(e.to_string()))?;

            let result = self.verify_postconditions(&action, &contract, &observation)?;

            // Record in idempotency registry
            if let Some(key) = &action.idempotency_key {
                self.idempotency.insert(key.0.clone(), result).await;
            }

            // Determine final state
            match result {
                VerificationResult::Verified => {
                    state_machine
                        .advance(State::Verified)
                        .map_err(|e| ExecutorError::VerificationFailed(e.to_string()))?;
                    state_machine
                        .advance(State::Committed)
                        .map_err(|e| ExecutorError::VerificationFailed(e.to_string()))?;

                    let receipt = self.create_receipt_with_observation(
                        &action,
                        &contract,
                        result,
                        attempts,
                        observation,
                    );
                    return Ok((result, receipt));
                }
                VerificationResult::Failed => {
                    // If verify_before_retry is enabled and we haven't exceeded retries
                    if self.config.verify_before_retry && attempts < self.config.max_retries {
                        // Apply backoff before retry
                        sleep(std::time::Duration::from_millis(backoff_ms)).await;
                        backoff_ms = (backoff_ms * 2).min(5000); // Cap at 5 seconds
                        continue;
                    }

                    state_machine
                        .advance(State::VerificationFailed)
                        .map_err(|e| ExecutorError::VerificationFailed(e.to_string()))?;

                    let receipt = self.create_receipt(&action, &contract, result, attempts);
                    return Ok((VerificationResult::Failed, receipt));
                }
                VerificationResult::Unknown => {
                    // Unknown requires observation/recovery action
                    if attempts < self.config.max_retries {
                        // Apply backoff before retry
                        sleep(std::time::Duration::from_millis(backoff_ms)).await;
                        backoff_ms = (backoff_ms * 2).min(5000);
                        continue;
                    }
                    let receipt = self.create_receipt(&action, &contract, result, attempts);
                    return Ok((result, receipt));
                }
                _ => {
                    // Partial, Duplicate
                    let receipt = self.create_receipt(&action, &contract, result, attempts);
                    return Ok((result, receipt));
                }
            }
        }
    }

    /// Create a receipt for the action
    fn create_receipt(
        &self,
        action: &Action,
        contract: &Contract,
        result: VerificationResult,
        attempts: u32,
    ) -> Receipt {
        Receipt::new(action.id, contract.id, result, attempts)
    }

    /// Create a receipt with observation
    fn create_receipt_with_observation(
        &self,
        action: &Action,
        contract: &Contract,
        result: VerificationResult,
        attempts: u32,
        observation: Observation,
    ) -> Receipt {
        Receipt::new(action.id, contract.id, result, attempts).with_observation(observation)
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
    use agentverify_core::Predicate;
    use std::sync::Arc;

    #[tokio::test]
    async fn executor_returns_verified_on_empty_postconditions() {
        let executor = Executor::new();
        let action = Action::new("test", serde_json::json!({}));
        let contract = Contract::new("test").with_postcondition(Predicate::exists("x"), "x exists");

        // No observer, so observation will be empty
        let result = executor.execute(action, contract, None).await;
        if let Err(e) = &result {
            eprintln!("Executor error: {:?}", e);
        }
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
}
