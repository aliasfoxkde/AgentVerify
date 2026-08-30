//! Policy evaluation engine

use std::sync::{Arc, Mutex};

use crate::error::PolicyViolation;
use crate::policy::{
    AccessLevel, IdempotencyRateLimitTracker, Policy, PolicyConfig, PolicyDecision,
    RateLimitTracker,
};
use agentverify_core::{Action, Contract};

/// Policy engine for evaluating actions against policies
#[derive(Debug, Clone)]
pub struct PolicyEngine {
    /// Policy engine configuration
    #[allow(dead_code)]
    config: PolicyConfig,
    /// Rate limit tracking by action name
    rate_limit_tracker: Arc<Mutex<RateLimitTracker>>,
    /// Rate limit tracking by idempotency key
    idempotency_tracker: Arc<Mutex<IdempotencyRateLimitTracker>>,
    /// Current access level for evaluation
    current_access_level: AccessLevel,
}

impl PolicyEngine {
    /// Create a new policy engine with default configuration
    #[must_use]
    pub fn new(config: PolicyConfig) -> Self {
        Self {
            config,
            rate_limit_tracker: Arc::new(Mutex::new(RateLimitTracker::new())),
            idempotency_tracker: Arc::new(Mutex::new(IdempotencyRateLimitTracker::new())),
            current_access_level: AccessLevel::User,
        }
    }

    /// Create a new policy engine with default configuration
    #[must_use]
    pub fn with_default_config() -> Self {
        Self::new(PolicyConfig::default())
    }

    /// Set the current access level for evaluation
    #[must_use]
    pub fn with_access_level(mut self, level: AccessLevel) -> Self {
        self.current_access_level = level;
        self
    }

    /// Evaluate an action against a policy
    ///
    /// # Arguments
    /// * `policy` - The policy to evaluate against
    /// * `action` - The action to evaluate
    /// * `contract` - Optional contract associated with the action
    ///
    /// # Returns
    /// `PolicyDecision::Allowed` if the action is permitted, or
    /// `PolicyDecision::Denied` with the violation reason if not.
    #[must_use]
    pub fn evaluate(
        &self,
        policy: &Policy,
        action: &Action,
        contract: Option<&Contract>,
    ) -> PolicyDecision {
        self.evaluate_with_access(policy, action, contract, self.current_access_level)
    }

    /// Evaluate an action against a policy with a specific access level
    #[must_use]
    pub fn evaluate_with_access(
        &self,
        policy: &Policy,
        action: &Action,
        contract: Option<&Contract>,
        access_level: AccessLevel,
    ) -> PolicyDecision {
        // Check if policy is enabled
        if !policy.is_enabled() {
            return PolicyDecision::Denied(PolicyViolation::ActionNotAllowed(action.name.clone()));
        }

        // Check action name is not empty
        if action.name.is_empty() {
            return PolicyDecision::Denied(PolicyViolation::EmptyActionName);
        }

        // Check if action is blocked
        if policy.blocked_actions.contains(&action.name) {
            return PolicyDecision::Denied(PolicyViolation::ActionBlocked(action.name.clone()));
        }

        // Check if action matches allowed patterns
        if !policy.is_action_allowed(&action.name) {
            return PolicyDecision::Denied(PolicyViolation::ActionNotAllowed(action.name.clone()));
        }

        // Check access level requirements
        if let Some(required_level) = policy.get_required_access_level(&action.name) {
            if access_level < required_level {
                return PolicyDecision::Denied(PolicyViolation::InsufficientAccessLevel {
                    required: required_level,
                    actual: access_level,
                });
            }
        }

        // Check contract requirement
        if policy.is_contract_required(&action.name) {
            match contract {
                None => {
                    return PolicyDecision::Denied(PolicyViolation::ContractRequired(
                        action.name.clone(),
                    ));
                }
                Some(c) => {
                    // Validate the contract
                    if let Err(e) = c.validate() {
                        return PolicyDecision::Denied(PolicyViolation::ContractInvalid(
                            e.to_string(),
                        ));
                    }
                }
            }
        }

        // Check rate limits
        if let Some(limit) = policy.get_rate_limit(&action.name) {
            let allowed = self.check_rate_limit(&action.name, limit);
            if !allowed {
                let status = self.rate_limit_status(&action.name, limit);

                return PolicyDecision::Denied(PolicyViolation::RateLimitExceeded {
                    action_name: action.name.clone(),
                    current_count: status.current,
                    limit: status.limit,
                    window_secs: status.window_secs_remaining.max(limit.window.as_secs()),
                });
            }
        }

        // Check per-key rate limits
        if let Some(limit) = policy.get_rate_limit_per_key(&action.name) {
            if let Some(ref key) = action.idempotency_key {
                let allowed = self.check_idempotency_rate_limit(&key.0, &action.name, limit);
                if !allowed {
                    return PolicyDecision::Denied(PolicyViolation::RateLimitExceeded {
                        action_name: action.name.clone(),
                        current_count: 1, // We don't track per-key count precisely
                        limit: limit.max_count,
                        window_secs: limit.window.as_secs(),
                    });
                }
            }
        }

        PolicyDecision::Allowed
    }

    /// Check rate limit for an action (internal, mutates state)
    ///
    /// Uses a mutex for interior mutability so evaluation works through
    /// `&self`; a poisoned lock falls back to the (still consistent) guard
    /// data rather than panicking.
    fn check_rate_limit(&self, action_name: &str, limit: &crate::policy::RateLimit) -> bool {
        self.rate_limit_tracker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .check_rate_limit(action_name, limit)
    }

    /// Check idempotency rate limit (internal, mutates state)
    fn check_idempotency_rate_limit(
        &self,
        key: &str,
        action_name: &str,
        limit: &crate::policy::RateLimit,
    ) -> bool {
        self.idempotency_tracker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .check_rate_limit(key, action_name, limit)
    }

    /// Get the current rate limit status for an action
    ///
    /// # Panics
    ///
    /// Panics only if the bucket window lapses between the elapsed-time check
    /// above and the `checked_sub` below, which cannot yield a negative
    /// remainder otherwise.
    #[allow(clippy::unwrap_used)] // the guard above guarantees a positive remainder; unwrap avoids a silent zero on the narrow race
    pub fn rate_limit_status(
        &self,
        action_name: &str,
        limit: &crate::policy::RateLimit,
    ) -> RateLimitStatus {
        let tracker = self
            .rate_limit_tracker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let bucket = tracker.get_bucket(action_name);

        match bucket {
            Some(b) if b.window_start.elapsed() < limit.window => RateLimitStatus {
                current: b.count,
                limit: limit.max_count,
                remaining: limit.max_count.saturating_sub(b.count),
                window_secs_remaining: limit
                    .window
                    .checked_sub(b.window_start.elapsed())
                    .unwrap()
                    .as_secs(),
            },
            _ => RateLimitStatus {
                current: 0,
                limit: limit.max_count,
                remaining: limit.max_count,
                window_secs_remaining: 0,
            },
        }
    }

    /// Reset rate limit tracking (useful for testing)
    #[allow(dead_code)]
    pub fn reset_rate_limits(&mut self) {
        *self
            .rate_limit_tracker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = RateLimitTracker::new();
        *self
            .idempotency_tracker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            IdempotencyRateLimitTracker::new();
    }

    /// Clean up expired rate limit buckets
    #[allow(dead_code)]
    pub fn cleanup(&mut self) {
        self.rate_limit_tracker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .cleanup_expired();
        self.idempotency_tracker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .cleanup_expired();
    }
}

impl Default for PolicyEngine {
    fn default() -> Self {
        Self::with_default_config()
    }
}

/// Current rate limit status
#[derive(Debug, Clone)]
pub struct RateLimitStatus {
    /// Current count in the window
    pub current: u32,
    /// Maximum allowed in the window
    pub limit: u32,
    /// Remaining actions allowed
    pub remaining: u32,
    /// Seconds until the window resets
    pub window_secs_remaining: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::ActionPattern;
    use serde_json::json;
    use std::time::Duration;

    fn create_test_engine() -> PolicyEngine {
        PolicyEngine::new(PolicyConfig::default())
    }

    #[test]
    fn test_engine_default_allow() {
        let engine = create_test_engine();
        let policy = Policy::new("allow_all");
        let action = Action::new("any_action", json!({}));

        assert!(matches!(
            engine.evaluate(&policy, &action, None),
            PolicyDecision::Allowed
        ));
    }

    #[test]
    fn test_engine_blocks_blocked_actions() {
        let engine = create_test_engine();
        let policy = Policy::new("block_test").block_action_name("dangerous");
        let action = Action::new("dangerous", json!({}));

        assert!(matches!(
            engine.evaluate(&policy, &action, None),
            PolicyDecision::Denied(PolicyViolation::ActionBlocked(_))
        ));
    }

    #[test]
    fn test_engine_requires_contract() {
        let engine = create_test_engine();
        let policy = Policy::new("contract_required")
            .allow_action_name("transfer")
            .require_contract_for_action("transfer");

        let action = Action::new("transfer", json!({}));

        // No contract provided
        assert!(matches!(
            engine.evaluate(&policy, &action, None),
            PolicyDecision::Denied(PolicyViolation::ContractRequired(_))
        ));
    }

    #[test]
    fn test_engine_validates_contract() {
        let engine = create_test_engine();
        let policy = Policy::new("contract_required")
            .allow_action_name("transfer")
            .require_contract_for_action("transfer");

        let action = Action::new("transfer", json!({}));

        // Invalid contract (no postconditions)
        let invalid_contract = Contract::new("transfer");
        assert!(matches!(
            engine.evaluate(&policy, &action, Some(&invalid_contract)),
            PolicyDecision::Denied(PolicyViolation::ContractInvalid(_))
        ));
    }

    #[test]
    fn test_engine_valid_contract_passes() {
        let engine = create_test_engine();
        let policy = Policy::new("contract_required")
            .allow_action_name("transfer")
            .require_contract_for_action("transfer");

        let action = Action::new("transfer", json!({}));
        let valid_contract = Contract::new("transfer").with_postcondition(
            agentverify_core::Predicate::equals("status", "completed"),
            "transfer completed",
        );

        assert!(matches!(
            engine.evaluate(&policy, &action, Some(&valid_contract)),
            PolicyDecision::Allowed
        ));
    }

    #[test]
    fn test_engine_access_level_enforcement() {
        let engine = create_test_engine();
        let policy = Policy::new("admin_action")
            .allow_action_name("admin_action")
            .require_access_level("admin_action", AccessLevel::Admin);

        let action = Action::new("admin_action", json!({}));

        // With User level - should deny
        assert!(matches!(
            engine.evaluate(&policy, &action, None),
            PolicyDecision::Denied(PolicyViolation::InsufficientAccessLevel {
                required: AccessLevel::Admin,
                actual: AccessLevel::User,
            })
        ));

        // With Admin level - should allow
        assert!(matches!(
            engine.evaluate_with_access(&policy, &action, None, AccessLevel::Admin),
            PolicyDecision::Allowed
        ));
    }

    #[test]
    fn test_engine_action_pattern_matching() {
        let engine = create_test_engine();
        let policy = Policy::new("pattern_policy")
            .allow_action_pattern(ActionPattern::prefix("create_"))
            .allow_action_pattern(ActionPattern::regex("^delete_.+$").unwrap());

        assert!(matches!(
            engine.evaluate(&policy, &Action::new("create_user", json!({})), None),
            PolicyDecision::Allowed
        ));
        assert!(matches!(
            engine.evaluate(&policy, &Action::new("delete_account", json!({})), None),
            PolicyDecision::Allowed
        ));
        assert!(matches!(
            engine.evaluate(&policy, &Action::new("update_user", json!({})), None),
            PolicyDecision::Denied(_)
        ));
    }

    #[test]
    fn test_engine_disabled_policy_denies() {
        let engine = create_test_engine();
        let mut policy = Policy::new("disabled_policy").allow_action_name("any_action");
        policy.enabled = false;

        let action = Action::new("any_action", json!({}));

        assert!(matches!(
            engine.evaluate(&policy, &action, None),
            PolicyDecision::Denied(PolicyViolation::ActionNotAllowed(_))
        ));
    }

    #[test]
    fn with_default_config_matches_default_trait() {
        let built = PolicyEngine::with_default_config();
        let defaulted = PolicyEngine::default();

        let policy = Policy::new("allow_all");
        let action = Action::new("any_action", json!({}));

        assert!(matches!(
            built.evaluate(&policy, &action, None),
            PolicyDecision::Allowed
        ));
        assert!(matches!(
            defaulted.evaluate(&policy, &action, None),
            PolicyDecision::Allowed
        ));
    }

    #[test]
    fn with_access_level_builder_satisfies_requirement() {
        let engine = PolicyEngine::with_default_config().with_access_level(AccessLevel::Operator);
        let policy = Policy::new("operator_actions")
            .allow_action_name("drain_queue")
            .require_access_level("drain_queue", AccessLevel::Operator);

        let action = Action::new("drain_queue", json!({}));

        // The builder-supplied level is reused by `evaluate`.
        assert!(matches!(
            engine.evaluate(&policy, &action, None),
            PolicyDecision::Allowed
        ));

        // An explicit level still overrides the builder-supplied one.
        assert!(matches!(
            engine.evaluate_with_access(&policy, &action, None, AccessLevel::User),
            PolicyDecision::Denied(PolicyViolation::InsufficientAccessLevel {
                required: AccessLevel::Operator,
                actual: AccessLevel::User,
            })
        ));
    }

    #[test]
    fn access_level_requirement_allows_higher_level_than_required() {
        let engine = create_test_engine();
        let policy = Policy::new("escalation")
            .allow_action_name("grant_role")
            .require_access_level("grant_role", AccessLevel::Operator);

        // System outranks Operator, so the requirement is satisfied.
        assert!(matches!(
            engine.evaluate_with_access(
                &policy,
                &Action::new("grant_role", json!({})),
                None,
                AccessLevel::System
            ),
            PolicyDecision::Allowed
        ));
    }

    #[test]
    fn per_key_limit_is_ignored_without_idempotency_key() {
        let engine = create_test_engine();
        let policy = Policy::new("keyed")
            .allow_action_name("submit")
            .with_rate_limit_per_key("submit", 1, Duration::from_secs(60));

        // No idempotency key on the action: there is nothing to key the
        // bucket on, so the per-key limit does not apply.
        for _ in 0..3 {
            assert!(matches!(
                engine.evaluate(&policy, &Action::new("submit", json!({})), None),
                PolicyDecision::Allowed
            ));
        }
    }

    #[test]
    fn per_key_limit_is_scoped_to_the_action_name() {
        let engine = create_test_engine();
        let policy = Policy::new("keyed")
            .allow_action_name("submit")
            .allow_action_name("resubmit")
            .with_rate_limit_per_key("submit", 1, Duration::from_secs(60))
            .with_rate_limit_per_key("resubmit", 1, Duration::from_secs(60));

        let key = agentverify_core::IdempotencyKey::new("same-key");
        let submit = Action::with_idempotency("submit", json!({}), key.clone());
        let resubmit = Action::with_idempotency("resubmit", json!({}), key);

        assert!(matches!(
            engine.evaluate(&policy, &submit, None),
            PolicyDecision::Allowed
        ));
        // Same key, different action: separate bucket, so still allowed.
        assert!(matches!(
            engine.evaluate(&policy, &resubmit, None),
            PolicyDecision::Allowed
        ));
    }

    #[test]
    fn rate_limit_status_reports_untracked_actions_as_fully_available() {
        let engine = create_test_engine();
        let limit = crate::policy::RateLimit::new(5, Duration::from_secs(60));

        let status = engine.rate_limit_status("never_seen", &limit);
        assert_eq!(status.current, 0);
        assert_eq!(status.limit, 5);
        assert_eq!(status.remaining, 5);
        assert_eq!(status.window_secs_remaining, 0);
    }

    #[test]
    fn rate_limit_status_counts_consumed_budget() {
        let engine = create_test_engine();
        let policy = Policy::new("budget")
            .allow_action_name("api_call")
            .with_rate_limit("api_call", 4, Duration::from_secs(60));

        // Consume two of the four slots.
        assert!(matches!(
            engine.evaluate(&policy, &Action::new("api_call", json!({})), None),
            PolicyDecision::Allowed
        ));
        assert!(matches!(
            engine.evaluate(&policy, &Action::new("api_call", json!({})), None),
            PolicyDecision::Allowed
        ));

        let limit = policy.get_rate_limit("api_call").unwrap();
        let status = engine.rate_limit_status("api_call", limit);
        assert_eq!(status.current, 2);
        assert_eq!(status.limit, 4);
        assert_eq!(status.remaining, 2);
        // The window has barely started, so nearly the full window remains.
        assert!(status.window_secs_remaining <= 60);
        assert!(status.window_secs_remaining >= 59);
    }

    #[test]
    fn rate_limit_exceeded_violation_reports_consumed_window() {
        let engine = create_test_engine();
        let policy = Policy::new("tight")
            .allow_action_name("api_call")
            .with_rate_limit("api_call", 1, Duration::from_secs(60));

        assert!(matches!(
            engine.evaluate(&policy, &Action::new("api_call", json!({})), None),
            PolicyDecision::Allowed
        ));

        // The violation must report the action name, the budget that was
        // actually consumed, and a reset horizon no shorter than the window.
        assert!(matches!(
            engine.evaluate(&policy, &Action::new("api_call", json!({})), None),
            PolicyDecision::Denied(PolicyViolation::RateLimitExceeded {
                ref action_name,
                current_count,
                limit,
                window_secs,
            }) if action_name == "api_call"
                && current_count == 1
                && limit == 1
                && window_secs >= 60
        ));
    }

    #[test]
    fn reset_rate_limits_restores_exhausted_budget() {
        let mut engine = create_test_engine();
        let policy = Policy::new("reset")
            .allow_action_name("api_call")
            .with_rate_limit("api_call", 1, Duration::from_secs(60));

        assert!(matches!(
            engine.evaluate(&policy, &Action::new("api_call", json!({})), None),
            PolicyDecision::Allowed
        ));
        assert!(matches!(
            engine.evaluate(&policy, &Action::new("api_call", json!({})), None),
            PolicyDecision::Denied(PolicyViolation::RateLimitExceeded { .. })
        ));

        engine.reset_rate_limits();

        assert!(matches!(
            engine.evaluate(&policy, &Action::new("api_call", json!({})), None),
            PolicyDecision::Allowed
        ));
    }

    #[test]
    fn cleanup_keeps_live_buckets_and_releases_expired_ones() {
        let mut engine = create_test_engine();
        let live_policy = Policy::new("live")
            .allow_action_name("live_call")
            .with_rate_limit("live_call", 1, Duration::from_secs(60));

        assert!(matches!(
            engine.evaluate(&live_policy, &Action::new("live_call", json!({})), None),
            PolicyDecision::Allowed
        ));

        // `cleanup` must not discard buckets that are still inside their window,
        // otherwise the limit would silently reset.
        engine.cleanup();
        assert!(matches!(
            engine.evaluate(&live_policy, &Action::new("live_call", json!({})), None),
            PolicyDecision::Denied(PolicyViolation::RateLimitExceeded { .. })
        ));
    }

    #[test]
    fn cleanup_releases_limits_once_windows_have_elapsed() {
        let mut engine = create_test_engine();
        let policy = Policy::new("short")
            .allow_action_name("api_call")
            .with_rate_limit("api_call", 1, Duration::from_millis(20));

        assert!(matches!(
            engine.evaluate(&policy, &Action::new("api_call", json!({})), None),
            PolicyDecision::Allowed
        ));
        assert!(matches!(
            engine.evaluate(&policy, &Action::new("api_call", json!({})), None),
            PolicyDecision::Denied(PolicyViolation::RateLimitExceeded { .. })
        ));

        std::thread::sleep(Duration::from_millis(30));
        engine.cleanup();

        assert!(matches!(
            engine.evaluate(&policy, &Action::new("api_call", json!({})), None),
            PolicyDecision::Allowed
        ));
    }

    #[test]
    fn cleanup_releases_exhausted_per_key_limits() {
        let mut engine = create_test_engine();
        let policy = Policy::new("short_keyed")
            .allow_action_name("submit")
            .with_rate_limit_per_key("submit", 1, Duration::from_millis(20));

        let key = agentverify_core::IdempotencyKey::new("k-1");
        let first = Action::with_idempotency("submit", json!({}), key.clone());
        let second = Action::with_idempotency("submit", json!({}), key);

        assert!(matches!(
            engine.evaluate(&policy, &first, None),
            PolicyDecision::Allowed
        ));
        assert!(matches!(
            engine.evaluate(&policy, &second, None),
            PolicyDecision::Denied(PolicyViolation::RateLimitExceeded { .. })
        ));

        std::thread::sleep(Duration::from_millis(30));
        engine.cleanup();

        assert!(matches!(
            engine.evaluate(&policy, &second, None),
            PolicyDecision::Allowed
        ));
    }

    #[test]
    fn rate_limit_tracking_is_shared_across_clones() {
        let engine = create_test_engine();
        let cloned = engine.clone();
        let policy = Policy::new("shared")
            .allow_action_name("api_call")
            .with_rate_limit("api_call", 1, Duration::from_secs(60));

        assert!(matches!(
            engine.evaluate(&policy, &Action::new("api_call", json!({})), None),
            PolicyDecision::Allowed
        ));
        // The clone shares the same rate limit buckets, so the budget is
        // already spent.
        assert!(matches!(
            cloned.evaluate(&policy, &Action::new("api_call", json!({})), None),
            PolicyDecision::Denied(PolicyViolation::RateLimitExceeded { .. })
        ));
    }

    #[test]
    fn contract_validation_rejects_duplicate_postcondition_paths() {
        let engine = create_test_engine();
        let policy = Policy::new("contracted")
            .allow_action_name("transfer")
            .require_contract_for_action("transfer");

        let duplicated = Contract::new("transfer")
            .with_postcondition(
                agentverify_core::Predicate::equals("status", "completed"),
                "status is completed",
            )
            .with_postcondition(
                agentverify_core::Predicate::equals("status", "settled"),
                "status is settled",
            );

        assert!(matches!(
            engine.evaluate(
                &policy,
                &Action::new("transfer", json!({})),
                Some(&duplicated)
            ),
            PolicyDecision::Denied(PolicyViolation::ContractInvalid(_))
        ));
    }

    #[test]
    fn contract_requirement_is_checked_against_the_contract_itself() {
        let engine = create_test_engine();
        let policy = Policy::new("contracted")
            .allow_action_name("transfer")
            .require_contract_for_action("transfer");

        // The policy never cross-checks the contract's action name, so a valid
        // contract built for a different action still satisfies the requirement.
        let foreign = Contract::new("withdraw").with_postcondition(
            agentverify_core::Predicate::equals("status", "done"),
            "withdraw completed",
        );

        assert!(matches!(
            engine.evaluate(&policy, &Action::new("transfer", json!({})), Some(&foreign)),
            PolicyDecision::Allowed
        ));
    }
}
