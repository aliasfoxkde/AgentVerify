//! Policy error types

use crate::policy::AccessLevel;
use thiserror::Error;

/// Policy evaluation errors
#[derive(Debug, Clone, Error)]
pub enum PolicyError {
    /// Policy evaluation failed
    #[error("Policy evaluation failed: {0}")]
    EvaluationFailed(String),

    /// Rate limit configuration is invalid
    #[error("Invalid rate limit configuration: {0}")]
    InvalidRateLimit(String),

    /// Action pattern is invalid
    #[error("Invalid action pattern: {0}")]
    InvalidPattern(String),

    /// Contract validation failed
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
            Self::ActionNotAllowed(name) => write!(f, "Action '{name}' is not allowed"),
            Self::ActionBlocked(name) => write!(f, "Action '{name}' is blocked"),
            Self::EmptyActionName => write!(f, "Action name cannot be empty"),
            Self::ContractRequired(action) => {
                write!(f, "Contract required for action '{action}'")
            }
            Self::ContractInvalid(reason) => write!(f, "Contract invalid: {reason}"),
            Self::RateLimitExceeded {
                action_name,
                current_count,
                limit,
                window_secs,
            } => write!(
                f,
                "Rate limit exceeded for '{action_name}': {current_count}/{limit} in {window_secs}s window"
            ),
            Self::InsufficientAccessLevel { required, actual } => write!(
                f,
                "Insufficient access level: required {required:?}, got {actual:?}"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn policy_error_display_includes_context() {
        assert_eq!(
            PolicyError::EvaluationFailed("no rules matched".to_string()).to_string(),
            "Policy evaluation failed: no rules matched"
        );
        assert_eq!(
            PolicyError::InvalidRateLimit("max_count must be non-zero".to_string()).to_string(),
            "Invalid rate limit configuration: max_count must be non-zero"
        );
        assert_eq!(
            PolicyError::InvalidPattern("[unclosed".to_string()).to_string(),
            "Invalid action pattern: [unclosed"
        );
        assert_eq!(
            PolicyError::ContractValidation("no postconditions".to_string()).to_string(),
            "Contract validation failed: no postconditions"
        );
    }

    #[test]
    fn policy_error_is_usable_as_trait_object() {
        // The `thiserror` derive must surface `Display` through `std::error::Error`
        // so callers can handle policy failures generically.
        let errors: Vec<Box<dyn std::error::Error>> = vec![
            Box::new(PolicyError::EvaluationFailed("e".to_string())),
            Box::new(PolicyError::InvalidRateLimit("r".to_string())),
            Box::new(PolicyError::InvalidPattern("p".to_string())),
            Box::new(PolicyError::ContractValidation("c".to_string())),
        ];
        assert_eq!(errors.len(), 4);
        for err in &errors {
            assert!(!err.to_string().is_empty());
            // No variant wraps a source error.
            assert!(err.source().is_none());
        }
    }

    #[test]
    fn policy_error_is_debug_and_clone() {
        let err = PolicyError::InvalidPattern("a[".to_string());
        let cloned = err.clone();
        assert_eq!(format!("{err:?}"), format!("{cloned:?}"));
        assert!(format!("{err:?}").contains("InvalidPattern"));
    }

    #[test]
    fn policy_violation_display_covers_every_variant() {
        assert_eq!(
            PolicyViolation::ActionNotAllowed("delete_user".to_string()).to_string(),
            "Action 'delete_user' is not allowed"
        );
        assert_eq!(
            PolicyViolation::ActionBlocked("drop_table".to_string()).to_string(),
            "Action 'drop_table' is blocked"
        );
        assert_eq!(
            PolicyViolation::EmptyActionName.to_string(),
            "Action name cannot be empty"
        );
        assert_eq!(
            PolicyViolation::ContractRequired("transfer_funds".to_string()).to_string(),
            "Contract required for action 'transfer_funds'"
        );
        assert_eq!(
            PolicyViolation::ContractInvalid("no postconditions".to_string()).to_string(),
            "Contract invalid: no postconditions"
        );
        assert_eq!(
            PolicyViolation::RateLimitExceeded {
                action_name: "api_call".to_string(),
                current_count: 11,
                limit: 10,
                window_secs: 60,
            }
            .to_string(),
            "Rate limit exceeded for 'api_call': 11/10 in 60s window"
        );
        assert_eq!(
            PolicyViolation::InsufficientAccessLevel {
                required: AccessLevel::Admin,
                actual: AccessLevel::User,
            }
            .to_string(),
            "Insufficient access level: required Admin, got User"
        );
    }

    #[test]
    fn policy_violation_equality_distinguishes_payloads() {
        let limited = |count| PolicyViolation::RateLimitExceeded {
            action_name: "api_call".to_string(),
            current_count: count,
            limit: 10,
            window_secs: 60,
        };
        assert_eq!(limited(11), limited(11));
        assert_ne!(limited(11), limited(12));
        assert_ne!(
            PolicyViolation::ActionBlocked("a".to_string()),
            PolicyViolation::ActionNotAllowed("a".to_string())
        );
    }

    #[test]
    fn insufficient_access_level_display_uses_debug_of_level() {
        // `Display` for the violation intentionally renders `AccessLevel` via
        // `Debug`, so the message differs from `AccessLevel::Display`.
        let violation = PolicyViolation::InsufficientAccessLevel {
            required: AccessLevel::System,
            actual: AccessLevel::Operator,
        };
        assert_eq!(
            violation.to_string(),
            "Insufficient access level: required System, got Operator"
        );
        // The `window_secs` field is echoed verbatim, including non-default windows.
        assert_eq!(
            PolicyViolation::RateLimitExceeded {
                action_name: "a".to_string(),
                current_count: 1,
                limit: 1,
                window_secs: Duration::from_secs(90).as_secs(),
            }
            .to_string(),
            "Rate limit exceeded for 'a': 1/1 in 90s window"
        );
    }
}
