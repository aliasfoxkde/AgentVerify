//! Verification result types

use serde::{Deserialize, Serialize};

/// Result of verification
///
/// # Note on UNKNOWN
///
/// UNKNOWN is a first-class state. A timeout does NOT equal failure.
/// The external system may have:
/// - Received the request but not yet persisted
/// - Partially completed
/// - Responded but the response was lost
///
/// Always verify before retrying.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationResult {
    /// All postconditions satisfied
    Verified,
    /// Postconditions not met
    Failed,
    /// Cannot determine (timeout, partial, consistency issues)
    Unknown,
    /// Some postconditions met, others not
    Partial,
    /// Action already executed (idempotent)
    Duplicate,
}

impl VerificationResult {
    /// Returns true if the result indicates success
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Verified | Self::Duplicate)
    }

    /// Returns true if the result indicates failure
    pub fn is_failure(&self) -> bool {
        matches!(self, Self::Failed)
    }

    /// Returns true if the result indicates uncertainty
    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown)
    }

    /// Returns true if retry is safe without verification.
    /// Per verify-before-retry, no result should be retried without verification.
    pub fn can_retry_without_verify(&self) -> bool {
        // Verify-before-retry: always verify state before retrying
        false
    }
}

impl std::fmt::Display for VerificationResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Verified => write!(f, "verified"),
            Self::Failed => write!(f, "failed"),
            Self::Unknown => write!(f, "unknown"),
            Self::Partial => write!(f, "partial"),
            Self::Duplicate => write!(f, "duplicate"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verified_is_success() {
        assert!(VerificationResult::Verified.is_success());
    }

    #[test]
    fn duplicate_is_success() {
        assert!(VerificationResult::Duplicate.is_success());
    }

    #[test]
    fn failed_is_not_success() {
        assert!(!VerificationResult::Failed.is_success());
    }

    #[test]
    fn unknown_is_not_failure() {
        assert!(!VerificationResult::Unknown.is_failure());
    }

    #[test]
    fn can_retry_without_verify_only_on_failure() {
        // Failed should not retry without verify - the failure could be transient
        // and we should verify the state first
        assert!(!VerificationResult::Failed.can_retry_without_verify());
        assert!(!VerificationResult::Unknown.can_retry_without_verify());
        // Verified and Duplicate don't need retry
        // Partial requires verification
    }
}
