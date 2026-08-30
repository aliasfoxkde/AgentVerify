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

#[cfg(test)]
mod tests {
    use super::*;

    fn action() -> Action {
        Action::new("simulated", serde_json::json!({}))
    }

    #[test]
    fn only_completed_transport_error_and_ambiguous_are_terminal() {
        assert!(DispatchOutcome::Completed.is_terminal());
        assert!(DispatchOutcome::TransportError("connection refused".into()).is_terminal());
        assert!(DispatchOutcome::Ambiguous("state unclear".into()).is_terminal());

        assert!(!DispatchOutcome::Accepted.is_terminal());
        assert!(!DispatchOutcome::TimeoutBeforeDispatch.is_terminal());
        assert!(!DispatchOutcome::TimeoutAfterDispatch.is_terminal());
    }

    #[test]
    fn only_timeout_variants_report_timeout() {
        assert!(DispatchOutcome::TimeoutBeforeDispatch.is_timeout());
        assert!(DispatchOutcome::TimeoutAfterDispatch.is_timeout());

        assert!(!DispatchOutcome::Accepted.is_timeout());
        assert!(!DispatchOutcome::Completed.is_timeout());
        assert!(!DispatchOutcome::TransportError("dns failure".into()).is_timeout());
        assert!(!DispatchOutcome::Ambiguous("state unclear".into()).is_timeout());
    }

    #[test]
    fn should_retry_matches_timeout_variants_only() {
        // Retry is only advised for timeouts, where the real state must be
        // observed before re-dispatching (verify-before-retry).
        assert!(DispatchOutcome::TimeoutBeforeDispatch.should_retry());
        assert!(DispatchOutcome::TimeoutAfterDispatch.should_retry());

        assert!(!DispatchOutcome::Accepted.should_retry());
        assert!(!DispatchOutcome::Completed.should_retry());
        assert!(!DispatchOutcome::TransportError("connection reset".into()).should_retry());
        assert!(!DispatchOutcome::Ambiguous("state unclear".into()).should_retry());
    }

    #[test]
    fn dispatch_error_messages_identify_the_failure() {
        let cases: Vec<(DispatchError, &str)> = vec![
            (
                DispatchError::ActionNotSupported("purge_all".into()),
                "Action not supported: purge_all",
            ),
            (
                DispatchError::TransportError("connection refused".into()),
                "Transport error: connection refused",
            ),
            (
                DispatchError::TimeoutBeforeDispatch,
                "Timeout before dispatch",
            ),
            (
                DispatchError::TimeoutAfterDispatch,
                "Timeout after dispatch",
            ),
            (
                DispatchError::Rejected("quota exceeded".into()),
                "Action rejected: quota exceeded",
            ),
            (
                DispatchError::Ambiguous("split brain".into()),
                "Ambiguous result: split brain",
            ),
        ];

        for (error, expected) in cases {
            assert_eq!(error.to_string(), expected);
        }
    }

    #[test]
    fn dispatch_outcome_is_cloneable_and_debuggable() {
        let original = DispatchOutcome::TransportError("connection reset by peer".into());
        let clone = original.clone();

        let rendered = format!("{clone:?}");
        assert!(
            rendered.contains("TransportError"),
            "unexpected debug: {rendered}"
        );
        assert!(rendered.contains("connection reset by peer"));
    }

    #[tokio::test]
    async fn simulated_executor_reports_completion_without_external_calls() {
        let simulated = SimulatedActionExecutor::new();

        let outcome = simulated
            .execute(&action())
            .await
            .expect("simulated dispatch must not fail");

        assert!(
            outcome.is_terminal(),
            "simulated dispatch reports a terminal outcome, got {outcome:?}"
        );
        assert!(!outcome.is_timeout());
        assert!(!outcome.should_retry());
    }

    #[tokio::test]
    async fn simulated_executor_default_matches_new() {
        let simulated = SimulatedActionExecutor::default();

        let outcome = simulated
            .execute(&action())
            .await
            .expect("simulated dispatch must not fail");

        assert!(matches!(outcome, DispatchOutcome::Completed));
    }
}
