//! AgentVerify Policy Engine
//!
//! Policy engine for defining and evaluating access control, rate limiting,
//! and contract verification policies.
//!
//! # Core Concept
//!
//! Policies define the rules that govern which actions can be executed,
//! which contracts are required, and what rate limits apply.
//!
//! # Policy Evaluation
//!
//! Policies are evaluated in conjunction with actions and contracts. A policy
//! can:
//! - Allow or deny actions based on name patterns
//! - Require specific contracts for certain actions
//! - Apply rate limits per action type or idempotency key
//! - Define resource thresholds
//!
//! # Example
//!
//! ```ignore
//! use agentverify_policy::{Policy, PolicyEngine, PolicyConfig};
//! use agentverify_core::{Action, Contract};
//!
//! let config = PolicyConfig::default();
//! let engine = PolicyEngine::new(config);
//!
//! let policy = Policy::new("high_value_actions")
//!     .allow_action_name("transfer_funds")
//!     .require_contract_for_action("transfer_funds")
//!     .with_rate_limit("transfer_funds", 10, std::time::Duration::from_secs(60));
//!
//! let action = Action::new("transfer_funds", serde_json::json!({"amount": 50000}));
//! let result = engine.evaluate(&policy, &action, None);
//! ```

mod engine;
mod policy;
mod error;

pub use engine::PolicyEngine;
pub use error::{PolicyError, PolicyViolation};
pub use policy::{
    AccessLevel, ActionPattern, ContractRequirement, Policy, PolicyConfig, PolicyDecision,
    RateLimit,
};

#[cfg(test)]
mod tests {
    use super::*;
    use agentverify_core::{Action, Contract, Predicate};
    use serde_json::json;
    use std::time::Duration;

    #[test]
    fn test_policy_allow_action_by_name() {
        let engine = PolicyEngine::new(PolicyConfig::default());
        let policy = Policy::new("test_policy")
            .allow_action_name("create_user")
            .allow_action_name("update_user");

        let allowed_action = Action::new("create_user", serde_json::json!({}));
        let denied_action = Action::new("delete_user", serde_json::json!({}));

        assert!(matches!(
            engine.evaluate(&policy, &allowed_action, None),
            PolicyDecision::Allowed
        ));
        assert!(matches!(
            engine.evaluate(&policy, &denied_action, None),
            PolicyDecision::Denied(_)
        ));
    }

    #[test]
    fn test_policy_require_contract() {
        let engine = PolicyEngine::new(PolicyConfig::default());
        let policy = Policy::new("test_policy")
            .allow_action_name("transfer")
            .require_contract_for_action("transfer");

        let action = Action::new("transfer", serde_json::json!({}));

        // Without contract - should be denied
        assert!(matches!(
            engine.evaluate(&policy, &action, None),
            PolicyDecision::Denied(PolicyViolation::ContractRequired(_))
        ));

        // With contract - should be allowed
        let contract = Contract::new("transfer")
            .with_postcondition(Predicate::equals("status", "completed"), "transfer completed");

        assert!(matches!(
            engine.evaluate(&policy, &action, Some(&contract)),
            PolicyDecision::Allowed
        ));
    }

    #[test]
    fn test_policy_rate_limit_exceeded() {
        let config = PolicyConfig::default();
        let engine = PolicyEngine::new(config);
        let policy = Policy::new("rate_limited")
            .allow_action_name("api_call")
            .with_rate_limit("api_call", 2, Duration::from_secs(60));

        let action1 = Action::new("api_call", serde_json::json!({}));
        let action2 = Action::new("api_call", serde_json::json!({}));
        let action3 = Action::new("api_call", serde_json::json!({}));

        // First two should be allowed
        assert!(matches!(
            engine.evaluate(&policy, &action1, None),
            PolicyDecision::Allowed
        ));
        assert!(matches!(
            engine.evaluate(&policy, &action2, None),
            PolicyDecision::Allowed
        ));

        // Third should be denied due to rate limit
        assert!(matches!(
            engine.evaluate(&policy, &action3, None),
            PolicyDecision::Denied(PolicyViolation::RateLimitExceeded { .. })
        ));
    }

    #[test]
    fn test_policy_rate_limit_per_idempotency_key() {
        let config = PolicyConfig::default();
        let engine = PolicyEngine::new(config);
        let policy = Policy::new("idempotent_rate_limit")
            .allow_action_name("submit")
            .with_rate_limit_per_key("submit", 1, Duration::from_secs(60));

        let key = agentverify_core::IdempotencyKey::new("unique-key-123");
        let action1 = Action::with_idempotency("submit", serde_json::json!({}), key.clone());
        let action2 = Action::with_idempotency("submit", serde_json::json!({}), key);

        // First should be allowed
        assert!(matches!(
            engine.evaluate(&policy, &action1, None),
            PolicyDecision::Allowed
        ));

        // Second with same key should be denied
        assert!(matches!(
            engine.evaluate(&policy, &action2, None),
            PolicyDecision::Denied(PolicyViolation::RateLimitExceeded { .. })
        ));
    }

    #[test]
    fn test_policy_multiple_allow_patterns() {
        let engine = PolicyEngine::new(PolicyConfig::default());
        let policy = Policy::new("multi_pattern")
            .allow_action_pattern(ActionPattern::Exact("create_user".into()))
            .allow_action_pattern(ActionPattern::Prefix("update_".into()))
            .allow_action_pattern(ActionPattern::regex("^delete_.*$").unwrap());

        // Exact match
        assert!(matches!(
            engine.evaluate(&policy, &Action::new("create_user", json!({})), None),
            PolicyDecision::Allowed
        ));

        // Prefix match
        assert!(matches!(
            engine.evaluate(&policy, &Action::new("update_profile", json!({})), None),
            PolicyDecision::Allowed
        ));

        // Regex match
        assert!(matches!(
            engine.evaluate(&policy, &Action::new("delete_account", json!({})), None),
            PolicyDecision::Allowed
        ));

        // No match
        assert!(matches!(
            engine.evaluate(&policy, &Action::new("read_user", json!({})), None),
            PolicyDecision::Denied(_)
        ));
    }

    #[test]
    fn test_policy_blocked_action() {
        let engine = PolicyEngine::new(PolicyConfig::default());
        let policy = Policy::new("blocked_policy")
            .allow_action_name("safe_action")
            .block_action_name("dangerous_action");

        assert!(matches!(
            engine.evaluate(&policy, &Action::new("safe_action", json!({})), None),
            PolicyDecision::Allowed
        ));
        assert!(matches!(
            engine.evaluate(&policy, &Action::new("dangerous_action", json!({})), None),
            PolicyDecision::Denied(PolicyViolation::ActionBlocked(_))
        ));
    }

    #[test]
    fn test_policy_contract_validation_failure() {
        let engine = PolicyEngine::new(PolicyConfig::default());
        let policy = Policy::new("strict_policy")
            .allow_action_name("transfer")
            .require_contract_for_action("transfer");

        let action = Action::new("transfer", serde_json::json!({}));

        // Contract without postconditions should fail validation
        let invalid_contract = Contract::new("transfer");

        assert!(matches!(
            engine.evaluate(&policy, &action, Some(&invalid_contract)),
            PolicyDecision::Denied(PolicyViolation::ContractInvalid(_))
        ));
    }

    #[test]
    fn test_policy_access_level_check() {
        let config = PolicyConfig::default();
        let engine = PolicyEngine::new(config);
        let policy = Policy::new("access_control")
            .allow_action_name("admin_action")
            .require_access_level("admin_action", AccessLevel::Admin);

        let action = Action::new("admin_action", serde_json::json!({}));

        // Without access level set, should deny
        assert!(matches!(
            engine.evaluate(&policy, &action, None),
            PolicyDecision::Denied(PolicyViolation::InsufficientAccessLevel {
                required: AccessLevel::Admin,
                actual: AccessLevel::User,
            })
        ));
    }

    #[test]
    fn test_policy_empty_action_name_denied() {
        let engine = PolicyEngine::new(PolicyConfig::default());
        let policy = Policy::new("test_policy").allow_action_name("valid_action");

        let empty_action = Action::new("", serde_json::json!({}));

        assert!(matches!(
            engine.evaluate(&policy, &empty_action, None),
            PolicyDecision::Denied(PolicyViolation::EmptyActionName)
        ));
    }

    #[test]
    fn test_rate_limit_window_reset() {
        let config = PolicyConfig::default();
        let engine = PolicyEngine::new(config);
        let policy = Policy::new("short_rate_limit")
            .allow_action_name("api_call")
            .with_rate_limit("api_call", 1, Duration::from_millis(50));

        let action1 = Action::new("api_call", serde_json::json!({}));
        let action2 = Action::new("api_call", serde_json::json!({}));

        assert!(matches!(
            engine.evaluate(&policy, &action1, None),
            PolicyDecision::Allowed
        ));

        // Same action immediately after should be rate limited
        assert!(matches!(
            engine.evaluate(&policy, &action2, None),
            PolicyDecision::Denied(PolicyViolation::RateLimitExceeded { .. })
        ));

        // Wait for window to reset
        std::thread::sleep(Duration::from_millis(60));

        // Should be allowed again
        assert!(matches!(
            engine.evaluate(&policy, &action2, None),
            PolicyDecision::Allowed
        ));
    }
}
