//! Action executor trait and dispatch outcomes
//!
//! Separates action dispatch from observation and receipt storage.

use agentverify_core::Action;
use thiserror::Error;

/// Errors that can occur during action dispatch
#[derive(Debug, Error)]
pub enum DispatchError {
    /// The target system does not support this action
    #[error("Action not supported: {0}")]
    ActionNotSupported(String),

    /// A transport-level error occurred while reaching the target system
    #[error("Transport error: {0}")]
    TransportError(String),

    /// The action timed out before it could be dispatched
    #[error("Timeout before dispatch")]
    TimeoutBeforeDispatch,

    /// The action was dispatched but the result could not be confirmed in time
    #[error("Timeout after dispatch")]
    TimeoutAfterDispatch,

    /// The target system explicitly rejected the action
    #[error("Action rejected: {0}")]
    Rejected(String),

    /// The outcome cannot be determined without reconciliation
    #[error("Ambiguous result: {0}")]
    Ambiguous(String),
}

/// Outcome of action dispatch
///
/// Distinguishes between various terminal and non-terminal states
/// to enable proper handling without inferring failure from timeout.
#[derive(Debug, Clone)]
pub enum DispatchOutcome {
    /// Action was accepted and will complete asynchronously
    Accepted,

    /// Action completed synchronously
    Completed,

    /// Action timed out before it could be dispatched
    TimeoutBeforeDispatch,

    /// Action timed out after dispatch but before we could determine result
    TimeoutAfterDispatch,

    /// Transport-level error prevented dispatch
    TransportError(String),

    /// Result is ambiguous and requires reconciliation
    Ambiguous(String),
}

impl DispatchOutcome {
    /// Returns true if the outcome represents a terminal state
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            DispatchOutcome::Completed
                | DispatchOutcome::TransportError(_)
                | DispatchOutcome::Ambiguous(_)
        )
    }

    /// Returns true if the outcome represents a timeout
    #[must_use]
    pub fn is_timeout(&self) -> bool {
        matches!(
            self,
            DispatchOutcome::TimeoutBeforeDispatch | DispatchOutcome::TimeoutAfterDispatch
        )
    }

    /// Returns true if the action should be retried
    ///
    /// Timeouts require observation to determine actual state before retry.
    #[must_use]
    pub fn should_retry(&self) -> bool {
        matches!(
            self,
            DispatchOutcome::TimeoutBeforeDispatch | DispatchOutcome::TimeoutAfterDispatch
        )
    }
}

/// Action executor trait for dispatching actions
///
/// Implement this trait to integrate with actual action systems
/// (REST APIs, databases, message queues, etc.)
#[async_trait::async_trait]
pub trait ActionExecutor: Send + Sync {
    /// Dispatch an action and return the outcome
    async fn execute(&self, action: &Action) -> Result<DispatchOutcome, DispatchError>;
}

/// A simulated action executor for testing and CLI use
///
/// This executor immediately completes actions successfully without
/// actually dispatching to any external system. Use for simulation
/// mode or when no real executor is available.
#[derive(Debug, Clone, Default)]
pub struct SimulatedActionExecutor {
    _private: (),
}

impl SimulatedActionExecutor {
    /// Create a new simulated action executor
    #[must_use]
    pub fn new() -> Self {
        Self { _private: () }
    }
}

#[async_trait::async_trait]
impl ActionExecutor for SimulatedActionExecutor {
    async fn execute(&self, _action: &Action) -> Result<DispatchOutcome, DispatchError> {
        // Simulate immediate successful completion
        Ok(DispatchOutcome::Completed)
    }
}
