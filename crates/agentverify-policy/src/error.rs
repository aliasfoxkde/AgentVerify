//! Policy error types

use crate::policy::AccessLevel;
use thiserror::Error;

/// Policy evaluation errors
#[derive(Debug, Clone, Error)]
pub enum PolicyError {
    #[error("Policy evaluation failed: {0}")]
    EvaluationFailed(String),

    #[error("Invalid rate limit configuration: {0}")]
    InvalidRateLimit(String),

    #[error("Invalid action pattern: {0}")]
    InvalidPattern(String),

    #[error("Contract validation failed: {0}")]
    ContractValidation(String),
}

/// Reason for a policy violation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyViolation {
    /// Action name is not in the allowed list
    ActionNotAllowed(String),

    /// Action is explicitly blocked
    ActionBlocked(String),

    /// Action name is empty
    EmptyActionName,

    /// Contract is required but not provided
    ContractRequired(String),

    /// Contract is invalid
    ContractInvalid(String),

    /// Rate limit exceeded
    RateLimitExceeded {
        /// The action that was rate limited
        action_name: String,
        /// Current count in window
        current_count: u32,
        /// Maximum allowed in window
        limit: u32,
        /// Window duration in seconds
        window_secs: u64,
    },

    /// Insufficient access level
    InsufficientAccessLevel {
        /// Required access level
        required: AccessLevel,
        /// Actual access level
        actual: AccessLevel,
    },
}

impl std::fmt::Display for PolicyViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ActionNotAllowed(name) => write!(f, "Action '{}' is not allowed", name),
            Self::ActionBlocked(name) => write!(f, "Action '{}' is blocked", name),
            Self::EmptyActionName => write!(f, "Action name cannot be empty"),
            Self::ContractRequired(action) => {
                write!(f, "Contract required for action '{}'", action)
            }
            Self::ContractInvalid(reason) => write!(f, "Contract invalid: {}", reason),
            Self::RateLimitExceeded {
                action_name,
                current_count,
                limit,
                window_secs,
            } => write!(
                f,
                "Rate limit exceeded for '{}': {}/{} in {}s window",
                action_name, current_count, limit, window_secs
            ),
            Self::InsufficientAccessLevel { required, actual } => write!(
                f,
                "Insufficient access level: required {:?}, got {:?}",
                required, actual
            ),
        }
    }
}
